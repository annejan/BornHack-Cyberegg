//! Factory test / boot supervisor — probes hardware at every cold
//! boot, returns a [`HardwareInfo`] summary the rest of the firmware
//! conditions on, and stamps a KV flag so first-boot interactive
//! tests only run once.
//!
//! ## Design (Phase 3): always-boots architecture
//!
//! Prior phases ran *after* `embassy_nrf::init(ExternalXtal)`, which
//! hangs forever on a dead 32 MHz crystal — so a broken-HFXO badge
//! would never reach the gate.  Phase 3 inverts the dependency:
//!
//! 1. `bin/embassy.rs::main` calls `embassy_nrf::init(Internal)`
//!    (always safe — no crystal dependency, runs on HFINT/64 MHz RC).
//! 2. Bare-minimum init runs: watchdog, QSPI flash, LEDs, KV store.
//!    None of these need HFXO.
//! 3. [`probe`] runs.  It actively *requests* HFXO with a short
//!    timeout and records the outcome.  On success the chip now runs
//!    on HFXO (precise clock for USB + BLE).  On failure it
//!    `TASKS_HFCLKSTOP`s the abandoned request and the chip stays
//!    on HFINT.  Either way [`probe`] **returns** — it never hangs.
//! 4. `main` consumes the returned [`HardwareInfo`] to gate the
//!    spawn of HFXO-dependent tasks (BLE, USB mass storage).
//!    HFINT-tolerant tasks (LoRa SPI, display, buzzer, watch,
//!    game) spawn unconditionally.
//!
//! Net effect: the firmware *always* boots and reports what works,
//! gracefully degrades when peripherals are dead, and never bricks
//! itself trying to wait for hardware that isn't there.
//!
//! ## What the boundaries are
//!
//! Recoverable in firmware (degrade gracefully):
//!
//! - HFXO crystal (lose USB + BLE, keep everything else)
//! - LFXO crystal (fall back to RC, lose long-term accuracy)
//! - Per-peripheral failures (battery ADC, buzzer, etc. — Phase 4+)
//!
//! Not recoverable in firmware (would need external tooling):
//!
//! - nRF52840 CPU itself, internal RAM / flash hardware, bootloader
//!   region corruption, power supply.  All detectable via SWD —
//!   factory floor catches them, the firmware doesn't try.
//!
//! ## Phases
//!
//! 1. Skeleton + KV gate ✓
//! 2. Post-init `STATE` reads (replaced by Phase 3's active probe) ✓
//! 3. **Active probe + conditional spawning** (this commit)
//! 4. Per-peripheral probes — QSPI JEDEC re-read, SX1262 LoRa
//!    version, SSD1675 EPD BUSY transitions, battery ADC sanity,
//!    buzzer beep — requires threading `Peripherals` into `probe`.
//! 5. Interactive — joystick + buttons one-at-a-time, LED cycle,
//!    qwiic continuity, full-screen display test pattern with human
//!    sign-off, beep-code feedback.
//! 6. Polish — dev override (Cancel + Execute combo at boot to
//!    re-run), KV-cache stable-result optimisation.
//!
//! ## KV gate
//!
//! Separate from the live [`HardwareInfo`] probe.  The KV flag
//! (`hwtest:passed`) only records that the *interactive* first-boot
//! tests have been signed off by a human at the factory.  Hardware
//! state is re-probed every cold boot regardless — important
//! because crystal contact is intermittent and "good once" doesn't
//! mean "good always".

use crate::fw::kv;
use embassy_time::Timer;

// ---------------------------------------------------------------------------
// nRF52840 CLOCK peripheral — raw register addresses (PAC-free).
// ---------------------------------------------------------------------------

const CLOCK_TASKS_HFCLKSTART: *mut u32 = 0x4000_0000 as *mut u32;
const CLOCK_TASKS_HFCLKSTOP: *mut u32 = 0x4000_0004 as *mut u32;
const CLOCK_EVENTS_HFCLKSTARTED: *mut u32 = 0x4000_0100 as *mut u32;
const CLOCK_HFCLKSTAT: *const u32 = 0x4000_0408 as *const u32;
const CLOCK_LFCLKSTAT: *const u32 = 0x4000_0418 as *const u32;

/// `HFCLKSTAT.STATE` bit (16): 1 when the requested HFCLK source is
/// the current source.  For HFXO requests, this means the crystal
/// is running and the chip has switched to it.
const HFCLKSTAT_STATE_RUNNING: u32 = 1 << 16;
/// `LFCLKSTAT.STATE` bit (16): 1 when LFCLK is running.
const LFCLKSTAT_STATE_RUNNING: u32 = 1 << 16;

// ---------------------------------------------------------------------------
// HardwareInfo
// ---------------------------------------------------------------------------

/// Discovered hardware state for this boot.  Returned by [`probe`]
/// for the rest of the firmware to gate conditional task spawning
/// on.
///
/// All fields default to `false`; a probe that wasn't run (or that
/// failed) leaves its corresponding field at `false`.  Callers
/// **must** treat unknown fields as "not available" — never as a
/// silent default-to-OK.
#[derive(Default, Copy, Clone, Eq, PartialEq, defmt::Format)]
pub struct HardwareInfo {
    /// 32 MHz HFXO crystal started successfully within the probe
    /// timeout.  Required for USB peripheral (48 MHz clock domain)
    /// and BLE radio (RF accuracy).
    pub hfxo_ok: bool,
    /// 32.768 kHz LFXO crystal is running.  Required for accurate
    /// long-term timekeeping (drift < 20 ppm); falling back to the
    /// internal RC means timers drift by ~250 ppm.  `false` does
    /// **not** block boot — RTC keeps ticking on either source.
    pub lfxo_ok: bool,
}

// ---------------------------------------------------------------------------
// Probe
// ---------------------------------------------------------------------------

/// Probe the on-die clock hardware and return what works.  Runs
/// every cold boot — see module docs for why this is not cached
/// across boots.  Total wall-clock cost on healthy hardware:
/// ~5 ms (HFXO typically starts in <1 ms).
///
/// **Side effect** on success: the chip is now running on HFXO.
/// On failure it stays on HFINT (whatever the caller's
/// `embassy_nrf::init` config selected, which Phase 3 mandates be
/// `Internal`).
pub async fn probe() -> HardwareInfo {
    defmt::info!("hwtest: probe start");
    let hfxo_ok = probe_hfxo().await;
    let lfxo_ok = probe_lfxo();
    let info = HardwareInfo { hfxo_ok, lfxo_ok };
    defmt::info!("hwtest: probe done — {:?}", info);
    info
}

/// Actively start HFXO with a short timeout.  Mirrors
/// `bin/hwtest.rs::probe_hfxo` but with a tighter 100 ms cap
/// because every boot pays this cost; healthy crystals start in
/// well under a millisecond.
///
/// On timeout the abandoned request is cancelled via
/// `TASKS_HFCLKSTOP` so the controller stops driving the dead
/// crystal.  HFCLK remains on whichever source `embassy_nrf::init`
/// selected (HFINT for the Phase 3 always-boots config).
async fn probe_hfxo() -> bool {
    // If HFXO is already the current source (e.g. embassy was
    // configured with ExternalXtal and init waited for it), bail
    // out immediately — no re-request needed.
    if unsafe { CLOCK_HFCLKSTAT.read_volatile() } & HFCLKSTAT_STATE_RUNNING != 0 {
        defmt::info!("hwtest: HFXO already running at probe entry");
        return true;
    }

    // Trigger a fresh start request.
    unsafe {
        CLOCK_EVENTS_HFCLKSTARTED.write_volatile(0);
        CLOCK_TASKS_HFCLKSTART.write_volatile(1);
    }

    // Poll for up to 100 ms.  Healthy nRF52840 HFXO starts in
    // ~360 µs typical, ~1.5 ms worst-case per the datasheet, so
    // 100 ms is generous slack without making boot feel sluggish.
    for _ in 0..100u16 {
        if unsafe { CLOCK_EVENTS_HFCLKSTARTED.read_volatile() } != 0 {
            defmt::info!("hwtest: HFXO probe passed");
            return true;
        }
        Timer::after_millis(1).await;
    }

    // Timeout — abandon the request so the dead crystal isn't
    // left being driven, and so a later `TASKS_HFCLKSTART` from
    // a different code path doesn't get confused.
    unsafe { CLOCK_TASKS_HFCLKSTOP.write_volatile(1) };
    defmt::warn!(
        "hwtest: HFXO probe FAILED (event never fired in 100 ms — \
         crystal dead or bad solder joint).  Boot continues on HFINT; \
         USB + BLE will be disabled this session.",
    );
    false
}

/// Read `LFCLKSTAT.STATE`.  `embassy_nrf::init` brings LFCLK up
/// regardless of source (needed by RTC1 = embassy-time), so this
/// is a "did init succeed at all" check more than a crystal probe.
/// A proper LFXO crystal test (stop-and-restart-with-XTAL) is
/// risky enough that it's deferred to Phase 4.
fn probe_lfxo() -> bool {
    let stat = unsafe { CLOCK_LFCLKSTAT.read_volatile() };
    let running = stat & LFCLKSTAT_STATE_RUNNING != 0;
    if running {
        defmt::info!("hwtest: LFXO check passed (LFCLKSTAT={:#010x})", stat);
    } else {
        defmt::warn!(
            "hwtest: LFXO check FAILED — LFCLKSTAT={:#010x}, expected STATE bit set",
            stat,
        );
    }
    running
}

// ---------------------------------------------------------------------------
// KV gate (Phase 1+2 — interactive sign-off)
// ---------------------------------------------------------------------------
//
// The first-boot interactive tests (Phase 5) stamp `hwtest:passed`
// once a human has signed off button / display / LED / sound
// behaviour.  Independent from the per-boot [`HardwareInfo`] probe
// above — the KV flag is "factory worker said yes", not "the chip
// is currently OK".

const NAMESPACE: &str = "hwtest";
const KEY_PASSED: &str = "passed";

/// Returns `true` if the badge has already passed the interactive
/// factory test.  Errors are treated as "not passed" so a flaky
/// read at boot doesn't silently let an untested badge through.
pub async fn is_passed() -> bool {
    matches!(kv::namespace(NAMESPACE).exists(KEY_PASSED).await, Ok(true))
}

/// Stamp the KV flag.  Logs but does not propagate write errors —
/// a failed write just means the test re-runs on next boot, which
/// is the safer fallback for a one-shot factory-floor gate.
pub async fn mark_passed() {
    match kv::namespace(NAMESPACE)
        .set(KEY_PASSED, &1u32.to_le_bytes(), true)
        .await
    {
        Ok(()) => defmt::info!("hwtest: marked passed"),
        Err(e) => defmt::warn!("hwtest: failed to mark passed: {:?}", e),
    }
}

/// Phase-3 placeholder for the first-boot interactive test path.
/// Currently auto-stamps after a 500 ms splash so the rest of the
/// integration (boot supervisor, conditional spawns) can be
/// exercised end-to-end.  Phase 5 will replace the body with real
/// joystick / display / buzzer prompts and only call [`mark_passed`]
/// after the human confirms.
pub async fn run_first_boot_interactive(hw: &HardwareInfo) {
    defmt::info!("hwtest: first-boot interactive entered (Phase 3 stub)");
    defmt::info!("hwtest:   hardware seen at first boot: {:?}", hw);
    Timer::after_millis(500).await;
    mark_passed().await;
    defmt::info!("hwtest: first-boot stub complete");
}

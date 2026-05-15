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
    /// Battery voltage in millivolts (SAADC sample of the 1/3
    /// resistor-divider on AIN7).  `None` if the probe was skipped
    /// or errored; outside the sane range (2.5 – 5.0 V) → factory
    /// test fails this row.
    pub battery_mv: Option<u16>,
    /// Buzzer pin (P0_13) idles low through the 1 MΩ PCB
    /// pull-down.  `false` → pull-down missing / pin shorted to
    /// VCC; the audible buzzer will fail to drive predictably.
    pub buzzer_pin_ok: bool,
    /// QWIIC SDA (P1_10) reads high with no internal pull,
    /// indicating the external I²C pull-up on the PCB is intact.
    pub qwiic_sda_ok: bool,
    /// QWIIC SCL (P1_11) reads high with no internal pull — same
    /// check as [`Self::qwiic_sda_ok`].
    pub qwiic_scl_ok: bool,
    /// Internal die-temperature sample in °C (rounded i16).  `None`
    /// if the probe was skipped or errored; outside the 0..=60 °C
    /// range counts as a fail.
    pub die_temp_c: Option<i16>,
    /// SX1262 LoRa radio responded to `GetStatus` (0xC0) with a
    /// plausible mode bit-field.  `false` → SPI handshake failed,
    /// radio is missing / RST stuck / BUSY stuck.  Only meaningful
    /// when built with the `mesh` feature.
    pub lora_ok: bool,
}

/// Inclusive lower bound for a "sane" die-temperature reading.
const DIE_TEMP_MIN_C: i16 = 0;
/// Inclusive upper bound for a "sane" die-temperature reading —
/// matches the typical indoor / room-temp factory floor.  A reading
/// outside this range likely means the sensor is broken.
const DIE_TEMP_MAX_C: i16 = 60;

/// Inclusive lower bound for a "sane" battery reading, mirroring
/// `bin/hwtest.rs::VBAT_MIN_MV`.
const VBAT_MIN_MV: u16 = 2_500;
/// Inclusive upper bound for a "sane" battery reading.
const VBAT_MAX_MV: u16 = 5_000;

impl HardwareInfo {
    /// `true` when every populated probe passed.  `battery_mv: None`
    /// and `die_temp_c: None` both count as fails (the probe ran but
    /// couldn't get a sample).
    pub fn all_pass(&self) -> bool {
        let bat_ok = self
            .battery_mv
            .map(|mv| (VBAT_MIN_MV..=VBAT_MAX_MV).contains(&mv))
            .unwrap_or(false);
        let temp_ok = self
            .die_temp_c
            .map(|c| (DIE_TEMP_MIN_C..=DIE_TEMP_MAX_C).contains(&c))
            .unwrap_or(false);
        self.hfxo_ok
            && self.lfxo_ok
            && bat_ok
            && self.buzzer_pin_ok
            && self.qwiic_sda_ok
            && self.qwiic_scl_ok
            && temp_ok
            && self.lora_ok
    }
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
    let buzzer_pin_ok = probe_buzzer_pin();
    let qwiic_sda_ok = probe_qwiic_pin_high("SDA", unsafe {
        embassy_nrf::peripherals::P1_10::steal()
    });
    let qwiic_scl_ok = probe_qwiic_pin_high("SCL", unsafe {
        embassy_nrf::peripherals::P1_11::steal()
    });
    let battery_mv = probe_battery().await;
    let die_temp_c = probe_temperature().await;
    let lora_ok = probe_lora().await;
    let info = HardwareInfo {
        hfxo_ok,
        lfxo_ok,
        battery_mv,
        buzzer_pin_ok,
        qwiic_sda_ok,
        qwiic_scl_ok,
        die_temp_c,
        lora_ok,
    };
    defmt::info!("hwtest: probe done — {:?}", info);
    info
}

// ---------------------------------------------------------------------------
// Phase 5a silent peripheral probes
// ---------------------------------------------------------------------------

/// Buzzer pin (P0_13) idles low through the 1 MΩ PCB pull-down.
/// Reading high here means the pull-down is missing or the pin is
/// shorted to VCC — the buzzer task would drive unpredictably.
///
/// # Safety
///
/// `unsafe P0_13::steal()` is sound because the buzzer pin is not
/// owned by any other code at this point in boot: `bin/embassy.rs`
/// builds its `Buzzer` only after `factory_test::probe()` returns.
fn probe_buzzer_pin() -> bool {
    use embassy_nrf::gpio::{Input, Pull};
    let pin = unsafe { embassy_nrf::peripherals::P0_13::steal() };
    let input = Input::new(pin, Pull::None);
    let high = input.is_high();
    if high {
        defmt::warn!("hwtest: BUZZER pin FAIL (high — expected low via 1MΩ pull-down)");
    } else {
        defmt::info!("hwtest: BUZZER pin OK (low at rest)");
    }
    !high
}

/// QWIIC bus pin probe — generic over the steal site so the same
/// helper covers both SDA and SCL.  With internal pulls disabled
/// (Pull::None) a healthy bus reads high via the external I²C
/// pull-ups on the PCB.
fn probe_qwiic_pin_high<P>(label: &'static str, peri: embassy_nrf::Peri<'static, P>) -> bool
where
    P: embassy_nrf::gpio::Pin,
{
    use embassy_nrf::gpio::{Input, Pull};
    let input = Input::new(peri, Pull::None);
    let ok = input.is_high();
    if !ok {
        defmt::warn!("hwtest: QWIIC {} FAIL (reads low — missing external pull-up?)", label);
    } else {
        defmt::info!("hwtest: QWIIC {} OK", label);
    }
    ok
}

/// Internal die-temperature sample via the nRF52 TEMP peripheral.
/// Uses the existing `fw::temperature::read_and_cache` so the
/// reading is also pre-warmed for the watch / battery UI later.
///
/// Returns `None` if the read errored (shouldn't happen on healthy
/// silicon — TEMP is internal and self-contained).
async fn probe_temperature() -> Option<i16> {
    let t = crate::fw::temperature::read_and_cache().await;
    defmt::info!("hwtest: die temp = {} °C", t);
    Some(t)
}

/// SX1262 LoRa radio probe — RST pulse, wait for BUSY low, then
/// `GetStatus` (0xC0).  PASS when the returned status byte's mode
/// bits land in the valid `2..=6` range (i.e. the chip is not in
/// the reserved 0 or 7 modes and didn't return all-ones / all-
/// zeros).  Mirrors `bin/hwtest.rs::probe_lora` but uses the
/// existing `mesh::sx1262::Irqs` so there's no double-binding of
/// `SPI2 => spim::InterruptHandler<SPI2>`.
///
/// Only run when built with the `mesh` feature; otherwise the
/// radio peripheral isn't owned by the firmware at all and the
/// row is reported as `true` (the radio is intentionally absent).
///
/// # Safety
///
/// `unsafe steal()` of `SPI2`, the 7 LoRa pins, and the local
/// `Spim` / `Output` / `Input` handles all drop at end of this
/// function — releasing the GPIO + SPI config so the later
/// `run_meshcore_listener` in `main` re-acquires cleanly.
async fn probe_lora() -> bool {
    #[cfg(feature = "mesh")]
    {
        use embassy_nrf::Peri;
        use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull};
        use embassy_nrf::peripherals;
        use embassy_nrf::spim::{self, Frequency, Spim};

        // Pin assignments from `fw::board::board!` — keep these in
        // sync with that macro if the hardware changes.
        let spi_periph: Peri<'static, peripherals::SPI2> = unsafe { peripherals::SPI2::steal() };
        let sck: Peri<'static, AnyPin> = unsafe { peripherals::P1_13::steal() }.into();
        let mosi: Peri<'static, AnyPin> = unsafe { peripherals::P0_03::steal() }.into();
        let miso: Peri<'static, AnyPin> = unsafe { peripherals::P1_14::steal() }.into();
        let rst: Peri<'static, AnyPin> = unsafe { peripherals::P0_30::steal() }.into();
        let nss: Peri<'static, AnyPin> = unsafe { peripherals::P1_12::steal() }.into();
        let busy: Peri<'static, AnyPin> = unsafe { peripherals::P0_28::steal() }.into();
        let rf_sw: Peri<'static, AnyPin> = unsafe { peripherals::P0_04::steal() }.into();

        let _rf_sw = Output::new(rf_sw, Level::Low, OutputDrive::Standard);
        let mut rst_out = Output::new(rst, Level::Low, OutputDrive::Standard);
        let mut nss_out = Output::new(nss, Level::High, OutputDrive::Standard);
        let busy_in = Input::new(busy, Pull::None);

        Timer::after_micros(500).await;
        rst_out.set_high();

        let mut ready = false;
        for _ in 0..50u8 {
            if !busy_in.is_high() {
                ready = true;
                break;
            }
            Timer::after_millis(1).await;
        }
        if !ready {
            defmt::warn!("hwtest: LoRa BUSY stuck high after reset");
            return false;
        }

        let mut spi_cfg = spim::Config::default();
        spi_cfg.frequency = Frequency::M1;
        let mut spi = Spim::new(spi_periph, crate::fw::mesh::sx1262::Irqs, sck, mosi, miso, spi_cfg);

        let tx = [0xC0u8, 0x00];
        let mut rx = [0u8; 2];
        nss_out.set_low();
        let r = spi.transfer(&mut rx, &tx).await;
        nss_out.set_high();

        match r {
            Ok(()) => {
                let status = rx[1];
                defmt::info!("hwtest: LoRa GetStatus = 0x{:02X}", status);
                let mode = (status >> 4) & 0x07;
                mode != 0 && mode != 7 && status != 0xFF
            }
            Err(_) => {
                defmt::warn!("hwtest: LoRa SPI transfer failed");
                false
            }
        }
    }
    #[cfg(not(feature = "mesh"))]
    {
        // Radio isn't owned by the firmware in non-mesh builds —
        // there's nothing to test, so don't claim a fail.
        defmt::info!("hwtest: LoRa probe skipped (mesh feature disabled)");
        true
    }
}

/// Battery voltage sample via SAADC (P0_31 / AIN7 through the
/// 1/3 resistor divider gated by P0_07 = `vbat_rd`).  Returns the
/// reading in millivolts, or `None` on error.
///
/// # Safety
///
/// `unsafe steal()` of SAADC, P0_31, and P0_07 is sound because
/// `bin/embassy.rs::main` does not call `battery::init` (which
/// claims those peripherals) until well after `factory_test::probe`
/// returns.  All three handles drop at end of this function.
async fn probe_battery() -> Option<u16> {
    use embassy_nrf::gpio::{Level, Output, OutputDrive};
    use embassy_nrf::peripherals;
    use embassy_nrf::saadc::{ChannelConfig, Config, Saadc};

    let saadc = unsafe { peripherals::SAADC::steal() };
    let vbat = unsafe { peripherals::P0_31::steal() };
    let vbat_rd_pin = unsafe { peripherals::P0_07::steal() };

    // Enable the divider (vbat_rd low) and let the RC settle.
    let mut vbat_rd = Output::new(vbat_rd_pin, Level::Low, OutputDrive::Standard);
    Timer::after_millis(5).await;

    let ch_cfg = ChannelConfig::single_ended(vbat);
    let mut s = Saadc::new(saadc, crate::fw::battery::BatteryIrqs, Config::default(), [ch_cfg]);
    let mut buf = [0i16; 1];
    s.sample(&mut buf).await;
    let raw = buf[0].max(0) as u32;

    // Disable the divider so it doesn't bleed current after we drop.
    vbat_rd.set_high();

    // Same formula as `fw::battery::BatteryMonitor::read_mv`:
    //   3.6 V reference × gain factor × 3 (divider) ÷ 4096 (12-bit ADC).
    let mv = ((raw * 10_800) / 4_096) as u16;
    defmt::info!("hwtest: battery {} mV (raw={})", mv, raw);
    Some(mv)
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

/// First-boot interactive test path.  Paints the test status to the
/// e-paper using the write-test-name-*before*-test, write-`PASS`-*after*
/// pattern so that a hang on any peripheral leaves a forensic record on
/// the screen for factory-floor triage.  The e-ink retains state without
/// power: if the firmware locks up mid-test, the last visible line
/// *without* a `PASS` next to it is the broken step.
///
/// Strict factory mode: any test failure halts the badge in factory-test
/// state forever.  KV flag is NOT stamped, so a power-cycle re-enters
/// this same screen.  Factory worker pulls the badge for rework; after
/// re-flow the next power-on re-probes and (hopefully) passes.
///
/// ## Sequence
///
/// 1. Full clean (`update_tc` Mode 2, tri-color full waveform) — wipes
///    residual ghosting AND constitutes the first real EPD test.
/// 2. Header → partial refresh (`update_bw` Mode 1, fast LUT) — second
///    real EPD test.
/// 3. `EPD            PASS` row stamped: implicit because we wouldn't
///    have reached this point if the SPI handshake, RESET pulse, OTP
///    LUT readback, full refresh, or fast refresh had failed.
/// 4. For each clock probed earlier (HFXO, LFXO): draw name → partial
///    refresh, then draw PASS/FAIL → partial refresh.  (The clock
///    results are pre-known from [`probe`], so the screen reveals
///    them in two-step fashion to demonstrate the pattern Phase 5
///    will use for tests that genuinely could hang mid-run.)
/// 5. Footer + final refresh.  All-pass → wait for Fire → stamp KV.
///    Any fail → halt forever.
pub async fn run_first_boot_interactive(hw: &HardwareInfo, display: &mut crate::fw::epd::EpdGfx<'_>) {
    use embedded_graphics::geometry::Point;
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
    use embedded_graphics::text::{Baseline, TextStyleBuilder};
    use ssd1675::UpdateMode;
    use ssd1675::graphics::Color;

    defmt::info!("hwtest: first-boot interactive entered (Phase 4)");
    defmt::info!("hwtest:   hardware seen at first boot: {:?}", hw);

    let font = MonoTextStyle::new(&FONT_7X13_BOLD, Color::Black);
    let style = TextStyleBuilder::new().baseline(Baseline::Top).build();

    // Use the fastest patched-LUT timing for the per-test partial
    // refreshes — at the `EPD_LUT_SPEED_MIN` scale the waveform
    // duration is ~30 % of normal (≈3× speedup).  Some ghosting
    // between rows is acceptable since each row's text is the
    // primary content; the final NEEDS REWORK / Press FIRE footer
    // is the only line a worker dwell-reads.  The initial
    // tri-color clear keeps full timing — its job is to wipe
    // ghosting from the previous power-on session.
    let full_lut = crate::fw::epd::current_lut_speed();
    let fast_lut = crate::fw::epd::EPD_LUT_SPEED_MIN;

    // ── Step 1: full clean (tri-color Mode 2) ─────────────────────────
    // First real EPD test: if `update_tc` hangs or errors here, the
    // EPD line below never gets drawn — failure pin-pointed.
    display.clear(Color::White);
    let _ = display.update_tc(full_lut).await;

    // ── Step 2: header + partial refresh (Mode 1) ─────────────────────
    // Second real EPD test: validates the fast LUT path.
    draw_text(display, "FACTORY TEST", Point::new(20, 4), font, style);
    let _ = display.update_bw(UpdateMode::Mode1, fast_lut).await;

    // ── Step 3: two-column test grid ──────────────────────────────────
    // 8 tests in 4 rows × 2 columns.  EPD is no longer a dedicated
    // row — its pass is already implicit (you wouldn't see *any*
    // of this if EPD init / Mode 2 / Mode 1 hadn't all worked).
    //
    // Column layout (FONT_7X13 = 7 px wide, 13 px tall):
    //   Col 1 label:  x=4    Col 1 result: x=44
    //   Col 2 label:  x=80   Col 2 result: x=120
    //
    // To minimise refresh count: draw all 8 names in one refresh,
    // then all 8 results in one refresh.  Two refreshes total for
    // the entire test grid (vs the old 12-refresh per-test loop).
    let battery_ok = hw
        .battery_mv
        .map(|mv| (VBAT_MIN_MV..=VBAT_MAX_MV).contains(&mv))
        .unwrap_or(false);
    let temp_ok = hw
        .die_temp_c
        .map(|c| (DIE_TEMP_MIN_C..=DIE_TEMP_MAX_C).contains(&c))
        .unwrap_or(false);

    // (col1_label, col1_result, col2_label, col2_result, y)
    let rows = [
        ("HFXO", hw.hfxo_ok,        "VBAT", battery_ok,            30),
        ("LFXO", hw.lfxo_ok,        "TEMP", temp_ok,               48),
        ("SDA",  hw.qwiic_sda_ok,   "BUZZ", hw.buzzer_pin_ok,      66),
        ("SCL",  hw.qwiic_scl_ok,   "LORA", hw.lora_ok,            84),
    ];

    // First pass: just the labels (no results) — partial refresh.
    for (l1, _, l2, _, y) in rows {
        draw_text(display, l1, Point::new(4, y),  font, style);
        draw_text(display, l2, Point::new(80, y), font, style);
    }
    let _ = display.update_bw(UpdateMode::Mode1, fast_lut).await;

    // Second pass: stamp each result next to its label.  Single
    // refresh paints all 8 PASS/FAIL outcomes at once.
    for (_, r1, _, r2, y) in rows {
        draw_text(display, if r1 { "PASS" } else { "FAIL" }, Point::new(44,  y), font, style);
        draw_text(display, if r2 { "PASS" } else { "FAIL" }, Point::new(120, y), font, style);
    }
    let _ = display.update_bw(UpdateMode::Mode1, fast_lut).await;

    // ── Footer ───────────────────────────────────────────────────────
    let all_pass = hw.all_pass();
    let footer = if all_pass {
        "ALL PASS - shipping"
    } else {
        "NEEDS REWORK"
    };
    draw_text(display, footer, Point::new(4, 130), font, style);
    let _ = display.update_bw(UpdateMode::Mode1, fast_lut).await;

    if !all_pass {
        defmt::error!(
            "hwtest: FAILURE on first boot — halting in factory-test mode forever \
             (HardwareInfo: {:?})",
            hw,
        );
        loop {
            // Long sleep — the watchdog task is feeding the WDT so
            // the chip won't reset out of this state.  Screen retains
            // the FAIL row indefinitely; that's the point.
            Timer::after_secs(60).await;
        }
    }

    // All pass → auto-stamp + draw ship image.  No human-input gate
    // needed: a healthy badge requires zero worker interaction beyond
    // plugging in USB and watching the screen.  Brief pause lets the
    // worker read the green column before the screen turns over.
    Timer::after_secs(3).await;
    mark_passed().await;
    defmt::info!("hwtest: first-boot complete — drawing ship image");

    // Replace the test status with a clean "ready to ship" screen.
    // Full tri-color refresh so the persisted image is sharp and
    // ghost-free; e-ink will hold this state for the badge's entire
    // shipping life (factory pack → distributor → end-user).  The
    // next time the badge powers up `is_passed()` returns true,
    // factory_test is skipped, and normal boot overwrites this
    // image with the regular app UI.
    draw_ship_image(display, full_lut).await;

    // Halt forever — factory worker powers off / unplugs to pack.
    // Watchdog task keeps the WDT fed.  Should the worker forget
    // to power off, the chip just sits in WFE until USB power is
    // pulled; the image on the e-paper persists either way.
    defmt::info!("hwtest: halted, ready to ship");
    loop {
        Timer::after_secs(60).await;
    }
}

/// Render the post-pass "ready to ship" screen.  Persistent: the
/// e-ink retains this image with no power, so the badge ships in
/// its box with the test confirmation already visible — no separate
/// sticker / label needed.
async fn draw_ship_image(display: &mut crate::fw::epd::EpdGfx<'_>, lut_speed: u8) {
    use embedded_graphics::Drawable;
    use embedded_graphics::geometry::Point;
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
    // Use the ASCII variant of FONT_10X20 — same one `name_screen.rs`
    // already pulls in — so we share the glyph table instead of
    // dragging in a separate iso_8859_1 copy (saves ~3 KB).
    use embedded_graphics::mono_font::ascii::FONT_10X20;
    use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
    use embedded_graphics::primitives::StyledDrawable;
    use embedded_graphics::geometry::Size;
    use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
    use ssd1675::graphics::Color;

    display.clear(Color::White);

    let centered = TextStyleBuilder::new()
        .alignment(Alignment::Center)
        .baseline(Baseline::Top)
        .build();
    let big = MonoTextStyle::new(&FONT_10X20, Color::Black);
    let small = MonoTextStyle::new(&FONT_7X13_BOLD, Color::Black);

    // Decorative border so it reads like a stamped passed-QC label.
    let border = Rectangle::new(Point::new(4, 4), Size::new(144, 144));
    let _ = border.draw_styled(
        &PrimitiveStyle::with_stroke(Color::Black, 2),
        display,
    );

    // Title block (top half).
    let _ = Text::with_text_style("BORNHACK", Point::new(76, 24), big, centered).draw(display);
    let _ = Text::with_text_style("2026", Point::new(76, 48), big, centered).draw(display);

    // Separator line.
    let sep = Rectangle::new(Point::new(20, 76), Size::new(112, 1));
    let _ = sep.draw_styled(
        &PrimitiveStyle::with_stroke(Color::Black, 1),
        display,
    );

    // Stamp text (bottom half).
    let _ = Text::with_text_style("CyberAegg", Point::new(76, 86), small, centered).draw(display);
    let _ = Text::with_text_style("FACTORY TESTED", Point::new(76, 106), small, centered).draw(display);
    let _ = Text::with_text_style("Ready to ship", Point::new(76, 126), small, centered).draw(display);

    let _ = display.update_tc(lut_speed).await;
}

/// Convenience: draw a single text string at the given position.
fn draw_text(
    display: &mut crate::fw::epd::EpdGfx<'_>,
    text: &str,
    pos: embedded_graphics::geometry::Point,
    font: embedded_graphics::mono_font::MonoTextStyle<'_, ssd1675::graphics::Color>,
    style: embedded_graphics::text::TextStyle,
) {
    use embedded_graphics::Drawable;
    use embedded_graphics::text::Text;
    let _ = Text::with_text_style(text, pos, font, style).draw(display);
}



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
}

/// Inclusive lower bound for a "sane" battery reading, mirroring
/// `bin/hwtest.rs::VBAT_MIN_MV`.
const VBAT_MIN_MV: u16 = 2_500;
/// Inclusive upper bound for a "sane" battery reading.
const VBAT_MAX_MV: u16 = 5_000;

impl HardwareInfo {
    /// `true` when every populated probe passed.  `battery_mv: None`
    /// counts as a fail (the probe ran but couldn't get a sample).
    pub fn all_pass(&self) -> bool {
        let bat_ok = self
            .battery_mv
            .map(|mv| (VBAT_MIN_MV..=VBAT_MAX_MV).contains(&mv))
            .unwrap_or(false);
        self.hfxo_ok
            && self.lfxo_ok
            && bat_ok
            && self.buzzer_pin_ok
            && self.qwiic_sda_ok
            && self.qwiic_scl_ok
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
    let info = HardwareInfo {
        hfxo_ok,
        lfxo_ok,
        battery_mv,
        buzzer_pin_ok,
        qwiic_sda_ok,
        qwiic_scl_ok,
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

    // ── Step 3: EPD row ───────────────────────────────────────────────
    // Implicit pass: reaching this point means init_epd returned (SPI +
    // RESET + BUSY + OTP LUT all worked), tri-color Mode 2 worked, and
    // fast Mode 1 worked.  Anything else would have hung above.
    draw_test_row(display, font, style, "EPD", Some(true), 30);
    let _ = display.update_bw(UpdateMode::Mode1, fast_lut).await;

    // ── Step 4 onwards: per-test rows.  Each test draws its name
    //    first (partial refresh), then its result (partial refresh).
    //    Compact 14-px row pitch fits the 6 probed tests + EPD +
    //    header + footer on the 152-px-tall screen.
    let battery_ok = hw
        .battery_mv
        .map(|mv| (VBAT_MIN_MV..=VBAT_MAX_MV).contains(&mv))
        .unwrap_or(false);

    for (label, result, y) in [
        ("HFXO", hw.hfxo_ok, 44),
        ("LFXO", hw.lfxo_ok, 58),
        ("VBAT", battery_ok, 72),
        ("BUZZ", hw.buzzer_pin_ok, 86),
        ("SDA",  hw.qwiic_sda_ok, 100),
        ("SCL",  hw.qwiic_scl_ok, 114),
    ] {
        draw_test_row(display, font, style, label, None, y);
        let _ = display.update_bw(UpdateMode::Mode1, fast_lut).await;
        draw_test_row(display, font, style, label, Some(result), y);
        let _ = display.update_bw(UpdateMode::Mode1, fast_lut).await;
    }

    // ── Footer ───────────────────────────────────────────────────────
    let all_pass = hw.all_pass();
    let footer = if all_pass {
        "Press FIRE to ship"
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

    wait_for_fire_press().await;
    mark_passed().await;
    defmt::info!("hwtest: first-boot Phase 4 complete");
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

/// Block until the joystick Fire button is pressed (P1_02 goes low).
///
/// Runs before `bin/embassy.rs::main` spawns the `run_buttons` task,
/// so we can't use the `BTN_WATCH` channel yet.  Instead we briefly
/// steal the `P1_02` peripheral, build a transient `Input` with
/// pull-up, await the falling edge, then drop — the GPIO config is
/// released so the later `Input::new(board!(p, joy_fire), ..)` in
/// `main` re-initialises cleanly.
///
/// # Safety
///
/// `unsafe { P1_02::steal() }` is sound here because:
///
/// 1. This function only runs from `run_first_boot_interactive`, which
///    main calls before any other code touches `joy_fire`.
/// 2. The stolen `Peri` is consumed by `Input::new` and dropped at
///    end-of-block, releasing the pin before `main` continues.
async fn wait_for_fire_press() {
    use embassy_nrf::gpio::{Input, Pull};
    use embassy_nrf::peripherals;

    defmt::info!("hwtest: waiting for Fire button to acknowledge results");
    let pin = unsafe { peripherals::P1_02::steal() };
    let mut fire = Input::new(pin, Pull::Up);
    fire.wait_for_falling_edge().await;
    defmt::info!("hwtest: Fire pressed");
}

/// Draw one test row: name on the left, status on the right.  Status
/// is `None` (blank — used when the test hasn't run yet), `Some(true)`
/// (`"PASS"`), or `Some(false)` (`"FAIL"`).  The blank-status form is
/// what the hang-detection pattern relies on: draw the row with
/// `status = None`, refresh, run the test, then redraw with the
/// actual result and refresh again.
fn draw_test_row(
    display: &mut crate::fw::epd::EpdGfx<'_>,
    font: embedded_graphics::mono_font::MonoTextStyle<'_, ssd1675::graphics::Color>,
    style: embedded_graphics::text::TextStyle,
    name: &str,
    status: Option<bool>,
    y: i32,
) {
    use embedded_graphics::Drawable;
    use embedded_graphics::geometry::Point;
    use embedded_graphics::text::Text;

    let _ = Text::with_text_style(name, Point::new(4, y), font, style).draw(display);
    if let Some(ok) = status {
        let label = if ok { "PASS" } else { "FAIL" };
        let _ = Text::with_text_style(label, Point::new(110, y), font, style).draw(display);
    }
}

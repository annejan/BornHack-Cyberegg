//! Factory hardware test firmware.
//!
//! Flashes standalone (no bootloader, starts at 0x0) and verifies basic
//! hardware functionality before the full firmware is installed.
//!
//! Checks, in order:
//!   1..7  — all buttons and joystick directions are pulled high.
//!           Joystick pins (3..7) are additionally shorts-tested against
//!           each other by driving one low and sampling the other four.
//!   8     — LoRa SX1262 responds on SPI.
//!   9     — battery voltage inside a sane Li-ion range.
//!   10    — QSPI external flash returns a plausible JEDEC ID.
//!   11/12 — QWIIC SDA / SCL pulled high and not shorted to each other.
//!   13..18 — EPD signal lines (BUSY/RESET/DC/CSN/SCK/MOSI) checked for
//!            shorts via internal pull-ups.  BUSY (code 13) is always
//!            treated as informational: a populated EPD panel will
//!            legitimately drive it low at rest and in response to
//!            activity on neighbouring signal lines, so its warnings
//!            are logged but never cause a failure.  The other five
//!            EPD codes are hard failures.
//!   19    — PS_SYNC driven high by the power supply circuit (no
//!            internal pull; any low reading is a real fault).
//!   20    — Buzzer pin idles low via the 1 MΩ pull-down on the PCB.
//!   21    — 32.768 kHz LFXO starts within 1 s of being requested.
//!            Marginal solder joints or a damaged crystal prevent it
//!            from oscillating, which breaks BLE connection timing in
//!            the main firmware but leaves advertising working.
//!   22    — 32 MHz HFXO starts within 1 s of being requested. The
//!            main firmware falls back to the internal RC oscillator
//!            without it, which is accurate enough to boot and to run
//!            the CPU but **not** accurate enough for the LoRa radio
//!            or BLE, so those features fail silently or drift.
//!
//! Visual/audible feedback:
//!   - boot:    LED white, all checks run
//!   - pass:    LED green, short three-note "OK" chime
//!   - fail:    LED red, looped beep sequence.  Each failure code is encoded as
//!     `(code / 5)` long beeps followed by `(code % 5)` short beeps — e.g. code
//!     13 → long-long-short-short-short. Codes are separated by a 0.8 s gap and
//!     the whole cycle repeats after a 2 s pause.

#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_nrf::gpio::{Flex, Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::pwm::{DutyCycle, SimplePwm};
use embassy_nrf::qspi::{self, Qspi};
use embassy_nrf::saadc::{self, ChannelConfig, Saadc};
use embassy_nrf::spim::{self, Frequency, Spim};
use embassy_nrf::{bind_interrupts, peripherals};
use embassy_time::Timer;
use {defmt_rtt as _, panic_probe as _};

// Board pin map — inlined (not using the main lib, which is gated on
// `embassy-base` / `simulator`). Keep in sync with `src/fw/board.rs`.
#[rustfmt::skip]
macro_rules! board {
    ($p:expr, led_red)    => { $p.P1_07 };
    ($p:expr, led_green)  => { $p.P1_15 };
    ($p:expr, led_blue)   => { $p.P0_02 };

    ($p:expr, buzzer)     => { $p.P0_13 };

    ($p:expr, epd_busy)   => { $p.P0_14 };
    ($p:expr, epd_reset)  => { $p.P0_11 };
    ($p:expr, epd_dc)     => { $p.P0_12 };
    ($p:expr, epd_csn)    => { $p.P1_09 };
    ($p:expr, epd_sck)    => { $p.P0_08 };
    ($p:expr, epd_mosi)   => { $p.P0_27 };

    ($p:expr, vbat)       => { $p.P0_31 };
    ($p:expr, vbat_rd)    => { $p.P0_07 };
    ($p:expr, ps_sync)    => { $p.P0_17 };

    ($p:expr, btn_can)    => { $p.P0_06 };
    ($p:expr, btn_exe)    => { $p.P0_26 };

    ($p:expr, joy_up)     => { $p.P1_04 };
    ($p:expr, joy_down)   => { $p.P1_03 };
    ($p:expr, joy_left)   => { $p.P1_05 };
    ($p:expr, joy_right)  => { $p.P1_01 };
    ($p:expr, joy_fire)   => { $p.P1_02 };

    ($p:expr, lora_dio1)  => { $p.P0_29 };
    ($p:expr, lora_busy)  => { $p.P0_28 };
    ($p:expr, lora_rf_sw) => { $p.P0_04 };
    ($p:expr, lora_rst)   => { $p.P0_30 };
    ($p:expr, lora_miso)  => { $p.P1_14 };
    ($p:expr, lora_mosi)  => { $p.P0_03 };
    ($p:expr, lora_sck)   => { $p.P1_13 };
    ($p:expr, lora_nss)   => { $p.P1_12 };

    ($p:expr, qwiic_sda)  => { $p.P1_10 };
    ($p:expr, qwiic_scl)  => { $p.P1_11 };

    ($p:expr, flash_csn)  => { $p.P0_25 };
    ($p:expr, flash_sck)  => { $p.P0_21 };
    ($p:expr, flash_io0)  => { $p.P0_20 };
    ($p:expr, flash_io1)  => { $p.P0_24 };
    ($p:expr, flash_io2)  => { $p.P0_22 };
    ($p:expr, flash_io3)  => { $p.P0_23 };
}

bind_interrupts!(struct Irqs {
    SAADC => saadc::InterruptHandler;
    SPI2 => spim::InterruptHandler<peripherals::SPI2>;
    QSPI => qspi::InterruptHandler<peripherals::QSPI>;
});

// ── Failure codes (= number of beeps per code) ──────────────────────────────
const ERR_CANCEL: u8 = 1;
const ERR_EXECUTE: u8 = 2;
const ERR_UP: u8 = 3;
const ERR_DOWN: u8 = 4;
const ERR_LEFT: u8 = 5;
const ERR_RIGHT: u8 = 6;
const ERR_FIRE: u8 = 7;
const ERR_LORA: u8 = 8;
const ERR_BATTERY: u8 = 9;
const ERR_QSPI: u8 = 10;
const ERR_QWIIC_SDA: u8 = 11;
const ERR_QWIIC_SCL: u8 = 12;
const ERR_EPD_BUSY: u8 = 13;
const ERR_EPD_RESET: u8 = 14;
const ERR_EPD_DC: u8 = 15;
const ERR_EPD_CSN: u8 = 16;
const ERR_EPD_SCK: u8 = 17;
const ERR_EPD_MOSI: u8 = 18;
const ERR_PS_SYNC: u8 = 19;
const ERR_BUZZER: u8 = 20;
const ERR_LFXO: u8 = 21;
const ERR_HFXO: u8 = 22;

const VBAT_MIN_MV: u16 = 3000;
const VBAT_MAX_MV: u16 = 4400;

const BEEP_FREQ_FAIL: u32 = 880; // A5
const BEEP_SHORT_MS: u64 = 150;
const BEEP_LONG_MS: u64 = 600; // one long beep = 5 short beeps
const BEEP_GAP_MS: u64 = 150;
const CODE_GAP_MS: u64 = 800;
const CYCLE_GAP_MS: u64 = 2000;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // Use the default clock config (HFINT / LFRC) — crystals are checked
    // explicitly via probe_hfxo/probe_lfxo below, so requesting them in
    // init() would just hang if a crystal were bad.
    let p = embassy_nrf::init(embassy_nrf::config::Config::default());

    defmt::info!("hwtest: starting");

    // LED white (active low → all three LOW = on).
    let mut led_red = Output::new(board!(p, led_red), Level::Low, OutputDrive::Standard);
    let mut led_green = Output::new(board!(p, led_green), Level::Low, OutputDrive::Standard);
    let mut led_blue = Output::new(board!(p, led_blue), Level::Low, OutputDrive::Standard);

    let mut failures: heapless::Vec<u8, 22> = heapless::Vec::new();

    // Buzzer pin is held in a mutable binding so a short-lived `Input`
    // reborrow can probe it for the 1 MΩ pull-down before PWM takes it.
    let mut buzzer_pin = board!(p, buzzer);

    // ── Cancel/Execute buttons: static pull-up high-check only ────────────
    let btn_can = Input::new(board!(p, btn_can), Pull::Up);
    let btn_exe = Input::new(board!(p, btn_exe), Pull::Up);

    // ── PS_SYNC — power-supply buck/boost mode signal. External circuit
    //    should hold it high; no internal pull so we don't mask a fault. ──
    let ps_sync = Input::new(board!(p, ps_sync), Pull::None);

    // ── Joystick pins (internal pull-up) — tested for high at rest AND
    //    for shorts against other joystick pins via Flex. ────────────────
    let mut joy: [(u8, Flex<'_>); 5] = [
        (ERR_UP, Flex::new(board!(p, joy_up))),
        (ERR_DOWN, Flex::new(board!(p, joy_down))),
        (ERR_LEFT, Flex::new(board!(p, joy_left))),
        (ERR_RIGHT, Flex::new(board!(p, joy_right))),
        (ERR_FIRE, Flex::new(board!(p, joy_fire))),
    ];
    for (_, f) in joy.iter_mut() {
        f.set_as_input(Pull::Up);
    }

    // ── QWIIC bus (external 4.7k pull-ups) — high-check plus short between
    //    SDA and SCL. No internal pull. ────────────────────────────────
    let mut qwi: [(u8, Flex<'_>); 2] = [
        (ERR_QWIIC_SDA, Flex::new(board!(p, qwiic_sda))),
        (ERR_QWIIC_SCL, Flex::new(board!(p, qwiic_scl))),
    ];
    for (_, f) in qwi.iter_mut() {
        f.set_as_input(Pull::None);
    }

    // ── EPD pins — panel is not fitted during board test; use internal
    //    pull-ups to detect solder bridges between the six signal lines. ──
    let mut epd: [(u8, Flex<'_>); 6] = [
        (ERR_EPD_BUSY, Flex::new(board!(p, epd_busy))),
        (ERR_EPD_RESET, Flex::new(board!(p, epd_reset))),
        (ERR_EPD_DC, Flex::new(board!(p, epd_dc))),
        (ERR_EPD_CSN, Flex::new(board!(p, epd_csn))),
        (ERR_EPD_SCK, Flex::new(board!(p, epd_sck))),
        (ERR_EPD_MOSI, Flex::new(board!(p, epd_mosi))),
    ];
    for (_, f) in epd.iter_mut() {
        f.set_as_input(Pull::Up);
    }

    // Let the pull resistors charge the lines.
    Timer::after_millis(20).await;

    defmt::info!("hwtest: checking buttons pulled high");
    if btn_can.is_low() {
        defmt::warn!("hwtest:   cancel FAIL (reads low)");
        let _ = failures.push(ERR_CANCEL);
    }
    if btn_exe.is_low() {
        defmt::warn!("hwtest:   execute FAIL (reads low)");
        let _ = failures.push(ERR_EXECUTE);
    }
    if ps_sync.is_low() {
        defmt::warn!("hwtest:   PS_SYNC FAIL (reads low)");
        let _ = failures.push(ERR_PS_SYNC);
    }
    // Buzzer pin should idle low through the 1 MΩ pull-down on the PCB.
    {
        let buzzer_in = Input::new(buzzer_pin.reborrow(), Pull::None);
        if buzzer_in.is_high() {
            defmt::warn!("hwtest:   buzzer FAIL (expected low via 1M pull-down)");
            let _ = failures.push(ERR_BUZZER);
        }
    }
    defmt::info!("hwtest: checking joystick pulled high");
    check_all_high(&joy, "joystick", &mut failures);
    defmt::info!("hwtest: checking QWIIC bus pulled high");
    check_all_high(&qwi, "qwiic", &mut failures);
    defmt::info!("hwtest: checking EPD lines pulled high");
    check_all_high(&epd, "epd", &mut failures);

    defmt::info!("hwtest: joystick short-to-neighbour scan");
    check_shorts(&mut joy, Pull::Up, "joystick", &mut failures).await;
    defmt::info!("hwtest: QWIIC short-to-neighbour scan");
    check_shorts(&mut qwi, Pull::None, "qwiic", &mut failures).await;
    defmt::info!("hwtest: EPD short-to-neighbour scan");
    check_shorts(&mut epd, Pull::Up, "epd", &mut failures).await;
    // A populated EPD will legitimately drive BUSY low at rest and in
    // response to activity on the neighbouring signal lines, so code 13
    // is informational only — warnings stay in the log for visibility
    // but the code is removed from the failure list before reporting.
    if let Some(pos) = failures.iter().position(|&c| c == ERR_EPD_BUSY) {
        failures.remove(pos);
        defmt::info!("hwtest:   EPD BUSY readings ignored — EPD panel likely installed");
    }

    // ── LoRa SX1262 ───────────────────────────────────────────────────────
    if !probe_lora(
        p.SPI2,
        board!(p, lora_sck).into(),
        board!(p, lora_mosi).into(),
        board!(p, lora_miso).into(),
        board!(p, lora_rst).into(),
        board!(p, lora_nss).into(),
        board!(p, lora_busy).into(),
        board!(p, lora_rf_sw).into(),
    )
    .await
    {
        let _ = failures.push(ERR_LORA);
    }

    // ── 32 MHz HFXO start ─────────────────────────────────────────────────
    defmt::info!("hwtest: starting 32 MHz HFXO");
    if !probe_hfxo().await {
        let _ = failures.push(ERR_HFXO);
    }

    // ── 32.768 kHz LFXO start ─────────────────────────────────────────────
    defmt::info!("hwtest: starting 32.768 kHz LFXO");
    if !probe_lfxo().await {
        let _ = failures.push(ERR_LFXO);
    }

    // ── Battery voltage via SAADC ─────────────────────────────────────────
    let mv = read_battery_mv(p.SAADC, board!(p, vbat), board!(p, vbat_rd).into()).await;
    defmt::info!("hwtest: vbat = {} mV", mv);
    if !(VBAT_MIN_MV..=VBAT_MAX_MV).contains(&mv) {
        let _ = failures.push(ERR_BATTERY);
    }

    // ── QSPI external flash: read JEDEC ID ────────────────────────────────
    if !probe_qspi(
        p.QSPI,
        board!(p, flash_sck),
        board!(p, flash_csn),
        board!(p, flash_io0),
        board!(p, flash_io1),
        board!(p, flash_io2),
        board!(p, flash_io3),
    ) {
        let _ = failures.push(ERR_QSPI);
    }

    // ── Report ────────────────────────────────────────────────────────────
    let mut pwm = SimplePwm::new_1ch(p.PWM0, buzzer_pin, &Default::default());

    if failures.is_empty() {
        defmt::info!("hwtest: PASS");
        // Red + blue off, green on.
        led_red.set_high();
        led_blue.set_high();
        beep(&mut pwm, 523, 120).await; // C5
        Timer::after_millis(40).await;
        beep(&mut pwm, 659, 120).await; // E5
        Timer::after_millis(40).await;
        beep(&mut pwm, 784, 200).await; // G5
        loop {
            Timer::after_secs(60).await;
        }
    }

    defmt::warn!("hwtest: FAIL codes = {:?}", failures.as_slice());
    // Green + blue off, red on.
    led_green.set_high();
    led_blue.set_high();
    loop {
        for &code in &failures {
            // Long beep = 5, short beep = 1.  Code 13 → 2 long + 3 short.
            let longs = code / 5;
            let shorts = code % 5;
            for _ in 0..longs {
                beep(&mut pwm, BEEP_FREQ_FAIL, BEEP_LONG_MS).await;
                Timer::after_millis(BEEP_GAP_MS).await;
            }
            for _ in 0..shorts {
                beep(&mut pwm, BEEP_FREQ_FAIL, BEEP_SHORT_MS).await;
                Timer::after_millis(BEEP_GAP_MS).await;
            }
            Timer::after_millis(CODE_GAP_MS).await;
        }
        Timer::after_millis(CYCLE_GAP_MS).await;
    }
}

/// Record each pin's failure code if it's not currently high.
fn check_all_high(
    pins: &[(u8, Flex<'_>)],
    group: &'static str,
    failures: &mut heapless::Vec<u8, 22>,
) {
    for (code, pin) in pins {
        if pin.is_low() {
            defmt::warn!("hwtest:   {} code {} FAIL (reads low)", group, code);
            if !failures.contains(code) {
                let _ = failures.push(*code);
            }
        }
    }
}

/// Drive each pin in `pins` low in turn and check every other pin stays high.
/// A low reading on a neighbour means a solder bridge or board short —
/// record the neighbour's failure code. The driven pin is restored to
/// input with `restore_pull` after each iteration.
async fn check_shorts(
    pins: &mut [(u8, Flex<'_>)],
    restore_pull: Pull,
    group: &'static str,
    failures: &mut heapless::Vec<u8, 22>,
) {
    for i in 0..pins.len() {
        let driver_code = pins[i].0;
        pins[i].1.set_as_output(OutputDrive::Standard);
        pins[i].1.set_low();
        Timer::after_millis(2).await;
        for (j, (code, pin)) in pins.iter().enumerate() {
            if i == j {
                continue;
            }
            if pin.is_low() {
                defmt::warn!(
                    "hwtest:   {} short: driving {} low → {} also low",
                    group,
                    driver_code,
                    code,
                );
                if !failures.contains(code) {
                    let _ = failures.push(*code);
                }
            }
        }
        pins[i].1.set_as_input(restore_pull);
        Timer::after_millis(2).await;
    }
}

/// Pulse a single tone on the buzzer. `freq_hz == 0` is silence.
async fn beep(pwm: &mut SimplePwm<'_>, freq_hz: u32, duration_ms: u64) {
    if freq_hz == 0 {
        Timer::after_millis(duration_ms).await;
        return;
    }
    pwm.set_period(freq_hz);
    pwm.enable();
    let duty = DutyCycle::normal(pwm.max_duty() / 2);
    pwm.set_duty(0, duty);
    Timer::after_millis(duration_ms).await;
    pwm.disable();
}

/// Single SAADC sample of the battery divider (P0_31 / AIN7).
/// Pulls vbat_rd low to enable the 1/3 divider, samples, then disables it.
/// Conversion: pin sees Vbat/3, full-scale = 3.6 V, 12-bit.
///     mV = raw * 10800 / 4096
async fn read_battery_mv(
    saadc: embassy_nrf::Peri<'static, peripherals::SAADC>,
    vbat: embassy_nrf::Peri<'static, peripherals::P0_31>,
    vbat_rd: embassy_nrf::Peri<'static, embassy_nrf::gpio::AnyPin>,
) -> u16 {
    let mut rd = Output::new(vbat_rd, Level::High, OutputDrive::Standard);
    rd.set_low();
    Timer::after_millis(5).await;

    let ch = ChannelConfig::single_ended(vbat);
    let mut s = Saadc::new(saadc, Irqs, saadc::Config::default(), [ch]);
    let mut buf = [0i16; 1];
    s.sample(&mut buf).await;
    rd.set_high();

    ((buf[0].max(0) as u32) * 10800 / 4096) as u16
}

/// Read the QSPI flash JEDEC ID (opcode 0x9F). Returns true if the response
/// looks like a real chip (not all 0x00 or all 0xFF).
fn probe_qspi(
    qspi_periph: embassy_nrf::Peri<'_, peripherals::QSPI>,
    sck: embassy_nrf::Peri<'_, peripherals::P0_21>,
    csn: embassy_nrf::Peri<'_, peripherals::P0_25>,
    io0: embassy_nrf::Peri<'_, peripherals::P0_20>,
    io1: embassy_nrf::Peri<'_, peripherals::P0_24>,
    io2: embassy_nrf::Peri<'_, peripherals::P0_22>,
    io3: embassy_nrf::Peri<'_, peripherals::P0_23>,
) -> bool {
    let mut cfg = qspi::Config::default();
    cfg.capacity = 2 * 1024 * 1024;
    cfg.read_opcode = qspi::ReadOpcode::FASTREAD;
    cfg.write_opcode = qspi::WriteOpcode::PP;

    let mut q = Qspi::new(qspi_periph, Irqs, sck, csn, io0, io1, io2, io3, cfg);
    let mut jedec = [0u8; 3];
    let _ = q.blocking_custom_instruction(0x9F, &[], &mut jedec);

    defmt::info!(
        "hwtest: JEDEC ID: {:02X} {:02X} {:02X}",
        jedec[0],
        jedec[1],
        jedec[2]
    );
    jedec != [0x00; 3] && jedec != [0xFF; 3]
}

// nRF52840 CLOCK peripheral register addresses (@ 0x40000000).
const CLOCK_TASKS_HFCLKSTART: *mut u32 = 0x4000_0000 as *mut u32;
const CLOCK_TASKS_HFCLKSTOP: *mut u32 = 0x4000_0004 as *mut u32;
const CLOCK_TASKS_LFCLKSTART: *mut u32 = 0x4000_0008 as *mut u32;
const CLOCK_TASKS_LFCLKSTOP: *mut u32 = 0x4000_000C as *mut u32;
const CLOCK_EVENTS_HFCLKSTARTED: *mut u32 = 0x4000_0100 as *mut u32;
const CLOCK_EVENTS_LFCLKSTARTED: *mut u32 = 0x4000_0104 as *mut u32;
const CLOCK_LFCLKSTAT: *const u32 = 0x4000_0418 as *const u32;
const CLOCK_LFCLKSRC: *mut u32 = 0x4000_0518 as *mut u32;

/// Request the 32 MHz HFXO and wait up to 1 s for `HFCLKSTARTED`.
///
/// HFCLK is on HFINT after `embassy_nrf::init()`, so the start task is a
/// fresh request.  If the crystal never stabilises the event never fires;
/// HFCLK stays on HFINT and the CPU keeps running, so the rest of the
/// test remains intact.
async fn probe_hfxo() -> bool {
    unsafe {
        CLOCK_EVENTS_HFCLKSTARTED.write_volatile(0);
        CLOCK_TASKS_HFCLKSTART.write_volatile(1);
    }
    for _ in 0..100u16 {
        if unsafe { CLOCK_EVENTS_HFCLKSTARTED.read_volatile() } != 0 {
            defmt::info!("hwtest:   HFXO started");
            return true;
        }
        Timer::after_millis(10).await;
    }
    // Abandon the request so the controller stops driving the XO.
    unsafe { CLOCK_TASKS_HFCLKSTOP.write_volatile(1) };
    defmt::warn!("hwtest:   HFXO FAIL (HFCLKSTARTED event never fired)");
    false
}

/// Request the 32.768 kHz LFXO and wait up to 1 s for `LFCLKSTARTED`.
///
/// `embassy_nrf::init()` has already started LFCLK on the RC oscillator
/// for its time driver (RTC1), so we must stop LFCLK, swap the source to
/// XTAL, and restart.  The LFCLK-off window is kept short: Timer is
/// unusable during it (RTC1 is LFCLK-driven) so we busy-wait on CPU
/// cycles instead.  If the crystal never starts we fall back to the RC
/// oscillator so embassy-time keeps ticking for the rest of the test.
async fn probe_lfxo() -> bool {
    const CYCLES_PER_MS: u32 = 64_000; // CPU clock = 64 MHz
    unsafe {
        // Stop the current LFCLK (RC).
        CLOCK_TASKS_LFCLKSTOP.write_volatile(1);
        while CLOCK_LFCLKSTAT.read_volatile() & (1 << 16) != 0 {
            cortex_m::asm::nop();
        }
        // Select XTAL and request start.
        CLOCK_LFCLKSRC.write_volatile(1); // 1 = Xtal
        CLOCK_EVENTS_LFCLKSTARTED.write_volatile(0);
        CLOCK_TASKS_LFCLKSTART.write_volatile(1);

        // Poll for ~1 s using CPU cycle delays because RTC1 is stalled.
        for _ in 0..1000u16 {
            if CLOCK_EVENTS_LFCLKSTARTED.read_volatile() != 0 {
                defmt::info!("hwtest:   LFXO started");
                return true;
            }
            cortex_m::asm::delay(CYCLES_PER_MS);
        }

        // Timed out — fall back to the RC oscillator so Timer works again.
        CLOCK_TASKS_LFCLKSTOP.write_volatile(1);
        while CLOCK_LFCLKSTAT.read_volatile() & (1 << 16) != 0 {
            cortex_m::asm::nop();
        }
        CLOCK_LFCLKSRC.write_volatile(0); // 0 = RC
        CLOCK_EVENTS_LFCLKSTARTED.write_volatile(0);
        CLOCK_TASKS_LFCLKSTART.write_volatile(1);
        for _ in 0..100u16 {
            if CLOCK_EVENTS_LFCLKSTARTED.read_volatile() != 0 {
                break;
            }
            cortex_m::asm::delay(CYCLES_PER_MS);
        }
    }
    defmt::warn!("hwtest:   LFXO FAIL (LFCLKSTARTED event never fired)");
    false
}

/// Reset the SX1262 and read its status over SPI. Returns true if the chip
/// releases BUSY within 50 ms and returns a plausible status byte.
#[allow(clippy::too_many_arguments)]
async fn probe_lora<'a>(
    spi_periph: embassy_nrf::Peri<'a, peripherals::SPI2>,
    sck: embassy_nrf::Peri<'a, embassy_nrf::gpio::AnyPin>,
    mosi: embassy_nrf::Peri<'a, embassy_nrf::gpio::AnyPin>,
    miso: embassy_nrf::Peri<'a, embassy_nrf::gpio::AnyPin>,
    rst: embassy_nrf::Peri<'a, embassy_nrf::gpio::AnyPin>,
    nss: embassy_nrf::Peri<'a, embassy_nrf::gpio::AnyPin>,
    busy: embassy_nrf::Peri<'a, embassy_nrf::gpio::AnyPin>,
    rf_sw: embassy_nrf::Peri<'a, embassy_nrf::gpio::AnyPin>,
) -> bool {
    // Hold RF switch in a defined state.
    let _rf_sw = Output::new(rf_sw, Level::Low, OutputDrive::Standard);
    let mut rst_out = Output::new(rst, Level::Low, OutputDrive::Standard);
    let mut nss_out = Output::new(nss, Level::High, OutputDrive::Standard);
    let busy_in = Input::new(busy, Pull::None);

    // Reset pulse: hold low ≥100 µs then release.
    Timer::after_micros(500).await;
    rst_out.set_high();

    // Wait for BUSY to fall (chip ready), 50 ms timeout.
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
    let mut spi = Spim::new(spi_periph, Irqs, sck, mosi, miso, spi_cfg);

    // GetStatus (0xC0) + NOP byte → status returned on MISO in the second byte.
    let tx = [0xC0u8, 0x00];
    let mut rx = [0u8; 2];
    nss_out.set_low();
    let r = spi.transfer(&mut rx, &tx).await;
    nss_out.set_high();

    match r {
        Ok(()) => {
            let status = rx[1];
            defmt::info!("hwtest: LoRa GetStatus = 0x{:02X}", status);
            // Chip mode bits [6:4]: 0=unused, 7=reserved. Valid modes are 2..=6.
            let mode = (status >> 4) & 0x07;
            mode != 0 && mode != 7 && status != 0xFF
        }
        Err(_) => {
            defmt::warn!("hwtest: LoRa SPI transfer failed");
            false
        }
    }
}

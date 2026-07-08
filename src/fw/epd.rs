//! EPD display driver wiring (SSD1675 / SSD1675B over SPI3).
//!
//! ## LUT cycle-duration tuning
//!
//! [`EPD_LUT_SPEED`] scales every non-zero byte in the OTP LUT timing
//! region before each refresh: `100` = OEM duration (per-variant default
//! in `vendor/ssd1675`), `0` = no delay, values >100 stretch linearly.
//! Persisted in the `"settings"` KV namespace under `"epd_lut"`.

use core::convert::Infallible;
use core::sync::atomic::{AtomicI8, AtomicI16, AtomicU8, Ordering};

#[cfg(feature = "embassy-base")]
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
#[cfg(feature = "embassy-base")]
use embassy_sync::signal::Signal;

use defmt_rtt as _;
use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pin as GpioPin, Port, Pull};
use embassy_nrf::spim::{Config, Frequency, InterruptHandler, Spim};
use embassy_nrf::{Peri, bind_interrupts, peripherals};
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use panic_probe as _;
use ssd1675::{
    Builder, Dimensions, Display, GraphicDisplay, Interface, LUT_TABLE_MIN_C, LUT_TABLE_SIZE,
    LUT_TABLE_STEP_C10, Rotation, detect_variant_from_otp, patch_no_invert,
};
use static_cell::StaticCell;

// EPD display configuration - compile-time constants with generics
pub struct EpdConfig<const ROWS: u16, const COLS: u8>;

impl<const ROWS: u16, const COLS: u8> EpdConfig<ROWS, COLS> {
    /// Buffer size in bytes (rows * cols / 8)
    pub const BUF_SIZE: usize = ROWS as usize * COLS as usize / 8;

    /// Get Dimensions for ssd1675 driver
    pub const fn to_dimensions() -> Dimensions {
        Dimensions {
            rows: ROWS,
            cols: COLS,
        }
    }
}

// Type aliases for common display sizes
pub type EpdConfig152x152 = EpdConfig<152, 152>;

bind_interrupts!(struct Irqs {
    SPIM3 => InterruptHandler<peripherals::SPI3>;
});

pub type EpdGfx<'a> = GraphicDisplay<
    'a,
    Interface<
        ExclusiveDevice<Spim<'a>, Output<'a>, embassy_time::Delay>,
        Input<'a>,
        Output<'a>,
        Output<'a>,
    >,
    &'a mut [u8],
>;

/// Boot-probed per-temperature LUT table — full OTP waveform with inversion
/// phases.  16 × 107 = 1.7 KB.  Used by `update_tc` for tri-color full
/// refreshes where the inversion phases reset ghosting.
static LUT_TABLE_CELL: StaticCell<[[u8; 107]; LUT_TABLE_SIZE]> = StaticCell::new();
/// Same as `LUT_TABLE_CELL` but with inversion phases zeroed per
/// `patch_no_invert`.  Used by `update_bw` for flicker-free fast refreshes.
static LUT_TABLE_NO_INVERT_CELL: StaticCell<[[u8; 107]; LUT_TABLE_SIZE]> = StaticCell::new();

fn pin_nr(p: &Peri<'_, AnyPin>) -> u8 {
    let port = match p.port() {
        Port::Port0 => 0u8,
        Port::Port1 => 1u8,
    };
    port * 32 + p.pin()
}

/// Read back the OTP LUT register (command 0x33) using stolen peripherals.
///
/// Sequence (per SSD1619 reference driver):
///   1. Hardware reset + 100 ms settle
///   2. Select the on-chip internal temperature sensor (0x18 = 0x80) — the
///      SSD1675 will use its own die measurement when the next LoadTemp step
///      runs.  The SoC's idea of temperature is *not* written: the panel's
///      internal sensor is more representative of the panel itself than the
///      nRF52840's die.
///   3. Send 0x22 / 0xB1 — EnableClock | LoadTemp | LoadLUT-Mode1 |
///      DisableClock
///   4. Send 0x20 — Master Activation (BUSY goes HIGH while controller loads
///      OTP zone)
///   5. Wait for BUSY LOW (controller has loaded the temperature zone into the
///      LUT register)
///   6. Send 0x33 command then read 107 bytes — the loaded LUT zone
///
/// All stolen resources are dropped before returning.
async fn probe_lut(
    sck: &Peri<'_, AnyPin>,
    data: &Peri<'_, AnyPin>,
    cs: &Peri<'_, AnyPin>,
    dc: &Peri<'_, AnyPin>,
    rst: &Peri<'_, AnyPin>,
    busy: &Peri<'_, AnyPin>,
    temp_raw: u16,
) -> [u8; 107] {
    let sck_nr = pin_nr(sck);
    let data_nr = pin_nr(data);
    let cs_nr = pin_nr(cs);
    let dc_nr = pin_nr(dc);
    let rst_nr = pin_nr(rst);
    let busy_nr = pin_nr(busy);

    // GPIO wrappers are mem::forget'd at the end to preserve pin config.
    let mut cs_out = Output::new(
        unsafe { AnyPin::steal(cs_nr) },
        Level::High,
        OutputDrive::Standard,
    );
    let mut dc_out = Output::new(
        unsafe { AnyPin::steal(dc_nr) },
        Level::Low,
        OutputDrive::Standard,
    );
    let mut rst_out = Output::new(
        unsafe { AnyPin::steal(rst_nr) },
        Level::Low,
        OutputDrive::Standard,
    );
    let busy_in = Input::new(unsafe { AnyPin::steal(busy_nr) }, Pull::Down);

    let mut cfg = Config::default();
    // SSD1675 datasheet rates SCK up to ~20 MHz; SPIM3 caps at 32 MHz.
    // 16 MHz is comfortably below both and 16× the previous M1 setting.
    cfg.frequency = Frequency::M16;

    // Hardware reset — flat 100 ms settle (BUSY does not reliably pulse during
    // reset/OTP boot).
    Timer::after_millis(10).await;
    rst_out.set_high();
    Timer::after_millis(100).await;

    // Phase 0: SoftReset + analog/digital block setup.  Matches the
    // badge.team SSD168x init pattern (HW reset → 0x12 → 0x74 → 0x7E
    // → ...).  Without these, OTP zone reload doesn't execute and
    // every band-LUT readback comes out byte-identical.
    cs_out.set_low();
    {
        let mut spi_tx = Spim::new_txonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg.clone(),
        );
        // 0x12 = SoftReset.  Puts chip in known state; BUSY pulses
        // high then low while internal logic clears.
        dc_out.set_low();
        spi_tx.write(&[0x12]).await.ok();
        dc_out.set_high();
        core::mem::forget(spi_tx);
    }
    cs_out.set_high();
    for _ in 0..100u8 {
        if !busy_in.is_high() {
            break;
        }
        Timer::after_millis(10).await;
    }

    cs_out.set_low();
    {
        let mut spi_tx = Spim::new_txonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg.clone(),
        );
        // 0x74 = AnalogBlockControl (value 0x54 per datasheet).
        dc_out.set_low();
        spi_tx.write(&[0x74]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0x54]).await.ok();
        // 0x7E = DigitalBlockControl (value 0x3B per datasheet).
        dc_out.set_low();
        spi_tx.write(&[0x7E]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0x3B]).await.ok();
        core::mem::forget(spi_tx);
    }
    cs_out.set_high();

    // Phase 1: write temperature and trigger OTP LUT zone load.
    cs_out.set_low();
    {
        let mut spi_tx = Spim::new_txonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg.clone(),
        );
        // 0x18 = 0x80: select the *internal* temperature sensor (B-variant
        // documented; A-variant accepts as no-op per the gap on pg 23).
        // NOTE: this only matters if a LoadTemp step runs.  The probe
        // deliberately does NOT LoadTemp (see 0x22/0x91 below) precisely so
        // the chip keeps the value we write via 0x1A instead of re-sampling
        // the sensor — sampling would overwrite our manual band value and
        // make every band-LUT identical.
        dc_out.set_low();
        spi_tx.write(&[0x18]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0x80]).await.ok();
        // 0x1A: write current MCU die temperature into the chip's
        // temperature register, 12-bit signed (pg 23 + pg 18 §6.10).
        // Critical for the §6.9 TR-search: the upcoming LoadTemp+LoadLut
        // sequence walks TR0..TR24 against THIS value and loads the
        // matching WS into the LUT register — which we then read back
        // via 0x33 and cache.  Without this write the register sits at
        // POR (`0x7FF` = 127.9 °C) and we'd cache the warmest-WS for
        // the entire session, regardless of actual ambient.
        // SSD1675 has no on-die sensor (pg 6 block diagram), and the
        // badge has no external sensor wired, so the MCU die value (rough
        // proxy, warmer than panel under load) is the best we have.
        let byte1 = ((temp_raw >> 4) & 0xFF) as u8;
        let byte2 = ((temp_raw & 0x0F) << 4) as u8;
        dc_out.set_low();
        spi_tx.write(&[0x1A]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[byte1, byte2]).await.ok();
        // 0x22 / 0x91: EnableClock | LoadLUT-OTP-Mode1 | DisableClock.
        // NO LoadTemp bit (that would be 0xB1) — LoadTemp re-samples the
        // sensor selected by 0x18 and clobbers the manual 0x1A value, so the
        // TR-search lands in the same band every iteration and all 16
        // band-LUTs come back identical.  0x91 keeps our written temperature
        // so the per-band TR-search loads a distinct WS each pass.
        dc_out.set_low();
        spi_tx.write(&[0x22]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0x91]).await.ok();
        // 0x20: Master Activation — BUSY goes HIGH while the controller loads the OTP
        // zone.
        dc_out.set_low();
        spi_tx.write(&[0x20]).await.ok();
        // Don't drop — Spim::drop disconnects SPI pins.
        core::mem::forget(spi_tx);
    }
    cs_out.set_high();

    // Wait for BUSY LOW: controller has finished loading the temperature zone into
    // the LUT register. Poll every 10 ms, up to 1 s total.
    for _ in 0..100u8 {
        if !busy_in.is_high() {
            break;
        }
        Timer::after_millis(10).await;
    }

    // Phase 2: read 107 bytes from the LUT register (0x33).
    // The controller now presents the loaded zone on MISO.
    // Stack-allocated only for the duration of the SPI read; caller moves it into
    // StaticCell.
    let mut lut = [0u8; 107];
    cs_out.set_low();
    {
        // Command phase: send 0x33 on MOSI.
        let mut spi_tx = Spim::new_txonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg.clone(),
        );
        dc_out.set_low();
        spi_tx.write(&[0x33]).await.ok();
        dc_out.set_high();
        core::mem::forget(spi_tx);
    }
    {
        // Data phase: read 107 bytes on MISO (same physical pin, now input).
        let mut spi_rx = Spim::new_rxonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg.clone(),
        );
        spi_rx.read(&mut lut).await.ok();
        // Drop the RX Spim — it will disable SPI3, but we restore TX mode below.
        drop(spi_rx);
    }
    cs_out.set_high();

    // Restore SPI3 to TX-only mode (data pin as MOSI) so the display's
    // Spim can transmit. The display's Spim doesn't reconfigure pin
    // selection on each write — it was set once at boot.
    {
        let restore = Spim::new_txonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg,
        );
        core::mem::forget(restore);
    }

    defmt::debug!("Display OTP LUT (107 bytes):");
    for (i, chunk) in lut.chunks(10).enumerate() {
        defmt::debug!("  [{=usize:03}] {:02x}", i * 10, chunk);
    }

    // Prevent Drop from disconnecting GPIO pins — the display's real
    // Output/Input instances still own these pins.
    core::mem::forget(cs_out);
    core::mem::forget(dc_out);
    core::mem::forget(rst_out);
    core::mem::forget(busy_in);

    lut
}

/// A `LUT.CFG`-set `speed=` override, or `-1` when unset. Read once at
/// boot by [`apply_lut_file_speed`] so a calibration bundle's speed wins
/// over the persisted-KV value.
pub static LUT_FILE_SPEED: AtomicI16 = AtomicI16::new(-1);

/// Read and apply a custom `LUT.CFG` from the USB-MSC FAT partition into
/// `lut_table`, in place.
///
/// `scratch` is a caller-owned byte buffer used to read the file — the EPD
/// `work_buffer` is reused for this so we add **no** new `.bss` (mesh
/// RAM/stack is marginal; extra statics overflow the boot stack and
/// HardFault). It also bounds the max file size to `scratch.len()`.
///
/// Fills only the bands the file specifies (a `band_lut` base and/or
/// `band_lut_NN` overrides); bands the file leaves out keep their
/// OTP-probed waveform, so a partial set still tracks temperature. A
/// `speed=` value is stashed in [`LUT_FILE_SPEED`] for later application.
///
/// The file's `variant` is validated against the live `panel`, and the
/// whole waveform is validated (`validate_bands`), *before* `lut_table` is
/// written — so a mismatched-variant or malformed file never half-applies.
/// Any failure is a no-op (OTP kept).
async fn load_custom_lut(
    lut_table: &mut [[u8; 107]; LUT_TABLE_SIZE],
    panel: ssd1675::DisplayVariant,
    scratch: &mut [u8],
) {
    use crate::lut_file::LutVariant;

    let Some(name) = crate::fw::fat12::to_8_3("LUT.CFG") else {
        return;
    };
    let Ok(file) = crate::fw::fat12::find_file(&name).await else {
        return; // no LUT.CFG — normal, keep OTP
    };
    let Ok(n) = crate::fw::fat12::read_file(&file, 0, scratch).await else {
        return;
    };
    let data = &scratch[..n];

    // Variant + speed, validated against the panel before anything else.
    let meta = match crate::lut_file::parse_meta(data) {
        Ok(m) => m,
        Err(_) => {
            defmt::warn!("LUT.CFG present but rejected (bad meta)");
            return;
        }
    };
    let file_is_b = meta.variant == LutVariant::B;
    let panel_is_b = matches!(panel, ssd1675::DisplayVariant::Ssd1675B);
    if file_is_b != panel_is_b {
        defmt::warn!(
            "LUT.CFG variant mismatch (file B={=bool}, panel B={=bool}) — ignoring to protect the panel",
            file_is_b,
            panel_is_b
        );
        return;
    }

    // Dry-run validate every waveform value BEFORE writing, so a malformed
    // file can't leave `lut_table` half-overwritten (no scratch table).
    if crate::lut_file::validate_bands(data).is_err() {
        defmt::warn!("LUT.CFG waveform rejected (bad hex / length / band) — keeping OTP");
        return;
    }

    let mut band_set = [false; LUT_TABLE_SIZE];
    match crate::lut_file::parse_bands(data, lut_table, &mut band_set) {
        Ok(true) => {
            let count = band_set.iter().filter(|&&s| s).count() as u8;
            defmt::info!("LUT.CFG accepted: {=u8} band(s) applied", count);
        }
        Ok(false) => defmt::info!("LUT.CFG accepted: no waveform (settings only)"),
        // Unreachable after validate_bands, but keep OTP if it ever fires.
        Err(_) => return,
    }

    if let Some(speed) = meta.speed {
        LUT_FILE_SPEED.store(speed as i16, Ordering::Relaxed);
    }
}

/// Apply a `LUT.CFG`-supplied `speed=` (if any) over the persisted value.
/// Call at boot *after* [`load_persisted_lut_speed`] so the file wins.
/// Clamps to `[EPD_LUT_SPEED_MIN, 255]`.
pub fn apply_lut_file_speed() {
    let s = LUT_FILE_SPEED.load(Ordering::Relaxed);
    if s >= 0 {
        let v = (s as u16).clamp(EPD_LUT_SPEED_MIN as u16, 255) as u8;
        EPD_LUT_SPEED.store(v, Ordering::Relaxed);
        defmt::info!("LUT.CFG speed override applied: {=u8}", v);
    }
}

/// Initialize the EPD display (SSD1675/SSD1675B, SPIM3 interface).
///
/// Boot-probes the chip's OTP at 16 temperatures (−10..+54 °C in 4 °C steps,
/// matching the deployed panels' OTP TR-band granularity) and caches the
/// resulting 16 × 107-byte WS images into [`LUT_TABLE_CELL`].  Driver later
/// indexes the table by `Display::active_temp_c10` and pushes the matching
/// LUT every refresh — bypasses the chip's own TR-search and the entire
/// temperature-register / `LoadTemp` dance.  See `Display::update_tc` /
/// `Display::update_bw`.
///
/// Probe takes ~16 × ~150 ms ≈ 2-3 s at boot.  Caller's read of the MCU die
/// temperature isn't required here — `probe_lut` writes a different
/// temperature register value on every iteration so the chip's TR-search
/// lands in a different band each time.
pub async fn init_epd<'a>(
    spi: Peri<'a, peripherals::SPI3>,
    sck_pin: Peri<'a, AnyPin>,
    mosi_pin: Peri<'a, AnyPin>,
    busy_pin: Peri<'a, AnyPin>,
    resetn_pin: Peri<'a, AnyPin>,
    dc_pin: Peri<'a, AnyPin>,
    csn_pin: Peri<'a, AnyPin>,
    dimension: Dimensions,
    black_buffer: &'a mut [u8],
    red_buffer: &'a mut [u8],
    work_buffer: &'a mut [u8],
    // When `true`, skip any custom `LUT.CFG` and use the OTP-probed
    // waveform — the boot-time escape hatch (user held Fire) for a bad
    // custom LUT that would otherwise blank the panel.
    force_otp_lut: bool,
) -> Result<EpdGfx<'a>, Infallible> {
    // Allocate the table in static storage first, then fill in-place — keeps
    // the 1.7 KB array off the stack.
    let lut_table: &'static mut [[u8; 107]; LUT_TABLE_SIZE] =
        LUT_TABLE_CELL.init([[0u8; 107]; LUT_TABLE_SIZE]);

    for i in 0..LUT_TABLE_SIZE {
        let temp_c10 = (LUT_TABLE_MIN_C as i32 * 10)
            + (i as i32) * (LUT_TABLE_STEP_C10 as i32);
        let temp_raw = temp_c10_to_ssd1675(temp_c10 as i16);
        lut_table[i] = probe_lut(
            &sck_pin,
            &mosi_pin,
            &csn_pin,
            &dc_pin,
            &resetn_pin,
            &busy_pin,
            temp_raw,
        )
        .await;
        defmt::debug!(
            "LUT[{=usize:02}] @ {=i32} m°C: probed",
            i,
            temp_c10
        );
    }

    // Sanity-check OTP probe: if every band-LUT is byte-identical to
    // band 0, the probe stalled (temperature write didn't change the
    // OTP zone, BUSY race, etc.) and the per-temperature lookup is
    // effectively single-LUT.  Panel will still drive but contrast /
    // ghosting won't track temperature.
    let all_identical = (1..LUT_TABLE_SIZE).all(|i| lut_table[i] == lut_table[0]);
    if all_identical {
        // A transient probe stall (BUSY race, OTP-load, temperature write not
        // landing) must not brick the badge — the panel still drives off the
        // single probed band, it just won't track temperature.  Warn and boot
        // with the degraded (single-LUT) table instead of panicking.
        defmt::warn!(
            "EPD OTP probe: all {} bands identical — booting with single-LUT (no temp tracking). lut[0..10] = {=[u8]:#04x}",
            LUT_TABLE_SIZE,
            lut_table[0][..10],
        );
    }

    // Detect the panel variant from a probed entry — needed both for the
    // custom-LUT validation below and for `patch_no_invert` later.
    let variant = detect_variant_from_otp(&lut_table[LUT_TABLE_SIZE / 2]);

    // Optionally replace OTP-probed bands with a user-supplied custom LUT
    // (LUT.CFG on the USB-MSC FAT partition), applied here BEFORE the
    // framebuffers are handed to the driver so `work_buffer` can double as
    // the file-read scratch — adding no new `.bss` (mesh RAM/stack is
    // marginal; extra statics HardFault at boot). Variant is validated
    // against the panel inside `load_custom_lut`; it fills only the bands
    // the file specifies. Skipped when the user holds Fire at boot — the
    // escape hatch for a LUT that renders badly.
    if !force_otp_lut {
        load_custom_lut(lut_table, variant, work_buffer).await;
    } else {
        defmt::info!("EPD: Fire held at boot — forcing OTP LUT, ignoring LUT.CFG");
    }

    // Build the SPI bus.
    let mut cfg = Config::default();
    // Same as the OTP-load path above: M16 for runtime EPD writes.
    // Refresh time is waveform-bound (LUT timings, not SCK), but a
    // faster bus frees the executor sooner during the ~80 KiB framebuffer
    // push so concurrent tasks (LoRa, USB) get more cycles.
    cfg.frequency = Frequency::M16;
    let bus = Spim::new_txonly(spi, Irqs, sck_pin, mosi_pin, cfg);

    // Initialize GPIO pins.
    let csn_out = Output::new(csn_pin, Level::High, OutputDrive::Standard);
    let resetn_out = Output::new(resetn_pin, Level::Low, OutputDrive::Standard);
    let dc_out = Output::new(dc_pin, Level::Low, OutputDrive::Standard);
    let busy_in = Input::new(busy_pin, Pull::Down);

    let spi_dev = ExclusiveDevice::new(bus, csn_out, embassy_time::Delay).unwrap();

    let controller = ssd1675::Interface::new(spi_dev, busy_in, dc_out, resetn_out);
    let config = Builder::new()
        .dimensions(dimension)
        .rotation(Rotation::Rotate0)
        .build()
        .unwrap();
    let display = Display::new(controller, config);
    let mut gfx = GraphicDisplay::new(display, black_buffer, red_buffer, work_buffer);
    // `variant` was detected (and any custom LUT applied) above, before the
    // framebuffers were moved into `gfx`.
    gfx.set_variant(variant);

    // Derive the no-invert table from the full one + register both.
    let lut_table_no_invert: &'static mut [[u8; 107]; LUT_TABLE_SIZE] =
        LUT_TABLE_NO_INVERT_CELL.init([[0u8; 107]; LUT_TABLE_SIZE]);
    for i in 0..LUT_TABLE_SIZE {
        lut_table_no_invert[i] = lut_table[i];
        patch_no_invert(&mut lut_table_no_invert[i], variant);
    }
    gfx.register_lut_tables(lut_table, lut_table_no_invert);
    defmt::info!(
        "Display controller: {}",
        match gfx.variant() {
            ssd1675::display::DisplayVariant::Ssd1675B => "SSD1675B (10-byte row LUT)",
            ssd1675::display::DisplayVariant::Ssd1675 => "SSD1675 (7-byte row LUT)",
        }
    );

    // DIAG (temperature-compensation analysis): dump every probed band's
    // full 107-byte OTP LUT + its frame count.  Lets us see exactly how the
    // OTP encodes temperature across bands — waveform bytes, TP timing
    // region, and the voltage trailer (VSH1/VSH2/VSL/VCOM).  band i = (-10 +
    // 4*i) °C.  One-shot at boot; capture with `probe-rs run` from reset.
    for i in 0..LUT_TABLE_SIZE {
        let frames = ssd1675::waveform_frames(&lut_table[i], variant);
        defmt::info!(
            "LUT band {=usize:02} ({=i32} C) {=u32} frames: {=[u8]:#04x}",
            i,
            -10 + 4 * i as i32,
            frames,
            lut_table[i][..],
        );
        // Throttle: each line is a ~107-byte defmt frame; bursting all 16 in
        // ~2 ms overruns the RTT buffer and the host drops the head (bands
        // 0..8). 80 ms/line lets RTT drain so every band is captured.
        Timer::after_millis(80).await;
    }

    Ok(gfx)
}

// ---------------------------------------------------------------------------
// Partial-mode state (lazy-allocated, single instance)
// ---------------------------------------------------------------------------

use core::sync::atomic::AtomicBool;
use ssd1675::partial::PartialState;

/// Single-shot guard for [`partial_state_take`] — second call panics
/// (`PartialState::take` itself panics on the second `take()` of the
/// underlying `ConstStaticCell`s, but this gives a clearer message).
static PARTIAL_TAKEN: AtomicBool = AtomicBool::new(false);

/// Take ownership of the driver's host-side partial-refresh state.
/// Call once at boot — typically right after `init_epd` succeeds.
/// Sized for the panel's actual dimensions; buffers in `.bss`,
/// allocated by the driver crate's `ConstStaticCell`s.
///
/// Returns the `PartialState`; caller stores it (typically alongside
/// the `EpdGfx`) and passes by `&mut` to `display.update_partial(...)`.
pub fn partial_state_take(rows: u16, cols: u8) -> PartialState {
    let prev = PARTIAL_TAKEN.swap(true, Ordering::Relaxed);
    if prev {
        defmt::panic!("partial_state_take called twice");
    }
    PartialState::take(rows, cols as u16)
}

// ---------------------------------------------------------------------------
// LUT cycle-duration scale: runtime atomic + persister glue
// ---------------------------------------------------------------------------

/// Lower bound on the LUT cycle-duration scale exposed to the user.
///
/// Anything below this risks producing a display so washed-out / blank
/// that the user cannot read the menu to dial it back up — a soft
/// lock-out.  Enforced by the menu inc/dec, the boot loader, and the
/// persister so the floor sticks across reboots.
pub const EPD_LUT_SPEED_MIN: u8 = 30;

/// Effective LUT cycle-duration scale. Default `100` (OEM); menu inc/dec
/// writes here and fires [`EPD_LUT_SPEED_DIRTY`]. `load_persisted_lut_speed`
/// also writes here at boot (without firing the signal).
pub static EPD_LUT_SPEED: AtomicU8 = AtomicU8::new(100);

/// Fired when [`EPD_LUT_SPEED`] is updated from the menu — drives the
/// persister loop in [`epd_lut_speed_persist_loop`].
#[cfg(feature = "embassy-base")]
pub static EPD_LUT_SPEED_DIRTY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Load the persisted LUT-speed override (if any) into [`EPD_LUT_SPEED`].
/// Call once at boot, after [`init_epd`]. Falls back to `100` when no
/// override has been stored.
///
/// Clamps to `[EPD_LUT_SPEED_MIN, 255]` — defends against a stale KV
/// value (e.g. from a build that allowed lower values) locking the user
/// out of an unreadable display.
///
/// Gated on `mesh`, not `embassy-base`: the settings KV store lives under
/// `fw::mesh::settings`, so persistence only exists in mesh builds. In
/// non-mesh configs (embassy-game / embassy-watch) the menu still tunes the
/// live atomic; the value just isn't saved across reboots.
#[cfg(feature = "mesh")]
pub async fn load_persisted_lut_speed() {
    let scale = crate::fw::mesh::settings::get_epd_lut_speed()
        .await
        .unwrap_or(100)
        .max(EPD_LUT_SPEED_MIN);
    EPD_LUT_SPEED.store(scale, Ordering::Relaxed);
}

/// Self-heating bias for the SSD1675 temperature feed, °C × 10 — subtracted
/// from the MCU die reading to estimate the panel temperature.
///
/// Now 0 for both variants: the driver's SSD1675A voltage gradient
/// (`DisplayVariant::voltages`) and the OTP waveform band-selection both need
/// the *real* panel temperature.  The old −25 °C bias on A drove the lookup
/// far too cold (e.g. 34 °C die → 9 °C lookup), which over-drove the panel and
/// picked the slow cold-band waveform.  Re-introduce a small offset here only
/// if the die is measured to run materially hotter than the panel.
fn self_heating_bias_c10(_variant: ssd1675::DisplayVariant) -> i16 {
    0
}

/// User-tunable extra bias on top of [`self_heating_bias_c10`], in
/// °C × 10.  Default 0, range `[EPD_TEMP_BIAS_MIN, EPD_TEMP_BIAS_MAX]`
/// (= ±5 °C in 0.5 °C steps).  Lets the user nudge the LUT-table
/// lookup warmer (positive) or cooler (negative) to compensate for
/// per-panel waveform tuning differences.
///
/// Persisted in the `"settings"` KV namespace under `"epd_tb"`.
pub const EPD_TEMP_BIAS_MIN: i8 = -50;
pub const EPD_TEMP_BIAS_MAX: i8 = 50;
pub const EPD_TEMP_BIAS_STEP: i8 = 5;

pub static EPD_TEMP_BIAS_C10: AtomicI8 = AtomicI8::new(0);

#[cfg(feature = "embassy-base")]
pub static EPD_TEMP_BIAS_DIRTY: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// Gated on `mesh` (settings KV store) — see `load_persisted_lut_speed`.
#[cfg(feature = "mesh")]
pub async fn load_persisted_temp_bias() {
    let v = crate::fw::mesh::settings::get_epd_temp_bias_c10()
        .await
        .unwrap_or(0)
        .clamp(EPD_TEMP_BIAS_MIN, EPD_TEMP_BIAS_MAX);
    EPD_TEMP_BIAS_C10.store(v, Ordering::Relaxed);
}

// Spawned only by the mesh settings persister; needs `fw::mesh::settings`.
#[cfg(feature = "mesh")]
pub async fn epd_temp_bias_persist_loop() -> ! {
    loop {
        EPD_TEMP_BIAS_DIRTY.wait().await;
        let v = EPD_TEMP_BIAS_C10
            .load(Ordering::Relaxed)
            .clamp(EPD_TEMP_BIAS_MIN, EPD_TEMP_BIAS_MAX);
        match crate::fw::mesh::settings::set_epd_temp_bias_c10(v).await {
            Ok(()) => defmt::debug!("settings: epd_temp_bias_c10={} persisted", v),
            Err(e) => defmt::warn!("settings: epd_temp_bias_c10 persist failed: {:?}", e),
        }
    }
}

/// PCB temperature estimate (°C × 10) for SSD1675 LUT-table indexing.
/// Returns `last_c10() - self_heating_bias_c10(variant) - user_bias`,
/// or `i16::MIN` if no MCU die reading has been taken yet.
pub fn panel_temp_c10(variant: ssd1675::DisplayVariant) -> i16 {
    let c10 = crate::fw::temperature::last_c10();
    if c10 == i16::MIN {
        i16::MIN
    } else {
        let user = EPD_TEMP_BIAS_C10.load(Ordering::Relaxed) as i16;
        c10 - self_heating_bias_c10(variant) - user
    }
}

/// Convert nRF52840 die temperature (°C × 10) into the SSD1675 12-bit
/// temperature-register format (1 LSB = 1/16 °C, two's complement, 12 bits
/// per datasheet §6.10 pg 18).
///
/// Example: 25.0 °C → c10=250 → raw = 250 × 16 / 10 = 400 = `0x190`
/// (matches datasheet pg 18 table).  Negative values use 12-bit
/// two's complement.
fn temp_c10_to_ssd1675(c10: i16) -> u16 {
    let raw = (c10 as i32 * 16) / 10;
    let clamped = raw.clamp(-2048, 2047);
    (clamped as u16) & 0x0FFF
}

pub fn current_lut_speed() -> u8 {
    EPD_LUT_SPEED.load(Ordering::Relaxed)
}

/// Persister loop: waits on [`EPD_LUT_SPEED_DIRTY`], writes the current
/// [`EPD_LUT_SPEED`] value to the `"settings"` KV namespace.  Spawned by
/// [`crate::fw::mesh::persister::run`] alongside the other settings loops.
// Gated on `mesh` (settings KV store) — see `load_persisted_lut_speed`.
#[cfg(feature = "mesh")]
pub async fn epd_lut_speed_persist_loop() -> ! {
    loop {
        EPD_LUT_SPEED_DIRTY.wait().await;
        // Clamp to the lock-out floor before persisting so a future menu
        // bug can't write an unrecoverable value.
        let scale = EPD_LUT_SPEED.load(Ordering::Relaxed).max(EPD_LUT_SPEED_MIN);
        match crate::fw::mesh::settings::set_epd_lut_speed(scale).await {
            Ok(()) => defmt::debug!("settings: epd_lut_speed={} persisted", scale),
            Err(e) => defmt::warn!("settings: epd_lut_speed persist failed: {:?}", e),
        }
    }
}

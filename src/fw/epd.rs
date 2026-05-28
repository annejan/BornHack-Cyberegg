//! EPD display driver wiring (SSD1675 / SSD1675B over SPI3).
//!
//! ## LUT cycle-duration tuning
//!
//! [`EPD_LUT_SPEED`] scales every non-zero byte in the OTP LUT timing
//! region before each refresh: `100` = OEM duration (per-variant default
//! in `vendor/ssd1675`), `0` = no delay, values >100 stretch linearly.
//! Persisted in the `"settings"` KV namespace under `"epd_lut"`.

use core::convert::Infallible;
use core::sync::atomic::{AtomicI8, AtomicU8, Ordering};

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
    cfg.frequency = Frequency::M1;

    // Hardware reset — flat 100 ms settle (BUSY does not reliably pulse during
    // reset/OTP boot).
    Timer::after_millis(10).await;
    rst_out.set_high();
    Timer::after_millis(100).await;

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
        // 0x18 = 0x80: temperature sensor source select (B-variant
        // documented; A-variant accepts as no-op per the gap on pg 23).
        // Tells the chip to use whatever's in the temperature register
        // verbatim — no I²C external-sensor poll on the LoadTemp step.
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
        // 0x22 / 0xB1: EnableClock | LoadTemp | LoadLUT-OTP-Mode1 | DisableClock
        dc_out.set_low();
        spi_tx.write(&[0x22]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0xB1]).await.ok();
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

    // Build the SPI bus.
    let mut cfg = Config::default();
    cfg.frequency = Frequency::M1;
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
    // Detect variant from a probed entry — needed before `register_lut_tables`
    // because `patch_no_invert` is variant-aware.
    let variant = detect_variant_from_otp(&lut_table[LUT_TABLE_SIZE / 2]);
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
#[cfg(feature = "embassy-base")]
pub async fn load_persisted_lut_speed() {
    let scale = crate::fw::mesh::settings::get_epd_lut_speed()
        .await
        .unwrap_or(100)
        .max(EPD_LUT_SPEED_MIN);
    EPD_LUT_SPEED.store(scale, Ordering::Relaxed);
}

/// Variant-aware self-heating bias for the SSD1675 LUT-table lookup,
/// °C × 10.  The SSD1675**A** variant runs hot waveforms aggressively and
/// blooms badly with the band-centre WS at face-value MCU die temp — bias
/// 15 °C warmer than measured so the lookup picks a milder WS.  The
/// SSD1675**B** variant doesn't show the same bloom and uses the raw
/// reading.
fn self_heating_bias_c10(variant: ssd1675::DisplayVariant) -> i16 {
    match variant {
        ssd1675::DisplayVariant::Ssd1675 => 250,
        ssd1675::DisplayVariant::Ssd1675B => 0,
    }
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

#[cfg(feature = "embassy-base")]
pub async fn load_persisted_temp_bias() {
    let v = crate::fw::mesh::settings::get_epd_temp_bias_c10()
        .await
        .unwrap_or(0)
        .clamp(EPD_TEMP_BIAS_MIN, EPD_TEMP_BIAS_MAX);
    EPD_TEMP_BIAS_C10.store(v, Ordering::Relaxed);
}

#[cfg(feature = "embassy-base")]
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
#[cfg(feature = "embassy-base")]
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

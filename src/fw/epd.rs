//! EPD display driver wiring (SSD1675 / SSD1675B over SPI3).
//!
//! ## LUT cycle-duration tuning
//!
//! [`EPD_LUT_SPEED`] scales every non-zero byte in the OTP LUT timing
//! region before each refresh: `100` = OEM duration (per-variant default
//! in `vendor/ssd1675`), `0` = no delay, values >100 stretch linearly.
//! Persisted in the `"settings"` KV namespace under `"epd_lut"`.

use core::convert::Infallible;
use core::sync::atomic::{AtomicPtr, AtomicU8, Ordering};

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
pub use ssd1675::LutMode;
use ssd1675::{Builder, Dimensions, Display, GraphicDisplay, Interface, Rotation};
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

static OTP_LUT: StaticCell<[u8; 107]> = StaticCell::new();
/// Un-patched copy of the probed OTP LUT, registered with the driver via
/// `set_full_lut` so `update_bw(Mode2)` can run the real full waveform
/// (with inversion / erase phases intact) instead of the patched fast LUT.
static OTP_LUT_FULL: StaticCell<[u8; 107]> = StaticCell::new();
static OTP_LUT_PTR: AtomicPtr<[u8; 107]> = AtomicPtr::new(core::ptr::null_mut());

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
        // 0x18: Temperature Sensor Selection = Internal.  The SSD1675's
        // own die sensor will be sampled by the LoadTemp step below;
        // we deliberately do NOT write a manual value via 0x1A — that
        // would override the on-chip sensor with the (unrelated) MCU
        // die temperature.
        dc_out.set_low();
        spi_tx.write(&[0x18]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0x80]).await.ok();
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
/// Reads the factory OTP LUT from the display controller before consuming
/// the peripheral tokens.  The SSD1675's on-chip temperature sensor is
/// selected and used for waveform compensation — no external temperature
/// is supplied or required.
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
    lut_mode: LutMode,
) -> Result<EpdGfx<'a>, Infallible> {
    // Read OTP LUT via stolen peripherals before consuming the real tokens.
    // Move raw bytes into static storage immediately — keeps the 107-byte buffer
    // off the stack.  Two copies: `otp_lut` will be patched in-place by
    // `set_otp_lut` (per `lut_mode`) for fast Mode 1 refreshes; `otp_full`
    // stays raw for Mode 2 full refreshes.
    let raw = probe_lut(
        &sck_pin,
        &mosi_pin,
        &csn_pin,
        &dc_pin,
        &resetn_pin,
        &busy_pin,
    )
    .await;
    let otp_full: &'static [u8; 107] = OTP_LUT_FULL.init(raw);
    let otp_lut: &'static mut [u8; 107] = OTP_LUT.init(raw);
    OTP_LUT_PTR.store(otp_lut as *mut _, Ordering::Relaxed);

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
    // Register the un-patched OTP first; `set_otp_lut` may then mutate
    // its buffer in place when `lut_mode == NoInvert`.
    gfx.set_full_lut(otp_full);
    gfx.set_otp_lut(otp_lut, lut_mode);
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

/// Read the current effective LUT-speed scale. Pass this to
/// [`ssd1675::GraphicDisplay::update_bw`] / `update_tc`.
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

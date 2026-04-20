use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::peripherals;
use embassy_nrf::spim::{Config, Frequency, InterruptHandler, Spim};
use embassy_nrf::{Peri, bind_interrupts};
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::StaticCell;

use core::convert::Infallible;
use embassy_nrf::gpio::{Pin as GpioPin, Port};
use ssd1675::{Builder, Dimensions, Display, GraphicDisplay, Interface, Rotation};
pub use ssd1675::LutMode;
use {defmt_rtt as _, panic_probe as _};

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
static OTP_LUT_PTR: core::sync::atomic::AtomicPtr<[u8; 107]> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

// Pin numbers cached at init for OTP LUT reload.
static EPD_PIN_SCK:  core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
static EPD_PIN_DATA: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
static EPD_PIN_CS:   core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
static EPD_PIN_DC:   core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
static EPD_PIN_RST:  core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
static EPD_PIN_BUSY: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

fn cache_pin_nr(p: &Peri<'_, AnyPin>) -> u8 {
    let port = match p.port() { Port::Port0 => 0u8, Port::Port1 => 1u8 };
    port * 32 + p.pin()
}

/// Temperature (°C × 10) at which the OTP LUT was last loaded.
static LUT_TEMP_C10: core::sync::atomic::AtomicI16 =
    core::sync::atomic::AtomicI16::new(i16::MIN);

/// Check if the current temperature has drifted more than 2°C from the last
/// LUT probe. If so, re-read the OTP LUT from the display controller at the
/// new temperature, patch it (NoInvert), and update the display's cached LUT.
///
/// Must be called while the display is in deep sleep (SPI3 idle).
/// The `display` reference is needed to update its stored LUT and temperature.
pub async fn maybe_reload_lut<'a>(display: &mut EpdGfx<'a>) {
    // Read current temperature via MPSL (safe after init, synchronous).
    // MPSL is only available when the mesh feature (nrf-mpsl) is enabled.
    #[cfg(feature = "mesh")]
    let current_c10 = super::temperature::read_via_mpsl();
    #[cfg(not(feature = "mesh"))]
    let current_c10 = super::temperature::last_c10();
    let last = LUT_TEMP_C10.load(core::sync::atomic::Ordering::Relaxed);
    if last != i16::MIN {
        let delta = (current_c10 - last).unsigned_abs();
        if delta < 20 {
            return;
        }
    }

    let temp_celsius = current_c10 / 10;
    let temp_raw = (temp_celsius as i32 * 16) as u16;
    defmt::info!(
        "EPD: temperature drift ({} → {} °C×10), reloading OTP LUT (raw 0x{:04x})",
        last, current_c10, temp_raw,
    );

    // Re-read the OTP LUT at the new temperature using stolen peripherals.
    // Safe: display is in deep_sleep, SPI3 is idle.
    use core::sync::atomic::Ordering::Relaxed;
    let sck  = unsafe { AnyPin::steal(EPD_PIN_SCK.load(Relaxed)) };
    let data = unsafe { AnyPin::steal(EPD_PIN_DATA.load(Relaxed)) };
    let cs   = unsafe { AnyPin::steal(EPD_PIN_CS.load(Relaxed)) };
    let dc   = unsafe { AnyPin::steal(EPD_PIN_DC.load(Relaxed)) };
    let rst  = unsafe { AnyPin::steal(EPD_PIN_RST.load(Relaxed)) };
    let busy = unsafe { AnyPin::steal(EPD_PIN_BUSY.load(Relaxed)) };

    let new_lut = probe_lut(
        &sck.into(), &data.into(), &cs.into(),
        &dc.into(), &rst.into(), &busy.into(),
        temp_raw,
    ).await;

    // Overwrite the static LUT buffer in-place and re-patch.
    let lut_ptr = OTP_LUT_PTR.load(Relaxed);
    if !lut_ptr.is_null() {
        let lut_ref = unsafe { &mut *lut_ptr };
        lut_ref.copy_from_slice(&new_lut);
        display.set_otp_lut(lut_ref, ssd1675::display::LutMode::NoInvert);
        display.set_temperature(temp_raw);
    }

    LUT_TEMP_C10.store(current_c10, core::sync::atomic::Ordering::Relaxed);
    defmt::info!("EPD: OTP LUT reloaded for {} °C", temp_celsius);
}

/// Read back the OTP LUT register (command 0x33) using stolen peripherals.
///
/// Sequence (per SSD1619 reference driver):
///   1. Hardware reset + 100 ms settle
///   2. Write temperature (0x18 internal sensor, 0x1A = measured value)
///   3. Send 0x22 / 0xB1 — EnableClock | LoadTemp | LoadLUT-Mode1 | DisableClock
///   4. Send 0x20 — Master Activation (BUSY goes HIGH while controller loads OTP zone)
///   5. Wait for BUSY LOW (controller has loaded the temperature zone into the LUT register)
///   6. Send 0x33 command then read 107 bytes — the loaded LUT zone
///
/// `temp_raw`: temperature in SSD1675 format (1 LSB = 1/16 °C, e.g. 25 °C = 0x0190).
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
    fn pin_nr(p: &Peri<'_, AnyPin>) -> u8 {
        let port = match p.port() {
            Port::Port0 => 0u8,
            Port::Port1 => 1u8,
        };
        port * 32 + p.pin()
    }
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

    // Hardware reset — flat 100 ms settle (BUSY does not reliably pulse during reset/OTP boot).
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
        // 0x18: Temperature Sensor Selection = Internal
        dc_out.set_low();
        spi_tx.write(&[0x18]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0x80]).await.ok();
        // 0x1A: Write Temperature (1 LSB = 1/16 °C)
        dc_out.set_low();
        spi_tx.write(&[0x1A]).await.ok();
        dc_out.set_high();
        spi_tx.write(&temp_raw.to_be_bytes()).await.ok();
        // 0x22 / 0xB1: EnableClock | LoadTemp | LoadLUT-OTP-Mode1 | DisableClock
        dc_out.set_low();
        spi_tx.write(&[0x22]).await.ok();
        dc_out.set_high();
        spi_tx.write(&[0xB1]).await.ok();
        // 0x20: Master Activation — BUSY goes HIGH while the controller loads the OTP zone.
        dc_out.set_low();
        spi_tx.write(&[0x20]).await.ok();
        // Don't drop — Spim::drop disconnects SPI pins.
        core::mem::forget(spi_tx);
    }
    cs_out.set_high();

    // Wait for BUSY LOW: controller has finished loading the temperature zone into the LUT register.
    // Poll every 10 ms, up to 1 s total.
    for _ in 0..100u8 {
        if !busy_in.is_high() {
            break;
        }
        Timer::after_millis(10).await;
    }

    // Phase 2: read 107 bytes from the LUT register (0x33).
    // The controller now presents the loaded zone on MISO.
    // Stack-allocated only for the duration of the SPI read; caller moves it into StaticCell.
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
/// Reads the factory OTP LUT from the display controller before consuming the
/// peripheral tokens. Pass the measured temperature so the controller selects
/// the correct OTP waveform zone.
///
/// `temperature`: temperature in °C. `None` defaults to 20 °C.
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
    temperature: Option<i16>,
    lut_mode: LutMode,
) -> Result<EpdGfx<'a>, Infallible> {
    let temp_celsius = temperature.unwrap_or(20);
    let temp_raw: u16 = ((temp_celsius) as i32 * 16) as u16;
    defmt::debug!(
        "EPD: temperature {} °C (raw 0x{:04x})",
        temp_celsius,
        temp_raw
    );

    // Cache pin numbers for OTP LUT reload at runtime.
    EPD_PIN_SCK.store(cache_pin_nr(&sck_pin), core::sync::atomic::Ordering::Relaxed);
    EPD_PIN_DATA.store(cache_pin_nr(&mosi_pin), core::sync::atomic::Ordering::Relaxed);
    EPD_PIN_CS.store(cache_pin_nr(&csn_pin), core::sync::atomic::Ordering::Relaxed);
    EPD_PIN_DC.store(cache_pin_nr(&dc_pin), core::sync::atomic::Ordering::Relaxed);
    EPD_PIN_RST.store(cache_pin_nr(&resetn_pin), core::sync::atomic::Ordering::Relaxed);
    EPD_PIN_BUSY.store(cache_pin_nr(&busy_pin), core::sync::atomic::Ordering::Relaxed);

    // Store the temperature at which LUTs were loaded (°C × 10).
    LUT_TEMP_C10.store(
        temperature.unwrap_or(20) as i16 * 10,
        core::sync::atomic::Ordering::Relaxed,
    );

    // Read OTP LUT via stolen peripherals before consuming the real tokens.
    // Move raw bytes into static storage immediately — keeps the 107-byte buffer off the stack.
    let otp_lut: &'static mut [u8; 107] = OTP_LUT.init(
        probe_lut(&sck_pin, &mosi_pin, &csn_pin, &dc_pin, &resetn_pin, &busy_pin, temp_raw).await,
    );
    OTP_LUT_PTR.store(otp_lut as *mut _, core::sync::atomic::Ordering::Relaxed);

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
    let mut display = Display::new(controller, config);
    display.set_temperature(temp_raw);
    let mut gfx = GraphicDisplay::new(display, black_buffer, red_buffer, work_buffer);
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

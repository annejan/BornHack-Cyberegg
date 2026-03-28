use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::peripherals;
use embassy_nrf::spim::{Config, Frequency, InterruptHandler, Spim};
use embassy_nrf::temp::Temp;
use embassy_nrf::{Peri, bind_interrupts};
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::StaticCell;

use core::convert::Infallible;
use embassy_nrf::gpio::{Pin as GpioPin, Port};
use ssd1675::{Builder, Dimensions, Display, GraphicDisplay, Interface, Rotation};
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
    TEMP  => embassy_nrf::temp::InterruptHandler;
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

/// Controls whether the inversion phases are zeroed out in the OTP LUT.
///
/// `Invert`   — full factory waveform, includes pre-charge/erase phases (~0.3s inversion visible).
/// `NoInvert` — timing groups 0–2 zeroed, suppresses the visible inversion at the cost of ghosting.
#[derive(Clone, Copy, PartialEq)]
pub enum LutMode {
    Invert,
    NoInvert,
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
    mode: LutMode,
) -> &'static [u8; 107] {
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

    // Safety: all stolen peripherals are dropped before this function returns.
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
    } // spi_tx dropped — SPIM3 disabled
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
    let lut = OTP_LUT.init([0u8; 107]);
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
    } // MOSI released as output
    {
        // Data phase: read 107 bytes on MISO (same physical pin, now input).
        let mut spi_rx = Spim::new_rxonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg,
        );
        spi_rx.read(lut).await.ok();
    }
    cs_out.set_high();

    defmt::info!("Display OTP LUT (107 bytes):");
    for (i, chunk) in lut.chunks(10).enumerate() {
        defmt::info!("  [{=usize:03}] {:02x}", i * 10, chunk);
    }

    if mode == LutMode::NoInvert {
        // Zero the pre-charge/erase timing phases that cause visible inversion.
        // Variant is detected by the same rule used in set_otp_lut().
        if lut[7] != 0 || lut[8] != 0 || lut[9] != 0 {
            // SSD1675: 7-byte waveform rows → waveform 5×7=35 bytes, timing groups 5 bytes each.
            // Zero timing groups 0–2: bytes 35–49 (15 bytes).
            lut[35..50].fill(0);
            lut[51] = 2;
            lut[52] = lut[52] + 0x10;
            lut[53] = 2;
            lut[54] = lut[54] + 0x10;
            lut[55] = 2;
        } else {
            // SSD1675B (data sheet p.14): waveform 5×10=50 bytes (0–49), then 10 timing phases
            // of 5 bytes each (TP[A/B/C/D] + RP) = 50 bytes (50–99).
            // Phase 4 (bytes 70–74) is the erase/inversion phase — zero it.
            // Phases 0–3 and 5–9 drive the actual image and must be kept intact.
            lut[50..64].fill(0);
        }
    }

    lut
}

/// Read the nRF52840 built-in temperature sensor using a stolen peripheral.
///
/// Uses `unsafe { peripherals::TEMP::steal() }` so the caller retains `p.TEMP`
/// for the BLE stack. The stolen peripheral is dropped before returning.
/// Returns temperature in degrees Celsius.
pub async fn read_nrf_temp() -> i16 {
    let mut sensor = Temp::new(unsafe { peripherals::TEMP::steal() }, Irqs);
    let t = sensor.read().await.to_num::<i16>();
    drop(sensor);
    t
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
    defmt::info!(
        "EPD: temperature {} °C (raw 0x{:04x})",
        temp_celsius,
        temp_raw
    );

    // Read OTP LUT via stolen peripherals before consuming the real tokens.
    let otp_lut = probe_lut(
        &sck_pin,
        &mosi_pin,
        &csn_pin,
        &dc_pin,
        &resetn_pin,
        &busy_pin,
        temp_raw,
        lut_mode,
    )
    .await;

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
    gfx.set_otp_lut(otp_lut);
    defmt::info!(
        "Display controller: {}",
        match gfx.variant() {
            ssd1675::display::DisplayVariant::Ssd1675B => "SSD1675B (10-byte row LUT)",
            ssd1675::display::DisplayVariant::Ssd1675 => "SSD1675 (7-byte row LUT)",
        }
    );

    Ok(gfx)
}

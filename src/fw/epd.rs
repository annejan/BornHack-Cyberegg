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

/// Read back the OTP LUT register (command 0x33) using stolen peripherals.
///
/// Pin IDs are derived from the real token references via `psel_bits()` so the
/// board! macro assignments remain the single source of truth.  All stolen
/// resources are dropped before returning; the real tokens stay available for
/// the main SPI init that follows.
async fn probe_lut(
    sck: &Peri<'_, AnyPin>,
    data: &Peri<'_, AnyPin>,
    cs: &Peri<'_, AnyPin>,
    dc: &Peri<'_, AnyPin>,
    rst: &Peri<'_, AnyPin>,
    busy: &Peri<'_, AnyPin>,
) -> &'static [u8; 107] {
    fn pin_nr(p: &Peri<'_, AnyPin>) -> u8 {
        let port = match p.port() { Port::Port0 => 0u8, Port::Port1 => 1u8 };
        port * 32 + p.pin()
    }
    let sck_nr  = pin_nr(sck);
    let data_nr = pin_nr(data);
    let cs_nr   = pin_nr(cs);
    let dc_nr   = pin_nr(dc);
    let rst_nr  = pin_nr(rst);
    let busy_nr = pin_nr(busy);

    // Safety: all stolen peripherals are dropped before this function returns.
    let mut cs_out  = Output::new(unsafe { AnyPin::steal(cs_nr) },   Level::High, OutputDrive::Standard);
    let mut dc_out  = Output::new(unsafe { AnyPin::steal(dc_nr) },   Level::Low,  OutputDrive::Standard);
    let mut rst_out = Output::new(unsafe { AnyPin::steal(rst_nr) },  Level::Low,  OutputDrive::Standard);
    let mut busy_in = Input::new(unsafe { AnyPin::steal(busy_nr) },  Pull::Down);

    let mut cfg = Config::default();
    cfg.frequency = Frequency::M1;
    let cfg2 = cfg.clone();

    // Hardware reset to ensure OTP LUT is freshly loaded into the LUT register.
    Timer::after_millis(10).await;
    rst_out.set_high();
    busy_in.wait_for_low().await;

    // Phase 1: send command 0x33 — data line is output (MOSI).
    cs_out.set_low();
    dc_out.set_low();
    {
        let mut spi_tx = Spim::new_txonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) },
            cfg,
        );
        spi_tx.write(&[0x33]).await.ok();
    } // spi_tx dropped — SPIM3 disabled, data line released as output
    dc_out.set_high();

    // Phase 2: read LUT bytes — data line is now input (MISO), display drives it.
    // 76 bytes: 70 bytes LUT + 6 bytes factory voltage/timing config
    //   byte 70 → gate driving voltage (reg 0x03)
    //   bytes 71-73 → source driving voltages VSH1, VSH2, VSL (reg 0x04)
    //   byte 74 → dummy line period (reg 0x3A)
    //   byte 75 → gate line width (reg 0x3B)
    // SSD1680 only streams 70 bytes; bytes 70-75 will read as 0xFF (bus idle).
    let lut = OTP_LUT.init([0u8; 107]);
    {
        let mut spi_rx = Spim::new_rxonly(
            unsafe { peripherals::SPI3::steal() },
            Irqs,
            unsafe { AnyPin::steal(sck_nr) },
            unsafe { AnyPin::steal(data_nr) }, // same physical pin, now MISO
            cfg2,
        );
        spi_rx.read(lut).await.ok();
    }
    cs_out.set_high();

    defmt::info!("Display OTP (76 bytes — LUT[0..70] + config[70..76]):");
    for (i, chunk) in lut.chunks(10).enumerate() {
        defmt::info!("  [{=usize:03}] {:02x}", i * 10, chunk);
    }
    defmt::info!("  [070] gate_vgh={:02x} vsh1={:02x} vsh2={:02x} vsl={:02x} dummy_line={:02x} gate_width={:02x}",
        lut[70], lut[71], lut[72], lut[73], lut[74], lut[75]);

    lut
}

/// Initialize the EPD display (SSD1680 1.54" 152x152, SPIM3 interface).
///
/// Reads the factory OTP LUT from the display controller before consuming the
/// peripheral tokens, then initializes the SPI bus and display and stores the
/// OTP LUT for use in subsequent `update_bw` calls.
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
    // Read OTP LUT via stolen peripherals before consuming the real tokens.
    let otp_lut = probe_lut(&sck_pin, &mosi_pin, &csn_pin, &dc_pin, &resetn_pin, &busy_pin).await;

    // Build the SPI bus.
    let mut cfg = Config::default();
    cfg.frequency = Frequency::M1;
    let bus = Spim::new_txonly(spi, Irqs, sck_pin, mosi_pin, cfg);

    // Initialize GPIO pins.
    let csn_out    = Output::new(csn_pin,    Level::High, OutputDrive::Standard);
    let resetn_out = Output::new(resetn_pin, Level::Low,  OutputDrive::Standard);
    let dc_out     = Output::new(dc_pin,     Level::Low,  OutputDrive::Standard);
    let busy_in    = Input::new(busy_pin,    Pull::Down);

    let spi_dev = ExclusiveDevice::new(bus, csn_out, embassy_time::Delay).unwrap();

    let controller = ssd1675::Interface::new(spi_dev, busy_in, dc_out, resetn_out);
    let config = Builder::new()
        .dimensions(dimension)
        .rotation(Rotation::Rotate0)
        .build()
        .unwrap();
    let display = Display::new(controller, config);
    let mut gfx = GraphicDisplay::new(display, black_buffer, red_buffer, work_buffer);
    gfx.set_otp_lut(otp_lut);

    Ok(gfx)
}

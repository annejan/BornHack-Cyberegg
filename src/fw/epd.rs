use embassy_embedded_hal::shared_bus::asynch::spi::SpiDevice;
use embassy_nrf::gpio::{AnyPin, Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::peripherals;
use embassy_nrf::spim::{Config, Frequency, InterruptHandler, Spim};
use embassy_nrf::{Peri, bind_interrupts};

use core::convert::Infallible;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use ssd1680::{Builder, Dimensions, Display, GraphicDisplay, Interface, Rotation};
use {defmt_rtt as _, panic_probe as _};

// EPD display configuration - compile-time constants with generics
pub struct EpdConfig<const ROWS: u16, const COLS: u8>;

impl<const ROWS: u16, const COLS: u8> EpdConfig<ROWS, COLS> {
    /// Buffer size in bytes (rows * cols / 8)
    pub const BUF_SIZE: usize = ROWS as usize * COLS as usize / 8;

    /// Get Dimensions for ssd1680 driver
    pub const fn to_dimensions() -> Dimensions {
        Dimensions {
            rows: ROWS,
            cols: COLS,
        }
    }
}

// Type aliases for common display sizes
pub type EpdConfig152x152 = EpdConfig<152, 152>;

// LUTs used to configure the display
#[rustfmt::skip]
const LUT: [u8; 70] = [
    // Phase 0     Phase 1     Phase 2     Phase 3     Phase 4     Phase 5     Phase 6
    // A B C D     A B C D     A B C D     A B C D     A B C D     A B C D     A B C D
    0b01001000, 0b10100000, 0b00010000, 0b00010000, 0b00010011, 0b00000000, 0b00000000,  // LUT0 - Black
    0b01001000, 0b10100000, 0b10000000, 0b00000000, 0b00000011, 0b00000000, 0b00000000,  // LUTT1 - White
    0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000,  // IGNORE
    0b01001000, 0b10100101, 0b00000000, 0b10111011, 0b00000000, 0b00000000, 0b00000000,  // LUT3 - Red
    0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000, 0b00000000,  // LUT4 - VCOM

    // Duration            |  Repeat
    // A   B     C     D   |
    64,   12,   32,   12,    6,   // 0 Flash
    16,   8,    4,    4,     6,   // 1 clear
    4,    8,    8,    16,    16,  // 2 bring in the black
    2,    2,    2,    64,    32,  // 3 time for red
    2,    2,    2,    2,     2,   // 4 final black sharpen phase
    0,    0,    0,    0,     0,   // 5
    0,    0,    0,    0,     0    // 6
];

bind_interrupts!(struct Irqs {
    SPIM3 => InterruptHandler<peripherals::SPI3>;
});

pub type EpdBus<'a> = Mutex<NoopRawMutex, Spim<'a>>;
pub type EpdGfx<'a> = GraphicDisplay<
    'a,
    Interface<
        SpiDevice<'a, NoopRawMutex, Spim<'a>, Output<'a>>,
        embassy_nrf::spim::Error,
        core::convert::Infallible,
        Input<'a>,
        Output<'a>,
        Output<'a>,
    >,
    &'a mut [u8],
>;

/// Initialize the EPD display SPI bus
///
/// # Arguments
/// * `spi`: The SPI peripheral
/// * `sck_pin`: The SCK pin
/// * `mosi_pin`: The MOSI pin
///
/// # Returns
/// A mutex wrapped SPI bus that can be shared between divices
///
pub fn init_epd_bus<'a>(
    spi: Peri<'a, peripherals::SPI3>,
    sck_pin: Peri<'a, AnyPin>,
    mosi_pin: Peri<'a, AnyPin>,
) -> EpdBus<'a> {
    let mut cfg = Config::default();
    cfg.frequency = Frequency::M1;
    let spim = Spim::new_txonly(spi, Irqs, sck_pin, mosi_pin, cfg);
    Mutex::<NoopRawMutex, _>::new(spim)
}

/// Initialize the EPD display
/// SSD1680 1.54" 152x152 EPD display
/// 24-pin connector, SPIM3 interface
///
/// # Arguments
/// * `bus`: The mutex wrapped SPI bus
/// * `busy_pin`: The EPD busy pin
/// * `resetn_pin`: The EPD resetn pin (Active low)
/// * `dc_pin`: The EPD DATA/COMMAND pin
/// * `csn_pin`: The EPD chip select pin (Active low)
/// * `dimension`: The display dimensions
/// * `black_buffer`: The buffer for the black image
/// * `red_buffer`: The buffer for the red image
///
/// # Returns
/// A graphics display object that can be used to draw graphics
///
pub fn init_epd<'a>(
    bus: &'a EpdBus<'a>,
    busy_pin: Peri<'a, AnyPin>,
    resetn_pin: Peri<'a, AnyPin>,
    dc_pin: Peri<'a, AnyPin>,
    csn_pin: Peri<'a, AnyPin>,
    dimension: Dimensions,
    black_buffer: &'a mut [u8],
    red_buffer: &'a mut [u8],
) -> Result<EpdGfx<'a>, Infallible> {
    // Initialize GPIO pins
    let csn_out = Output::new(csn_pin, Level::High, OutputDrive::Standard);
    let resetn_out = Output::new(resetn_pin, Level::Low, OutputDrive::Standard);
    let dc_out = Output::new(dc_pin, Level::Low, OutputDrive::Standard);
    let busy_in = Input::new(busy_pin, Pull::Down);

    // Initialize the SPI peripheral to communicate with the EPD
    let spi_dev = SpiDevice::new(bus, csn_out);

    // Initialize the SSD1680 display
    let controller = ssd1680::Interface::new(spi_dev, busy_in, dc_out, resetn_out);
    let config = Builder::new()
        .dimensions(dimension)
        .rotation(Rotation::Rotate0)
        .lut(&LUT)
        .build()
        .unwrap();
    let display = Display::new(controller, config);
    let gfx = GraphicDisplay::new(display, black_buffer, red_buffer);

    Ok(gfx)
}

#![no_std]
#![no_main]

// Code is for NRF52840
use embassy_executor::Spawner;
use embassy_nrf::gpio::Pull;
use embassy_nrf::{
    gpio::{Level, Output, OutputDrive},
    gpiote::{InputChannel, InputChannelPolarity},
};
use embassy_time::Timer;
use hello_graphics::{
    board, draw_graphics,
    fw::epd::{EpdBus, EpdConfig152x152 as EpdConfig, EpdGfx, init_epd, init_epd_bus},
};
use ssd1680::graphics::WHITE;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// Example to port: https://github.com/mbv/esp32-ssd1680/blob/main/src/main.rs

// Pin assignments SSD1680 EDP

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    // EPD display buffers
    let dimension = EpdConfig::to_dimensions();
    static BLACK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static RED_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    let black_buffer = BLACK_BUF.init([0; EpdConfig::BUF_SIZE]);
    let red_buffer = RED_BUF.init([0; EpdConfig::BUF_SIZE]);

    // LED (Low active)
    let mut led_red = Output::new(board!(p, led_red), Level::High, OutputDrive::Standard);
    let mut led_green = Output::new(board!(p, led_green), Level::High, OutputDrive::Standard);
    let mut led_blue = Output::new(board!(p, led_blue), Level::High, OutputDrive::Standard);

    defmt::info!("Init EPD");

    static BUS_CELL: StaticCell<EpdBus> = StaticCell::new();
    let bus = BUS_CELL.init(init_epd_bus(
        board!(p, epd_spi),
        board!(p, epd_sck),
        board!(p, epd_mosi),
    ));
    let mut display: EpdGfx<'_> = init_epd(
        bus,
        board!(p, epd_busy),
        board!(p, epd_reset),
        board!(p, epd_dc),
        board!(p, epd_csn),
        dimension,
        black_buffer,
        red_buffer,
    )
    .unwrap();

    let _ = display.reset().await;
    display.clear(WHITE);

    defmt::info!("EPD initialized");
    defmt::info!("Draw graphics");
    draw_graphics(&mut display).unwrap();
    defmt::info!("Entering main loop...");

    // Configure button input channels
    let mut ch_can = InputChannel::new(
        p.GPIOTE_CH0,
        board!(p, btn_can),
        Pull::Up,
        InputChannelPolarity::Toggle,
    );
    let mut ch_exe = InputChannel::new(
        p.GPIOTE_CH1,
        board!(p, btn_exe),
        Pull::Up,
        InputChannelPolarity::Toggle,
    );

    // Button press handlers that test LEDs
    let button1 = async {
        loop {
            ch_can.wait().await;
            led_green.set_level(ch_can.pin().get_level());
        }
    };

    let button2 = async {
        loop {
            ch_exe.wait().await;
            led_blue.set_level(ch_exe.pin().get_level());
        }
    };

    let main_loop = async {
        loop {
            led_red.set_low();
            Timer::after_millis(50).await;
            led_red.set_high();
            Timer::after_millis(4950).await;

            let _ = display.reset().await;
            let _ = display.update().await;
            defmt::info!("Updated EPD");
            let _ = display.deep_sleep().await.unwrap();
        }
    };

    embassy_futures::join::join3(main_loop, button1, button2).await;
}

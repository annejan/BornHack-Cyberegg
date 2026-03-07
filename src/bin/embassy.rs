#![no_std]
#![no_main]

// Code is for NRF52840
use embassy_executor::Spawner;
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_time::Timer;
use hello_graphics::fw::sx1262::run_lora_test;
use hello_graphics::{DISPLAY_STATE, health_err, update_health, with_health};
use hello_graphics::{
    board, draw_graphics,
    fw::epd::{EpdBus, EpdConfig152x152 as EpdConfig, EpdGfx, init_epd, init_epd_bus},
    fw::nfct::run_nfct,
};
use ssd1680::graphics::WHITE;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// Example to port: https://github.com/mbv/esp32-ssd1680/blob/main/src/main.rs

// Pin assignments SSD1680 EDP

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_nrf::config::Config::default();
    // We paid for the XTAL on the BOM, se let's use it.
    config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(config);

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
    let health_str = with_health!(|h| h.to_string());
    match draw_graphics(&mut display, &health_str) {
        Ok(_) => {}
        Err(_) => {
            health_err!(epd, "Failed to draw graphics");
            // Continue with the rest of the program, error is set
        }
    }
    defmt::info!("Entering main loop...");

    // Configure button input channels
    // Configure all buttons as simple inputs
    let mut btn_can = Input::new(board!(p, btn_can), Pull::Up);
    let mut btn_exe = Input::new(board!(p, btn_exe), Pull::Up);
    let mut joy_up = Input::new(board!(p, joy_up), Pull::Up);
    let mut joy_down = Input::new(board!(p, joy_down), Pull::Up);
    let mut joy_left = Input::new(board!(p, joy_left), Pull::Up);
    let mut joy_right = Input::new(board!(p, joy_right), Pull::Up);
    let mut joy_fire = Input::new(board!(p, joy_fire), Pull::Up);

    // Combined button handler
    let buttons = async {
        loop {
            let (_, index) = embassy_futures::select::select_array([
                btn_can.wait_for_any_edge(),
                btn_exe.wait_for_any_edge(),
                joy_up.wait_for_any_edge(),
                joy_down.wait_for_any_edge(),
                joy_left.wait_for_any_edge(),
                joy_right.wait_for_any_edge(),
                joy_fire.wait_for_any_edge(),
            ])
            .await;

            // Handle the specific button that was pressed (active low)
            match index {
                0 => {
                    update_health!(|f| f.buttons.up.seen_low = true);
                    led_green.set_low();
                    defmt::info!("Cancel button {}", btn_can.is_low());
                    btn_can.wait_for_rising_edge().await;
                    led_green.set_high();
                    update_health!(|f| f.buttons.up.seen_high = true);
                }
                1 => {
                    led_blue.set_low();
                    defmt::info!("Execute button pressed");
                    update_health!(|f| f.buttons.execute.seen_low = true);
                    btn_exe.wait_for_rising_edge().await;
                    led_blue.set_high();
                    update_health!(|f| f.buttons.execute.seen_high = true);
                }
                2 => {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().menu_up());
                    defmt::info!("Menu up");
                    update_health!(|f| f.buttons.up.seen_low = true);
                    joy_up.wait_for_rising_edge().await;
                    update_health!(|f| f.buttons.up.seen_high = true);
                }
                3 => {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().menu_down());
                    defmt::info!("Menu down");
                    update_health!(|f| f.buttons.down.seen_low = true);
                    joy_down.wait_for_rising_edge().await;
                    update_health!(|f| f.buttons.down.seen_high = true);
                }
                4 => {
                    defmt::info!("Joystick left");
                    update_health!(|f| f.buttons.left.seen_low = true);
                    joy_up.wait_for_rising_edge().await;
                    update_health!(|f| f.buttons.left.seen_high = true);
                }
                5 => {
                    defmt::info!("Joystick right");
                    update_health!(|f| f.buttons.right.seen_low = true);
                    joy_down.wait_for_rising_edge().await;
                    update_health!(|f| f.buttons.right.seen_high = true);
                }
                6 => {
                    DISPLAY_STATE.lock(|f| f.borrow_mut().set_fire_button(joy_fire.is_low()));
                    defmt::info!("Joystick fire: {}", joy_fire.is_low());
                    update_health!(|f| f.buttons.fire.seen_low = true);
                    joy_fire.wait_for_rising_edge().await;
                    update_health!(|f| f.buttons.fire.seen_high = true);
                }
                _ => unreachable!(),
            }
        }
    };

    // Run NFC tag emulation
    let run_nfc = run_nfct(p.NFCT);

    // Blink and EPD test
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

            // Also print health state to the console
            let health_str = with_health!(|f| f.to_string());
            defmt::info!("Health: {}", health_str.as_str());
        }
    };

    let run_lora = run_lora_test(
        board!(p, lora_spi),
        board!(p, lora_sck),
        board!(p, lora_mosi),
        board!(p, lora_miso),
        board!(p, lora_rst),
        board!(p, lora_nss),
        board!(p, lora_busy),
        board!(p, lora_dio1),
        board!(p, lora_rf_sw),
    );

    embassy_futures::join::join4(main_loop, run_nfc, run_lora, buttons).await;
}

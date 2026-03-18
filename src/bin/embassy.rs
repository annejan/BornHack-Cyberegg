#![no_std]
#![no_main]

// Code is for NRF52840
use embassy_boot_nrf::{AlignedBuffer, BlockingFirmwareUpdater, FirmwareUpdaterConfig};
use embassy_executor::Spawner;
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::nvmc::Nvmc;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::{Duration, Ticker, Timer};
use hello_graphics::fw::button::BTN_WATCH;
use hello_graphics::fw::sx1262::run_lora_test;
use hello_graphics::{
    board, draw_graphics,
    fw::button::run_buttons,
    fw::buzzer::{Buzzer, melodies},
    fw::epd::{EpdBus, EpdConfig152x152 as EpdConfig, EpdGfx, init_epd, init_epd_bus},
    fw::nfct::run_nfct,
};
use hello_graphics::{health_err, with_health};
use ssd1680::graphics::WHITE;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// Example to port: https://github.com/mbv/esp32-ssd1680/blob/main/src/main.rs

// Pin assignments SSD1680 EDP

// Feed the watchdog started by the bootloader (channel 0, 5s timeout).
async fn feed_watchdog() -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    loop {
        embassy_nrf::pac::WDT
            .rr(0)
            .write(|w| w.set_rr(embassy_nrf::pac::wdt::vals::Rr::RELOAD));
        ticker.next().await;
    }
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let mut config = embassy_nrf::config::Config::default();
    // We paid for the XTAL on the BOM, se let's use it.
    config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(config);

    // Without a SWD debugger attached, Embassy's WFE sleep can gate HFCLK, causing
    // variable SPI and GPIO interrupt latency. CONSTLAT keeps HFCLK running during
    // sleep, matching the behaviour seen when a debugger is connected.
    embassy_nrf::pac::POWER.tasks_constlat().write_value(1);

    // Power supply pin
    let _ps_sync = Output::new(board!(p, ps_sync), Level::High, OutputDrive::Standard);

    // EPD display buffers
    let dimension = EpdConfig::to_dimensions();
    static BLACK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static RED_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static WORK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    let black_buffer = BLACK_BUF.init([0; EpdConfig::BUF_SIZE]);
    let red_buffer = RED_BUF.init([0; EpdConfig::BUF_SIZE]);
    let work_buffer = WORK_BUF.init([0; EpdConfig::BUF_SIZE]);

    // LED (Low active)
    let mut led_red = Output::new(board!(p, led_red), Level::High, OutputDrive::Standard);
    let mut led_green = Output::new(board!(p, led_green), Level::High, OutputDrive::Standard);
    let mut led_blue = Output::new(board!(p, led_blue), Level::High, OutputDrive::Standard);

    Timer::after_millis(500).await;
    defmt::info!("Init EPD");

    static BUS_CELL: StaticCell<EpdBus> = StaticCell::new();
    let bus = BUS_CELL.init(init_epd_bus(
        board!(p, epd_spi),
        board!(p, epd_sck).into(),
        board!(p, epd_mosi).into(),
    ));
    let mut display: EpdGfx<'_> = init_epd(
        bus,
        board!(p, epd_busy).into(),
        board!(p, epd_reset).into(),
        board!(p, epd_dc).into(),
        board!(p, epd_csn).into(),
        dimension,
        black_buffer,
        red_buffer,
        work_buffer,
    )
    .unwrap();

    // Wait for EPD power-on reset to complete before sending commands.
    // On cold boot the display's internal power rails need time to stabilise.
    // With a debugger the flash programming delay provides this naturally;
    // without one the firmware starts almost immediately after power-on.
    // Timer::after_millis(10).await;
    // let _ = display.reset().await;
    // display.clear(WHITE);

    defmt::info!("EPD initialized");

    defmt::info!("Configure button GPIO");

    // Configure button input channels
    let btn_can = Input::new(board!(p, btn_can), Pull::Up);
    let btn_exe = Input::new(board!(p, btn_exe), Pull::Up);
    let joy_up = Input::new(board!(p, joy_up), Pull::Up);
    let joy_down = Input::new(board!(p, joy_down), Pull::Up);
    let joy_left = Input::new(board!(p, joy_left), Pull::Up);
    let joy_right = Input::new(board!(p, joy_right), Pull::Up);
    let joy_fire = Input::new(board!(p, joy_fire), Pull::Up);

    let mut button_rcvr = BTN_WATCH.receiver().unwrap();
    let buttons = run_buttons(
        btn_can, btn_exe, joy_up, joy_down, joy_left, joy_right, joy_fire,
    );
    defmt::info!("Button GPIO configured");

    // Run NFC tag emulation
    let run_nfc = run_nfct(p.NFCT);

    let mut buzzer = Buzzer::new(Output::new(
        board!(p, buzzer),
        Level::Low,
        OutputDrive::Standard,
    ));
    // buzzer.play_melody(melodies::IMPERIAL_MARCH).await;
    buzzer.play_melody(melodies::STARTUP).await;

    // Blink all three LEDs once to signal firmware has started
    led_red.set_low();
    led_green.set_low();
    led_blue.set_low();
    Timer::after_millis(200).await;
    led_red.set_high();
    led_green.set_high();
    led_blue.set_high();
    Timer::after_millis(200).await;

    // Number of fast B&W updates before a full tricolor refresh.
    const FAST_UPDATES_PER_FULL: u32 = 60;

    // All peripherals initialised successfully — commit this firmware so the
    // bootloader doesn't roll it back on the next reset.
    {
        let flash = Mutex::<NoopRawMutex, _>::new(core::cell::RefCell::new(Nvmc::new(p.NVMC)));
        let fw_config = FirmwareUpdaterConfig::from_linkerfile_blocking(&flash, &flash);
        let mut aligned = AlignedBuffer([0u8; 4]);
        let mut updater = BlockingFirmwareUpdater::new(fw_config, &mut aligned.0);
        let _ = updater.mark_booted();
        defmt::info!("Firmware marked as booted");
    }

    defmt::info!("Entering main loop...");
    // Blink and EPD test
    let main_loop = async {
        let mut loop_count: u32 = 0;
        loop {
            // Red blink = loop heartbeat
            let _ = display.clear(WHITE);

            let health_str = with_health!(|f| f.to_string());
            match draw_graphics(&mut display, &health_str) {
                Ok(_) => {}
                Err(_) => {
                    health_err!(epd, "Failed to draw graphics");
                }
            }
            defmt::info!("Health: {}", health_str.as_str());

            let _ = display.reset().await;
            if loop_count % (FAST_UPDATES_PER_FULL + 1) == FAST_UPDATES_PER_FULL {
                defmt::info!("Full tricolor refresh");
                let _ = display.update().await;
            } else {
                defmt::info!("Fast B&W refresh");
                let _ = display.update_ghost().await;
            }
            loop_count = loop_count.wrapping_add(1);

            let _ = display.deep_sleep().await;

            led_red.set_low();
            Timer::after_millis(50).await;
            led_red.set_high();
            // Timer::after_millis(950).await;
            button_rcvr.changed().await;
        }
    };

    let run_lora = run_lora_test(
        board!(p, lora_spi),
        board!(p, lora_sck).into(),
        board!(p, lora_mosi).into(),
        board!(p, lora_miso).into(),
        board!(p, lora_rst).into(),
        board!(p, lora_nss).into(),
        board!(p, lora_busy).into(),
        board!(p, lora_dio1).into(),
        board!(p, lora_rf_sw).into(),
    );

    embassy_futures::join::join5(main_loop, run_nfc, buttons, run_lora, feed_watchdog()).await;
}

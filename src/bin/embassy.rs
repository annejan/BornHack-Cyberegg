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
use hello_graphics::fw::battery::{self, battery_task, init as init_battery};
use hello_graphics::fw::ble::{init_ble, run_ble_peripheral};
use hello_graphics::fw::bonds::bond_task;
use hello_graphics::fw::button::BTN_WATCH;
use hello_graphics::fw::device_id;
use hello_graphics::fw::kv;
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
async fn main(spawner: Spawner) {
    let mut config = embassy_nrf::config::Config::default();
    // We paid for the XTAL on the BOM, so let's use it.
    config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(config);

    // Without a SWD debugger attached, Embassy's WFE sleep can gate HFCLK, causing
    // variable SPI and GPIO interrupt latency. CONSTLAT keeps HFCLK running during
    // sleep, matching the behaviour seen when a debugger is connected.
    embassy_nrf::pac::POWER.tasks_constlat().write_value(1);

    device_id::init();
    let [id0, id1] = device_id::get();
    defmt::info!("Device ID: {:02X}{:02X}", id0, id1);

    // Power supply pin
    // Pulling this pin low, puts the buck-boost converter in low power mode (3mA instead of 30mA idle current).
    let _ps_sync = Output::new(board!(p, ps_sync), Level::Low, OutputDrive::Standard);

    // -----------------------------------------------------------------------
    // KV store (QSPI flash) — must come before BLE so bonds are loaded first.
    // -----------------------------------------------------------------------
    match kv::init(
        p.QSPI,
        board!(p, flash_sck),
        board!(p, flash_csn),
        board!(p, flash_io0),
        board!(p, flash_io1),
        board!(p, flash_io2),
        board!(p, flash_io3),
    )
    .await
    {
        Ok(()) => {}
        Err(id) => defmt::panic!(
            "QSPI flash not reachable (JEDEC ID: {:02X} {:02X} {:02X})",
            id[0],
            id[1],
            id[2]
        ),
    }
    spawner.must_spawn(bond_task());

    // -----------------------------------------------------------------------
    // BLE (MPSL + SDC + TrouBLE peripheral task)
    // -----------------------------------------------------------------------
    static SDC_MEM: StaticCell<nrf_sdc::Mem<4096>> = StaticCell::new();
    // init_ble returns SoftdeviceController<'static> directly.
    // PPI channels are reserved hardware crossbar slots used by MPSL (CH19,30,31) and
    // the SoftDevice Controller (CH17-29 minus the MPSL ones) for timing-critical BLE
    // radio events.  They must not be used elsewhere in the application.
    let sdc = init_ble(
        &spawner,
        p.RTC0,
        p.TIMER0,
        p.TEMP,
        p.PPI_CH19,
        p.PPI_CH30,
        p.PPI_CH31,
        p.PPI_CH17,
        p.PPI_CH18,
        p.PPI_CH20,
        p.PPI_CH21,
        p.PPI_CH22,
        p.PPI_CH23,
        p.PPI_CH24,
        p.PPI_CH25,
        p.PPI_CH26,
        p.PPI_CH27,
        p.PPI_CH28,
        p.PPI_CH29,
        p.RNG,
        SDC_MEM.init(nrf_sdc::Mem::new()),
    );
    spawner.must_spawn(run_ble_peripheral(sdc));

    // -----------------------------------------------------------------------
    // EPD display
    // -----------------------------------------------------------------------
    let dimension = EpdConfig::to_dimensions();
    static BLACK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static RED_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static WORK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    let black_buffer = BLACK_BUF.init([0; EpdConfig::BUF_SIZE]);
    let red_buffer = RED_BUF.init([0; EpdConfig::BUF_SIZE]);
    let work_buffer = WORK_BUF.init([0; EpdConfig::BUF_SIZE]);

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

    defmt::info!("EPD initialized");
    defmt::info!("Configure button GPIO");

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

    defmt::info!("Initializing NFC");
    let run_nfc = run_nfct(p.NFCT);
    defmt::info!("NFC initialized");

    defmt::info!("Initializing buzzer");
    let mut buzzer = Buzzer::new(Output::new(
        board!(p, buzzer),
        Level::Low,
        OutputDrive::Standard,
    ));
    buzzer.play_melody(melodies::STARTUP).await;
    defmt::info!("Buzzer initialized, startup melody played");

    defmt::info!("Initializing battery monitor");
    let battery_monitor = match init_battery(p.SAADC, board!(p, vbat), board!(p, vbat_rd)).await {
        Ok(monitor) => {
            defmt::info!("Battery monitor initialized");
            monitor
        }
        Err(e) => {
            defmt::error!("Battery monitor initialization failed: {:?}", e);
            return;
        }
    };
    spawner.must_spawn(battery_task(battery_monitor));

    // White light blink indicating we can enter the main loop
    led_red.set_low();
    led_green.set_low();
    led_blue.set_low();
    Timer::after_millis(200).await;
    led_red.set_high();
    led_green.set_high();
    led_blue.set_high();
    Timer::after_millis(200).await;

    const FAST_UPDATES_PER_FULL: u32 = 60;

    // All peripherals initialised — commit this firmware so the bootloader won't roll back.
    {
        let flash = Mutex::<NoopRawMutex, _>::new(core::cell::RefCell::new(Nvmc::new(p.NVMC)));
        let fw_config = FirmwareUpdaterConfig::from_linkerfile_blocking(&flash, &flash);
        let mut aligned = AlignedBuffer([0u8; 4]);
        let mut updater = BlockingFirmwareUpdater::new(fw_config, &mut aligned.0);
        let _ = updater.mark_booted();
        defmt::info!("Firmware marked as booted");
    }

    defmt::info!("Entering main loop...");
    let main_loop = async {
        let mut loop_count: u32 = 0;
        loop {
            let _ = display.clear(WHITE);
            let health_str = with_health!(|f| f.to_string());
            let bat_str = battery::read_pct();
            match draw_graphics(&mut display, &health_str, &bat_str) {
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
            // Wait for button event to update EPD display
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

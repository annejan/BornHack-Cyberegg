#![no_std]
#![no_main]

// Code is for NRF52840
use embassy_executor::Spawner;
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::{Input, Pull};
use embassy_nrf::gpio::{Level, Output, OutputDrive};
use embassy_nrf::pac::wdt::vals::{Halt as WdtHalt, Sleep as WdtSleep};
use embassy_nrf::pwm::SimplePwm;
use embassy_nrf::wdt::{Config as WdtConfig, Watchdog};
use embassy_time::Timer;
use hello_graphics::fw::battery::{self, battery_task, init as init_battery};
use hello_graphics::fw::ble::{CompanionContext, init_ble, run_ble_peripheral};
use hello_graphics::fw::bonds::bond_task;
use hello_graphics::fw::button::BTN_WATCH;
use hello_graphics::fw::contacts::ContactStore;
use hello_graphics::fw::device_id;
use hello_graphics::fw::images::badgercorn::BADGERCORN_DATA;
use hello_graphics::fw::kv;
use hello_graphics::fw::meshcore::run_meshcore_listener;
use hello_graphics::fw::settings;
use hello_graphics::{
    ADVERT_SIGNAL, BLE_PAIRING_SIGNAL, DISPLAY_STATE, LORA_MSG_SIGNAL, MINUTE_TICK,
    health_err, unix_now, with_health,
};
use hello_graphics::{
    board, draw_graphics,
    fw::button::run_buttons,
    fw::buzzer::{Buzzer, buzzer_task, play as play_melody},
    fw::epd::{EpdConfig152x152 as EpdConfig, EpdGfx, LutMode, init_epd},
    fw::nfct::run_nfct,
};
use ssd1675::UpdateMode;
use ssd1675::graphics::Color;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

/// Fires `MINUTE_TICK` at every minute boundary so the display updates the clock.
#[embassy_executor::task]
async fn minute_tick_task() {
    loop {
        let secs_until_next = unix_now()
            .map(|t| 60 - (t % 60) as u64)
            .unwrap_or(60);
        Timer::after(embassy_time::Duration::from_secs(secs_until_next)).await;
        MINUTE_TICK.signal(());
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

    // Start a 5-second watchdog. Pauses when a debugger halts the CPU so
    // probe-rs attach doesn't trigger spurious resets.
    // A separate task pets it every 2 seconds; if any task starves the
    // executor the WDT fires, resets, and panic-probe prints a stack trace.
    let mut wdt_config = WdtConfig::default();
    wdt_config.timeout_ticks = 5 * 32768; // 5 seconds
    wdt_config.action_during_debug_halt = WdtHalt::PAUSE;
    wdt_config.action_during_sleep = WdtSleep::RUN;
    let (_wdt, [wdt_handle]) = Watchdog::try_new(p.WDT, wdt_config).expect("WDT init failed");
    spawner.must_spawn(pet_watchdog_task(wdt_handle));

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
    // Contact store — migrate if MAX_CONTACTS changed since last boot.
    // -----------------------------------------------------------------------
    ContactStore::new().init().await;

    // -----------------------------------------------------------------------
    // MeshCore device identity (load from KV or generate via TRNG)
    // Must happen before BLE init, which consumes p.RNG.
    // -----------------------------------------------------------------------
    let identity = settings::load_or_create_identity().await;

    // Read nRF52840 die temperature before BLE init consumes p.TEMP.
    let temp_celsius = hello_graphics::fw::temperature::read_and_cache().await;
    defmt::info!("Die temperature: {} °C", temp_celsius);

    // -----------------------------------------------------------------------
    // BLE (MPSL + SDC + TrouBLE peripheral task)
    // -----------------------------------------------------------------------

    // Collect entropy for the BLE security manager PRNG *before* init_ble
    // consumes p.RNG — the SDC holds the peripheral for its lifetime.
    let ble_prng_seed = hello_graphics::fw::device_identity::trng_seed(); // TRNG used before RNG peripheral is consumed by SDC

    static SDC_MEM: StaticCell<nrf_sdc::Mem<{ hello_graphics::fw::ble::SDC_MEM_SIZE }>> =
        StaticCell::new();
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
    let companion_ctx = CompanionContext {
        pub_key: identity.pub_key,
    };
    spawner.must_spawn(run_ble_peripheral(sdc, companion_ctx, ble_prng_seed));

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

    defmt::info!("Init EPD");

    let mut display: EpdGfx<'_> = init_epd(
        board!(p, epd_spi),
        board!(p, epd_sck).into(),
        board!(p, epd_mosi).into(),
        board!(p, epd_busy).into(),
        board!(p, epd_reset).into(),
        board!(p, epd_dc).into(),
        board!(p, epd_csn).into(),
        dimension,
        black_buffer,
        red_buffer,
        work_buffer,
        Some(temp_celsius),
        LutMode::NoInvert,
    )
    .await
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
    let buzzer = Buzzer::new(SimplePwm::new_1ch(
        p.PWM0,
        board!(p, buzzer),
        &Default::default(),
    ));
    spawner.must_spawn(buzzer_task(buzzer));
    // play_melody(0); // 0 = STARTUP
    defmt::info!("Buzzer task spawned, startup melody queued");

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
    spawner.must_spawn(minute_tick_task());

    // White light blink indicating we can enter the main loop
    led_red.set_low();
    led_green.set_low();
    led_blue.set_low();
    Timer::after_millis(200).await;
    led_red.set_high();
    led_green.set_high();
    led_blue.set_high();
    Timer::after_millis(200).await;

    defmt::info!("Entering main loop...");
    let main_loop = async {
        loop {
            let _ = display.clear(Color::White);
            // Snapshot active screen once; used for rendering and event filtering below.
            let active_screen = DISPLAY_STATE.lock(|f| f.borrow().active_screen());
            if active_screen == 4 {
                display.blit(Some(BADGERCORN_DATA), None);
            } else {
                let health_str = with_health!(|f| f.to_string());
                let bat_str = battery::read_pct();
                match draw_graphics(&mut display, &health_str, &bat_str) {
                    Ok(_) => {}
                    Err(_) => {
                        health_err!(epd, "Failed to draw graphics");
                    }
                }
            }

            // Race the display update against incoming events.
            // If an event arrives first, the update future is dropped — the EPD
            // controller may be mid-refresh, but the hardware reset at the top
            // of the next iteration will abort it and reinitialise the controller.
            // If the update finishes first, we wait for the next event normally.
            use embassy_futures::select::{Either, Either4, select, select4};
            let update_completed = matches!(
                select(
                    async {
                        let _ = display.reset().await;
                        let _ = display.update_bw(UpdateMode::Mode1).await;
                        let _ = display.deep_sleep().await;
                    },
                    async {
                        loop {
                            match select(
                                select4(
                                    button_rcvr.changed(),
                                    LORA_MSG_SIGNAL.wait(),
                                    ADVERT_SIGNAL.wait(),
                                    BLE_PAIRING_SIGNAL.wait(),
                                ),
                                MINUTE_TICK.wait(),
                            )
                            .await
                            {
                                Either::First(Either4::First(_)) => break,
                                Either::First(Either4::Second(_)) if active_screen == 2 => break,
                                Either::First(Either4::Third(_)) if active_screen == 3 => break,
                                Either::First(Either4::Fourth(_)) => break,
                                Either::Second(_) if active_screen == 1 => break,
                                _ => {}
                            }
                        }
                    },
                )
                .await,
                Either::First(_)
            );

            led_red.set_low();
            Timer::after_millis(50).await;
            led_red.set_high();

            if update_completed {
                // Update finished cleanly — wait for the next event before redrawing.
                // Signal-driven redraws only fire when the relevant screen is active.
                loop {
                    match select(
                        select4(
                            button_rcvr.changed(),
                            LORA_MSG_SIGNAL.wait(),
                            ADVERT_SIGNAL.wait(),
                            BLE_PAIRING_SIGNAL.wait(),
                        ),
                        MINUTE_TICK.wait(),
                    )
                    .await
                    {
                        Either::First(Either4::First(_)) => break,
                        Either::First(Either4::Second(_)) if active_screen == 2 => break,
                        Either::First(Either4::Third(_)) if active_screen == 3 => break,
                        Either::First(Either4::Fourth(_)) => break,
                        Either::Second(_) if active_screen == 1 => break,
                        _ => {}
                    }
                }
            }
            // If interrupted: loop back immediately; next reset() cleans up the controller.
        }
    };

    let run_lora = run_meshcore_listener(
        board!(p, lora_spi),
        board!(p, lora_sck).into(),
        board!(p, lora_mosi).into(),
        board!(p, lora_miso).into(),
        board!(p, lora_rst).into(),
        board!(p, lora_nss).into(),
        board!(p, lora_busy).into(),
        board!(p, lora_dio1).into(),
        board!(p, lora_rf_sw).into(),
        &identity,
    );

    embassy_futures::join::join4(main_loop, run_nfc, buttons, run_lora).await;
}

/// Pets the watchdog every 2 seconds. He's a good boy.
/// If this task stops running (executor starved), the 5-second WDT fires,
/// resets the chip, and panic-probe prints a defmt stack trace via RTT.
// TODO: Increase pet interval in production to save battery.
#[embassy_executor::task]
async fn pet_watchdog_task(mut handle: embassy_nrf::wdt::WatchdogHandle) {
    loop {
        handle.pet();
        Timer::after_secs(2).await;
    }
}

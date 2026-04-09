#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::pac::wdt::vals::{Halt as WdtHalt, Sleep as WdtSleep};
use embassy_nrf::pwm::SimplePwm;
use embassy_nrf::wdt::{Config as WdtConfig, Watchdog};
use embassy_time::Timer;
use hello_graphics::fw::battery::{self, battery_task, init as init_battery};
use hello_graphics::fw::button::BTN_WATCH;
use hello_graphics::fw::device_id;
use hello_graphics::fw::images::badgercorn::BADGERCORN_DATA;
use hello_graphics::{
    BLE_PAIRING_SIGNAL, DISPLAY_STATE, MINUTE_TICK,
    SCREEN_BADGERCORN, SCREEN_MAIN,
    board, draw_graphics, health_err, unix_now, with_health,
    fw::button::run_buttons,
    fw::buzzer::{Buzzer, buzzer_task},
    fw::epd::{EpdConfig152x152 as EpdConfig, EpdGfx, LutMode, init_epd},
    fw::led,
    fw::nfct::run_nfct,
};
use ssd1675::UpdateMode;
use ssd1675::graphics::Color;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

#[cfg(feature = "mesh")]
use hello_graphics::fw::mesh::{
    ble::{CompanionContext, init_ble, run_ble_peripheral},
    bonds::bond_task,
    contacts::ContactStore,
    kv, settings,
    meshcore::run_meshcore_listener,
};
#[cfg(feature = "mesh")]
use hello_graphics::{
    ADVERT_SIGNAL, LORA_MSG_SIGNAL, PM_SIGNAL,
    SCREEN_PM, SCREEN_CHANNEL, SCREEN_ADVERT,
};

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // ── Core hardware init ───────────────────────────────────────────────
    let mut config = embassy_nrf::config::Config::default();
    config.hfclk_source = HfclkSource::ExternalXtal;
    let p = embassy_nrf::init(config);

    embassy_nrf::pac::POWER.tasks_constlat().write_value(1);

    let mut wdt_config = WdtConfig::default();
    wdt_config.timeout_ticks = 5 * 32768;
    wdt_config.action_during_debug_halt = WdtHalt::PAUSE;
    wdt_config.action_during_sleep = WdtSleep::RUN;
    let (_wdt, [wdt_handle]) = Watchdog::try_new(p.WDT, wdt_config).expect("WDT init failed");
    spawner.must_spawn(pet_watchdog_task(wdt_handle));

    device_id::init();
    let [id0, id1] = device_id::get();
    defmt::info!("Device ID: {:02X}{:02X}", id0, id1);

    let _ps_sync = Output::new(board!(p, ps_sync), Level::Low, OutputDrive::Standard);

    // ── Mesh stack (KV, contacts, identity, BLE) ─────────────────────────
    // Must come before temperature/EPD because it consumes p.RNG, p.RTC0,
    // p.TIMER0, p.TEMP, and PPI channels.
    #[cfg(feature = "mesh")]
    let identity = {
        match kv::init(
            p.QSPI,
            board!(p, flash_sck), board!(p, flash_csn),
            board!(p, flash_io0), board!(p, flash_io1),
            board!(p, flash_io2), board!(p, flash_io3),
        ).await {
            Ok(()) => {}
            Err(id) => defmt::panic!(
                "QSPI flash not reachable (JEDEC ID: {:02X} {:02X} {:02X})",
                id[0], id[1], id[2],
            ),
        }
        spawner.must_spawn(bond_task());
        ContactStore::new().init().await;

        let identity = settings::load_or_create_identity().await;
        let ble_prng_seed = hello_graphics::fw::mesh::device_identity::trng_seed();

        static SDC_MEM: StaticCell<nrf_sdc::Mem<{ hello_graphics::fw::mesh::ble::SDC_MEM_SIZE }>> =
            StaticCell::new();
        let sdc = init_ble(
            &spawner,
            p.RTC0, p.TIMER0, p.TEMP,
            p.PPI_CH19, p.PPI_CH30, p.PPI_CH31,
            p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21,
            p.PPI_CH22, p.PPI_CH23, p.PPI_CH24, p.PPI_CH25,
            p.PPI_CH26, p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
            p.RNG,
            SDC_MEM.init(nrf_sdc::Mem::new()),
        );
        spawner.must_spawn(run_ble_peripheral(
            sdc, CompanionContext { pub_key: identity.pub_key }, ble_prng_seed,
        ));
        identity
    };

    // ── Temperature ──────────────────────────────────────────────────────
    let temp_celsius = hello_graphics::fw::temperature::read_and_cache().await;
    defmt::info!("Die temperature: {} °C", temp_celsius);

    // ── EPD display ──────────────────────────────────────────────────────
    static BLACK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static RED_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static WORK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    let mut display: EpdGfx<'_> = init_epd(
        board!(p, epd_spi),
        board!(p, epd_sck).into(), board!(p, epd_mosi).into(),
        board!(p, epd_busy).into(), board!(p, epd_reset).into(),
        board!(p, epd_dc).into(), board!(p, epd_csn).into(),
        EpdConfig::to_dimensions(),
        BLACK_BUF.init([0; EpdConfig::BUF_SIZE]),
        RED_BUF.init([0; EpdConfig::BUF_SIZE]),
        WORK_BUF.init([0; EpdConfig::BUF_SIZE]),
        Some(temp_celsius),
        LutMode::NoInvert,
    ).await.unwrap();
    defmt::info!("EPD initialized");

    // ── LEDs ─────────────────────────────────────────────────────────────
    let mut led_red = Output::new(board!(p, led_red), Level::High, OutputDrive::Standard);
    let mut led_green = Output::new(board!(p, led_green), Level::High, OutputDrive::Standard);
    let mut led_blue = Output::new(board!(p, led_blue), Level::High, OutputDrive::Standard);
    led_red.set_low(); led_green.set_low(); led_blue.set_low();
    Timer::after_millis(200).await;
    led_red.set_high(); led_green.set_high(); led_blue.set_high();
    Timer::after_millis(200).await;
    spawner.must_spawn(led::led_task(led_red, led_green, led_blue));

    // ── Buttons, NFC, buzzer, battery, clock ─────────────────────────────
    let mut button_rcvr = BTN_WATCH.receiver().unwrap();
    let buttons = run_buttons(
        Input::new(board!(p, btn_can), Pull::Up),
        Input::new(board!(p, btn_exe), Pull::Up),
        Input::new(board!(p, joy_up), Pull::Up),
        Input::new(board!(p, joy_down), Pull::Up),
        Input::new(board!(p, joy_left), Pull::Up),
        Input::new(board!(p, joy_right), Pull::Up),
        Input::new(board!(p, joy_fire), Pull::Up),
    );
    let run_nfc = run_nfct(p.NFCT);
    spawner.must_spawn(buzzer_task(Buzzer::new(
        SimplePwm::new_1ch(p.PWM0, board!(p, buzzer), &Default::default()),
    )));
    let battery_monitor = match init_battery(p.SAADC, board!(p, vbat), board!(p, vbat_rd)).await {
        Ok(m) => m,
        Err(e) => { defmt::error!("Battery init failed: {:?}", e); return; }
    };
    spawner.must_spawn(battery_task(battery_monitor));
    spawner.must_spawn(minute_tick_task());

    // ── Display loop + concurrent tasks ──────────────────────────────────
    defmt::info!("Entering main loop...");
    let main_loop = display_loop(&mut display, &mut button_rcvr);

    #[cfg(feature = "mesh")]
    {
        let run_lora = run_meshcore_listener(
            board!(p, lora_spi),
            board!(p, lora_sck).into(), board!(p, lora_mosi).into(),
            board!(p, lora_miso).into(), board!(p, lora_rst).into(),
            board!(p, lora_nss).into(), board!(p, lora_busy).into(),
            board!(p, lora_dio1).into(), board!(p, lora_rf_sw).into(),
            &identity,
        );
        embassy_futures::join::join4(main_loop, run_nfc, buttons, run_lora).await;
    }
    #[cfg(not(feature = "mesh"))]
    embassy_futures::join::join3(main_loop, run_nfc, buttons).await;
}

// ---------------------------------------------------------------------------
// Display loop
// ---------------------------------------------------------------------------

async fn display_loop(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<'_, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, u8, 2>,
) {
    use embassy_futures::select::{Either, select};

    loop {
        let _ = display.clear(Color::White);
        let active_screen = DISPLAY_STATE.lock(|f| f.borrow().active_screen());

        #[cfg(feature = "mesh")]
        if active_screen == SCREEN_PM {
            hello_graphics::PM_UNREAD.store(false, core::sync::atomic::Ordering::Relaxed);
            led::set_led(&led::LED_BLUE, led::LedState::Off);
        }

        if active_screen == SCREEN_BADGERCORN {
            display.blit(Some(BADGERCORN_DATA), None);
        } else {
            let health_str = with_health!(|f| f.to_string());
            let bat_str = battery::read_pct();
            if draw_graphics(display, &health_str, &bat_str).is_err() {
                health_err!(epd, "Failed to draw graphics");
            }
        }

        let update_completed = matches!(
            select(
                async {
                    let _ = display.reset().await;
                    let _ = display.update_bw(UpdateMode::Mode1).await;
                    let _ = display.deep_sleep().await;
                },
                wait_display_event(button_rcvr, active_screen),
            ).await,
            Either::First(_)
        );

        led::set_led(&led::LED_RED, led::LedState::BlinkOnce);

        if update_completed {
            wait_display_event(button_rcvr, active_screen).await;
        }
    }
}

/// Wait for a display-relevant event for the given screen.
async fn wait_display_event(
    button_rcvr: &mut embassy_sync::watch::Receiver<'_, embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex, u8, 2>,
    active_screen: u8,
) {
    use embassy_futures::select::{Either, Either3, select, select3};

    loop {
        #[cfg(feature = "mesh")]
        match select(
            select3(button_rcvr.changed(), BLE_PAIRING_SIGNAL.wait(), MINUTE_TICK.wait()),
            select3(PM_SIGNAL.wait(), LORA_MSG_SIGNAL.wait(), ADVERT_SIGNAL.wait()),
        ).await {
            Either::First(Either3::First(_))   => return,
            Either::First(Either3::Second(_))  => return,
            Either::First(Either3::Third(_))   if active_screen == SCREEN_MAIN    => return,
            Either::Second(Either3::First(_))  if active_screen == SCREEN_PM      => return,
            Either::Second(Either3::Second(_)) if active_screen == SCREEN_CHANNEL => return,
            Either::Second(Either3::Third(_))  if active_screen == SCREEN_ADVERT  => return,
            _ => {}
        }

        #[cfg(not(feature = "mesh"))]
        match select3(
            button_rcvr.changed(),
            BLE_PAIRING_SIGNAL.wait(),
            MINUTE_TICK.wait(),
        ).await {
            Either3::First(_)  => return,
            Either3::Second(_) => return,
            Either3::Third(_) if active_screen == SCREEN_MAIN => return,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn minute_tick_task() {
    loop {
        let secs = unix_now().map(|t| 60 - (t % 60) as u64).unwrap_or(60);
        Timer::after(embassy_time::Duration::from_secs(secs)).await;
        MINUTE_TICK.signal(());
    }
}

#[embassy_executor::task]
async fn pet_watchdog_task(mut handle: embassy_nrf::wdt::WatchdogHandle) {
    loop {
        handle.pet();
        Timer::after_secs(2).await;
    }
}

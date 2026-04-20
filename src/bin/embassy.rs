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
    BLE_PAIRING_SIGNAL, DISPLAY_STATE, MINUTE_TICK, SCREEN_BADGERCORN, SCREEN_MAIN, board,
    draw_graphics,
    fw::button::run_buttons,
    fw::buzzer::{Buzzer, buzzer_task},
    fw::epd::{EpdConfig152x152 as EpdConfig, EpdGfx, LutMode, init_epd},
    fw::led,
    fw::nfct::run_nfct,
    health_err, unix_now, with_health,
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
    kv,
    meshcore::run_meshcore_listener,
    settings,
};
#[cfg(feature = "mesh")]
use hello_graphics::{
    ADVERT_SIGNAL, LORA_MSG_SIGNAL, PM_SIGNAL, SCREEN_ADVERT, SCREEN_CHANNEL, SCREEN_PM,
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

    // ── External flash (shared between KV store and USB mass storage) ────
    match hello_graphics::fw::flash::init(
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
            id[2],
        ),
    }

    // ── FAT12 partition — auto-format if blank ─────────────────────────
    if let Err(e) = hello_graphics::fw::fat12::format_if_needed().await {
        defmt::warn!("FAT12 format check failed: {:?}", e);
    }

    // ── LEDs ─────────────────────────────────────────────────────────────
    // Hoisted above the mesh stack so the led_task is already running when
    // ContactStore::init wipes a legacy store; the wipe signals via the
    // existing fw::led::set_led / LED_GREEN atomics.
    let mut led_red = Output::new(board!(p, led_red), Level::High, OutputDrive::Standard);
    let mut led_green = Output::new(board!(p, led_green), Level::High, OutputDrive::Standard);
    let mut led_blue = Output::new(board!(p, led_blue), Level::High, OutputDrive::Standard);
    led_red.set_low();
    led_green.set_low();
    led_blue.set_low();
    Timer::after_millis(200).await;
    led_red.set_high();
    led_green.set_high();
    led_blue.set_high();
    Timer::after_millis(200).await;
    spawner.must_spawn(led::led_task(led_red, led_green, led_blue));

    // ── Mesh stack (KV, contacts, identity, BLE) ─────────────────────────
    // Must come before temperature/EPD because it consumes p.RNG, p.RTC0,
    // p.TIMER0, p.TEMP, and PPI channels.
    #[cfg(feature = "mesh")]
    let identity = {
        kv::init().await;
        spawner.must_spawn(bond_task());
        ContactStore::new().init().await;

        // Load persisted display/runtime settings (timezone, boost-RX) into
        // their in-RAM atomics SYNCHRONOUSLY here — before any task that
        // reads them starts rendering. Previously the load lived inside the
        // BLE task's init block, which races against the display task: on a
        // quick reboot the display could draw a frame using the static
        // default (TIMEZONE_OFFSET = 0 → UTC) before the BLE task had a
        // chance to load the persisted offset.
        hello_graphics::TIMEZONE_OFFSET.store(
            settings::get_timezone().await,
            core::sync::atomic::Ordering::Relaxed,
        );
        hello_graphics::BOOSTED_RX_GAIN.store(
            settings::get_boost_rx().await,
            core::sync::atomic::Ordering::Relaxed,
        );
        {
            let rp = settings::get_radio_params_or_default().await;
            use core::sync::atomic::Ordering::Relaxed;
            hello_graphics::LORA_FREQ_HZ.store(rp.freq_hz, Relaxed);
            hello_graphics::LORA_BW_HZ.store(rp.bw_hz, Relaxed);
            hello_graphics::LORA_SF.store(rp.sf, Relaxed);
            hello_graphics::LORA_CR.store(rp.cr, Relaxed);
            hello_graphics::LORA_TX_POWER.store(rp.tx_power, Relaxed);
            hello_graphics::LORA_CLIENT_REPEAT.store(rp.client_repeat, Relaxed);
        }
        {
            use core::sync::atomic::Ordering::Relaxed;
            if let Some(op) = settings::get_other_params().await {
                hello_graphics::ADVERT_LOC_POLICY
                    .store(op.advert_loc_policy != 0, Relaxed);
                // Clamp persisted value into the menu-exposed range (1 or 2).
                let ma = if op.multi_acks == 0 { 1 } else { op.multi_acks.min(2) };
                hello_graphics::MULTI_ACKS.store(ma, Relaxed);
                // Derived master-telemetry flag — "on" iff any mode is non-zero.
                let share = op.telemetry_mode_base != 0
                    || op.telemetry_mode_loc != 0
                    || op.telemetry_mode_env != 0;
                hello_graphics::TELEMETRY_SHARE.store(share, Relaxed);
            }
            hello_graphics::fw::mesh::PATH_HASH_MODE
                .store(settings::get_path_hash_mode().await.min(2), Relaxed);

            let adv = settings::get_advert_config_or_default().await;
            hello_graphics::ADVERT_ENABLED.store(adv.enabled, Relaxed);
            hello_graphics::ADVERT_INTERVAL_HOURS.store(adv.interval_hours, Relaxed);

            hello_graphics::IGNORE_BLINK
                .store(settings::get_ignore_blink().await, Relaxed);
            hello_graphics::LORA_DISABLED
                .store(!settings::get_lora_enabled().await, Relaxed);
            hello_graphics::BLE_DISABLED
                .store(!settings::get_ble_enabled().await, Relaxed);
        }

        let identity = settings::load_or_create_identity().await;
        let ble_prng_seed = hello_graphics::fw::mesh::device_identity::trng_seed();

        static SDC_MEM: StaticCell<nrf_sdc::Mem<{ hello_graphics::fw::mesh::ble::SDC_MEM_SIZE }>> =
            StaticCell::new();
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
        spawner.must_spawn(run_ble_peripheral(
            sdc,
            CompanionContext {
                pub_key: identity.pub_key,
            },
            ble_prng_seed,
        ));
        identity
    };

    // ── Game engine + sprite loader ─────────────────────────────────────
    #[cfg(feature = "game")]
    {
        hello_graphics::game::lifecycle::init().await;
        hello_graphics::game::sprite_loader::init().await;
    }

    // ── Temperature ──────────────────────────────────────────────────────
    let temp_celsius = hello_graphics::fw::temperature::read_and_cache().await;
    defmt::info!("Die temperature: {} °C", temp_celsius);

    // ── EPD display ──────────────────────────────────────────────────────
    static BLACK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static RED_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    static WORK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    let mut display: EpdGfx<'_> = init_epd(
        board!(p, epd_spi),
        board!(p, epd_sck).into(),
        board!(p, epd_mosi).into(),
        board!(p, epd_busy).into(),
        board!(p, epd_reset).into(),
        board!(p, epd_dc).into(),
        board!(p, epd_csn).into(),
        EpdConfig::to_dimensions(),
        BLACK_BUF.init([0; EpdConfig::BUF_SIZE]),
        RED_BUF.init([0; EpdConfig::BUF_SIZE]),
        WORK_BUF.init([0; EpdConfig::BUF_SIZE]),
        Some(temp_celsius),
        LutMode::NoInvert,
    )
    .await
    .unwrap();
    defmt::info!("EPD initialized");

    // LEDs are initialised earlier (above the mesh stack) so the led_task is
    // already running when the contact store needs to blink the green LED
    // during a legacy-format wipe.

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
    spawner.must_spawn(buzzer_task(Buzzer::new(SimplePwm::new_1ch(
        p.PWM0,
        board!(p, buzzer),
        &Default::default(),
    ))));
    let battery_monitor = match init_battery(p.SAADC, board!(p, vbat), board!(p, vbat_rd).into(), board!(p, charge).into()).await {
        Ok(m) => m,
        Err(e) => {
            defmt::error!("Battery init failed: {:?}", e);
            return;
        }
    };
    spawner.must_spawn(battery_task(battery_monitor));
    spawner.must_spawn(minute_tick_task());
    #[cfg(feature = "mesh")]
    spawner.must_spawn(advert_ticker_task());

    // ── USB mass storage ──────────────────────────────────────────────────
    // Runs alongside all other tasks.  VBUS detection is automatic —
    // the USB PHY powers up when a cable is connected.
    #[cfg(feature = "usb-storage")]
    let run_usb = hello_graphics::fw::usb_storage::run(p.USBD);

    // ── Display loop + concurrent tasks ──────────────────────────────────
    defmt::info!("Entering main loop...");
    let main_loop = display_loop(&mut display, &mut button_rcvr);

    #[cfg(all(feature = "mesh", feature = "usb-storage"))]
    {
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
        embassy_futures::join::join5(main_loop, run_nfc, buttons, run_lora, run_usb).await;
    }
    #[cfg(all(feature = "mesh", not(feature = "usb-storage")))]
    {
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
    #[cfg(all(not(feature = "mesh"), feature = "usb-storage"))]
    embassy_futures::join::join4(main_loop, run_nfc, buttons, run_usb).await;
    #[cfg(all(not(feature = "mesh"), not(feature = "usb-storage")))]
    embassy_futures::join::join3(main_loop, run_nfc, buttons).await;
}

// ---------------------------------------------------------------------------
// Display loop
// ---------------------------------------------------------------------------

async fn display_loop(
    display: &mut EpdGfx<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) {
    use embassy_futures::select::{Either, select};

    #[cfg(feature = "game")]
    let mut sprite_frame: u8 = 0;

    loop {
        let _ = display.clear(Color::White);
        let active_screen = DISPLAY_STATE.lock(|f| f.borrow().active_screen());

        // ── Game cycle: update engine, render animation ────────────────
        #[cfg(feature = "game")]
        if active_screen == hello_graphics::SCREEN_GAME {
            hello_graphics::game::render(display, sprite_frame).await;
        }

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

        let sprite_advance = match select(
            async {
                // Re-read temperature and reload OTP LUT if drift > 2°C.
                // Must happen while display is in deep sleep (SPI3 idle).
                hello_graphics::fw::epd::maybe_reload_lut(display).await;

                let _ = display.reset().await;
                let _ = display.update_bw(UpdateMode::Mode1).await;
                let _ = display.deep_sleep().await;
            },
            wait_display_event(button_rcvr, active_screen),
        )
        .await
        {
            Either::First(_) => {
                // Update finished — wait for next event.
                wait_display_event(button_rcvr, active_screen).await
            }
            Either::Second(sprite) => sprite, // interrupted by event
        };

        led::set_led(&led::LED_RED, led::LedState::BlinkOnce);

        // Advance animation frame only when the sprite timer fired.
        #[cfg(feature = "game")]
        if sprite_advance {
            let anim = hello_graphics::game::lifecycle::display_anim();
            let kind = hello_graphics::game::lifecycle::pet_kind();
            let count = hello_graphics::game::engine::anim_files::frame_count(kind, anim);
            if count > 0 {
                let next = sprite_frame + 1;
                // During hatching, clamp to the last frame instead of wrapping.
                let is_hatching = matches!(anim, hello_graphics::game::engine::DisplayAnim::Hatching { .. });
                sprite_frame = if is_hatching {
                    next.min(count - 1)
                } else {
                    next % count
                };
            }
        }
        #[cfg(not(feature = "game"))]
        let _ = sprite_advance;
    }
}

/// Wait for a display-relevant event for the given screen.
///
/// Returns `true` if the sprite animation timer fired (caller should
/// advance the frame), `false` for all other events.
async fn wait_display_event(
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
    active_screen: u8,
) -> bool {
    use embassy_futures::select::{Either, Either3, select, select3};

    #[cfg(feature = "game")]
    let sprite_active = active_screen == hello_graphics::SCREEN_GAME
        && (hello_graphics::game::sprite_loader::frame_count() > 0
            || hello_graphics::game::lifecycle::is_started());

    loop {
        #[cfg(feature = "game")]
        let sprite_tick = async {
            if sprite_active {
                // Compute frame interval from the current animation.
                // Hatching: spread frames over the full countdown.
                // Everything else: 10 seconds per frame (EPD is slow with red).
                let anim = hello_graphics::game::lifecycle::display_anim();
                let kind = hello_graphics::game::lifecycle::pet_kind();
                let frame_count = hello_graphics::game::engine::anim_files::frame_count(kind, anim);
                let interval_secs = match anim {
                    hello_graphics::game::engine::DisplayAnim::Hatching { ticks_remaining } => {
                        let total_secs = ticks_remaining as u64 * 10;
                        if frame_count > 0 { total_secs / frame_count as u64 } else { 10 }
                    }
                    _ => 10,
                };
                Timer::after_secs(interval_secs.max(1)).await;
            } else {
                core::future::pending::<()>().await;
            }
        };
        #[cfg(not(feature = "game"))]
        let sprite_tick = core::future::pending::<()>();

        #[cfg(feature = "mesh")]
        {
            use embassy_futures::select::Either4;
            use embassy_futures::select::select4;
            match select(
                select3(
                    button_rcvr.changed(),
                    BLE_PAIRING_SIGNAL.wait(),
                    MINUTE_TICK.wait(),
                ),
                select4(
                    sprite_tick,
                    PM_SIGNAL.wait(),
                    LORA_MSG_SIGNAL.wait(),
                    ADVERT_SIGNAL.wait(),
                ),
            )
            .await
            {
                Either::Second(Either4::First(_)) => return true, // sprite timer
                Either::First(Either3::First(_)) => return false,
                Either::First(Either3::Second(_)) => return false,
                Either::First(Either3::Third(_)) if active_screen == SCREEN_MAIN => return false,
                Either::Second(Either4::Second(_)) if active_screen == SCREEN_PM => return false,
                Either::Second(Either4::Third(_)) if active_screen == SCREEN_CHANNEL => {
                    return false;
                }
                Either::Second(Either4::Fourth(_)) if active_screen == SCREEN_ADVERT => {
                    return false;
                }
                _ => {}
            }
        }

        #[cfg(not(feature = "mesh"))]
        match select(
            select3(
                button_rcvr.changed(),
                BLE_PAIRING_SIGNAL.wait(),
                MINUTE_TICK.wait(),
            ),
            sprite_tick,
        )
        .await
        {
            Either::Second(_) => return true, // sprite timer
            Either::First(Either3::First(_)) => return false,
            Either::First(Either3::Second(_)) => return false,
            Either::First(Either3::Third(_)) if active_screen == SCREEN_MAIN => return false,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------

/// Periodic self-advert task.
///
/// Wakes every `ADVERT_INTERVAL_HOURS` hours and pushes `TxRequest::Advert`
/// when `ADVERT_ENABLED` is true. When scheduling changes via the menu, the
/// task wakes early on `ADVERT_CHANGED_SIGNAL` and re-reads the interval.
/// When disabled it waits on the change signal and never sends.
#[cfg(feature = "mesh")]
#[embassy_executor::task]
async fn advert_ticker_task() {
    use core::sync::atomic::Ordering::Relaxed;
    use embassy_futures::select::{Either, select};

    // Brief delay on boot so the radio and mesh stack are up before our first TX.
    Timer::after(embassy_time::Duration::from_secs(30)).await;

    loop {
        if !hello_graphics::ADVERT_ENABLED.load(Relaxed) {
            hello_graphics::ADVERT_CHANGED_SIGNAL.wait().await;
            hello_graphics::ADVERT_CHANGED_SIGNAL.reset();
            continue;
        }

        // Send an advert now, then sleep until the next tick (or wake early
        // if the menu changes the schedule).
        let _ = hello_graphics::fw::mesh::tx_send(
            hello_graphics::fw::mesh::TxRequest::Advert(
                hello_graphics::fw::mesh::meshcore::AdvertMode::Flood,
            ),
        );

        let hours = hello_graphics::ADVERT_INTERVAL_HOURS
            .load(Relaxed)
            .clamp(2, 96) as u64;
        let sleep = embassy_time::Duration::from_secs(hours * 3600);

        match select(
            Timer::after(sleep),
            hello_graphics::ADVERT_CHANGED_SIGNAL.wait(),
        )
        .await
        {
            Either::First(_) => {}
            Either::Second(_) => {
                hello_graphics::ADVERT_CHANGED_SIGNAL.reset();
            }
        }
    }
}

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

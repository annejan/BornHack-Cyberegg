#![no_std]
#![no_main]

use bornhack_aegg::fw::battery::{self, battery_task, init as init_battery};
use bornhack_aegg::fw::button::{BTN_WATCH, run_buttons};
use bornhack_aegg::fw::buzzer::{Buzzer, buzzer_task};
use bornhack_aegg::fw::epd::{EpdConfig152x152 as EpdConfig, EpdGfx, init_epd};
#[cfg(feature = "mesh")]
use bornhack_aegg::fw::mesh::{
    ble::{CompanionContext, init_ble, run_ble_peripheral},
    bonds::bond_task,
    channels,
    contacts::ContactStore,
    meshcore::run_meshcore_listener,
    persister, settings,
};
use bornhack_aegg::fw::nfct::run_nfct;
use bornhack_aegg::fw::{device_id, kv, led};
#[cfg(feature = "mesh")]
use bornhack_aegg::{
    ADVERT_SIGNAL, LORA_MSG_SIGNAL, PM_SIGNAL, SCREEN_ADVERT, SCREEN_CHANNEL, SCREEN_PM,
};
use bornhack_aegg::{
    BLE_PAIRING_SIGNAL, DISPLAY_STATE, MINUTE_TICK, SCREEN_MAIN, SCREEN_TOKEN, SCREEN_WATCH, board,
    draw_graphics, health_err, unix_now, with_health,
};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::pac::wdt::vals::{Halt as WdtHalt, Sleep as WdtSleep};
use embassy_nrf::pwm::SimplePwm;
use embassy_nrf::wdt::{Config as WdtConfig, Watchdog};
use embassy_time::Timer;
use panic_probe as _;
use ssd1675::UpdateMode;
use ssd1675::graphics::Color;
use static_cell::StaticCell;

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // ── Core hardware init ───────────────────────────────────────────────
    // Phase 3 boot-supervisor pattern: always-boots architecture.
    //
    // We init on HFINT (internal 64 MHz RC) so this call can never hang
    // waiting for the 32 MHz crystal — a dead or intermittent HFXO won't
    // brick the badge.  `factory_test::probe()` below will actively
    // request HFXO with a short timeout; if it starts the chip switches
    // over (giving USB + BLE their precise clock), and if it doesn't we
    // stay on HFINT and gate the HFXO-dependent task spawns accordingly.
    let mut config = embassy_nrf::config::Config::default();
    config.hfclk_source = HfclkSource::Internal;
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
    match bornhack_aegg::fw::flash::init(
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
    if let Err(e) = bornhack_aegg::fw::fat12::format_if_needed().await {
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

    // Boot breadcrumb #1 — ORANGE 50% duty (red + green together).  Orange,
    // not red, so the boot blink isn't confused with DFU mode's red blink.
    // Keeps a steady "alive, booting" pulse while the rest of init runs (KV,
    // watch, sprites, mesh, EPD).  Both colours share the same Duty50 phase so
    // they're on/off in lockstep → orange.  Cleared at EPD init below.
    led::set_led(&led::LED_RED, led::LedState::Duty50);
    led::set_led(&led::LED_GREEN, led::LedState::Duty50);

    // ── Signed-channel CSPRNG seed ───────────────────────────────────────
    // Draw 32 bytes from the on-chip TRNG via direct register access
    // before BLE init takes ownership of `p.RNG`.  After this point the
    // signed-channel CSPRNG produces fresh challenges without needing
    // the hardware peripheral again.
    bornhack_aegg::signed_channel::Csprng::seed_from_hardware();

    // ── KV store ─────────────────────────────────────────────────────────
    // Persistent key-value store used by the game (save/load), sponsor
    // slideshow flag, and the mesh stack.  Must be initialised before any
    // task reads or writes flash-backed state.
    kv::init().await;

    // ── Hardware probe (boot supervisor) ─────────────────────────────────
    // Always runs.  Active probe of HFXO + LFXO — see
    // `bornhack_aegg::fw::factory_test` module docs for the design.
    // The returned `HardwareInfo` drives conditional task spawning
    // below (BLE + USB MSC need HFXO; everything else tolerates HFINT).
    let hw = bornhack_aegg::fw::factory_test::probe().await;

    // ── EPD display ──────────────────────────────────────────────────────
    // Hoisted up here (Phase 4) so the first-boot interactive test path
    // can paint per-test status on the e-paper.  E-ink retains its last
    // state without power: the test-name-before-test / PASS-after-test
    // pattern means a hang on any peripheral leaves the screen showing
    // every passed test plus the broken-step name *without* its PASS,
    // pinpointing the failure for factory-floor triage with zero error
    // handling.
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
    )
    .await
    .unwrap();
    defmt::info!("EPD initialized");
    bornhack_aegg::fw::epd::load_persisted_lut_speed().await;
    bornhack_aegg::fw::epd::load_persisted_temp_bias().await;

    // Host-side partial-refresh state — lazily allocates ~46 KB
    // .bss buffers (shadow + pending + sent_pending + dirty + 2
    // plane scratches).  Initialised to all-White to match the
    // post-boot panel-clear refresh below.
    let dims = EpdConfig::to_dimensions();
    let mut partial_state = bornhack_aegg::fw::epd::partial_state_take(dims.rows, dims.cols);
    partial_state.clear_to(ssd1675::graphics::Color::White);

    // Boot breadcrumb #2 — switch from orange to blue while the boot
    // tri-color refresh + remaining task spawns finish.  This is the
    // longest dark phase in the original boot, ~13s, so a colour change
    // here gives users a "no, it's not stuck" signal partway through.
    led::set_led(&led::LED_RED, led::LedState::Off);
    led::set_led(&led::LED_GREEN, led::LedState::Off);
    led::set_led(&led::LED_BLUE, led::LedState::Duty50);

    // Boot-time full blank: clear both planes to white and push with the
    // tri-color waveform so red ink particles get cycled too.  Wipes any
    // residual ghosting from the previous power-on session before the
    // first fast-LUT refresh paints over it.
    // Temperature BEFORE the first refresh: the boot clear (and every refresh
    // after) must pick the panel's real LUT band, not the 20 °C default — a
    // cold default over-drives a warm panel.  Safe to read TEMP directly here:
    // MPSL / SoftDevice isn't up yet, so it owns nothing.
    let _ = bornhack_aegg::fw::temperature::read_and_cache().await;
    let panel_c10 = bornhack_aegg::fw::epd::panel_temp_c10(display.variant());
    if panel_c10 != i16::MIN {
        display.set_active_temperature(panel_c10);
    }

    display.clear(Color::White);
    let _ = display.reset().await;
    let _ = display
        .update_tc(bornhack_aegg::fw::epd::current_lut_speed())
        .await;

    // USB mass storage — spawn BEFORE the first-boot interactive
    // gate so the factory-floor "one plug-cycle per badge" workflow
    // works.  The factory test halts (NEEDS REWORK on fail, ship
    // image on pass) using `Timer::after_secs(..).await`, which
    // yields back to the executor — so this MSC task keeps running
    // through the halt and the host can mount the FAT12 partition
    // while the badge is still on the test screen.  Host-side
    // `scripts/copy_assets.py` then copies the asset bundle and
    // unmounts in-place; ship image stays on the e-ink for packing.
    //
    // USB peripheral derives its 48 MHz clock from HFXO via the PLL
    // — skip the spawn cleanly on a degraded boot rather than
    // letting the USB stack hang.
    #[cfg(feature = "usb-storage")]
    if hw.hfxo_ok {
        spawner.must_spawn(bornhack_aegg::fw::usb_storage::usb_storage_task(p.USBD));
    } else {
        defmt::warn!("USB mass storage disabled this boot — HFXO unavailable");
    }

    // First-boot interactive sign-off path — only on a virgin badge.
    // Paints test status on `display` via the write-name-then-test
    // pattern so a hang leaves a forensic record on the e-paper.
    if !bornhack_aegg::fw::factory_test::is_passed().await {
        bornhack_aegg::fw::factory_test::run_first_boot_interactive(&hw, &mut display).await;
    }

    let _ = display.deep_sleep().await;

    // ── Watch app — load persisted alarm state and start the persister ───
    #[cfg(feature = "watch")]
    {
        bornhack_aegg::watch::load_settings_from_kv().await;
        // If `ALARMS.ICS` is present on the FAT12 partition, populate slots
        // 1..N_ALARMS with one-shot calendar alarms.  Runs *after* the kv
        // load so the user's primary alarm (slot 0) isn't overwritten.
        bornhack_aegg::watch::import_alarms_from_fat12().await;
        spawner.must_spawn(bornhack_aegg::watch::settings_persister_task());
        spawner.must_spawn(bornhack_aegg::watch::alarm_ring_timeout_task());
    }

    // ── BornPets balance — install the active threshold set before any
    //    game tick runs.  Mode comes from the KV `game.mode` setting
    //    (defaults to Classic on a fresh badge); `BORNPETS.CFG` on the
    //    USB-MSC partition, if present, overrides individual fields on
    //    top of the preset.
    #[cfg(feature = "game")]
    {
        let mode = bornhack_aegg::game::settings::load_mode_from_kv().await;
        let _ = bornhack_aegg::fw::bornpets_cfg::load_and_install(mode).await;
        // Register custom pets from PETS.CFG (if any) before the game reads
        // the roster.  No file → built-in roster only.
        bornhack_aegg::fw::pets_cfg::load_and_install().await;
        spawner.must_spawn(bornhack_aegg::game::settings::persister_task());
    }

    // ── Mesh stack (contacts, identity, BLE) ─────────────────────────────
    // Must come before temperature/EPD because it consumes p.RNG, p.RTC0,
    // p.TIMER0, p.TEMP, and PPI channels.
    #[cfg(feature = "mesh")]
    let identity = {
        spawner.must_spawn(bond_task());
        spawner.must_spawn(persister::run());
        spawner.must_spawn(bornhack_aegg::fw::mesh::contacts_screen::refresh_cache_task());
        spawner.must_spawn(bornhack_aegg::fw::mesh::contacts_screen::mutation_persister_task());
        ContactStore::new().init().await;

        // Seed channel KV slots before the meshcore listener or BLE task
        // load them.  Previously this lived inside `run_ble_peripheral`
        // after a spin-wait on `INITIAL_BONDS`, which let the meshcore
        // listener load an empty channel set on a fresh-flash boot.
        channels::init().await;
        defmt::info!(
            "channels: store ready ({} active)",
            channels::count_active().await
        );

        // Load persisted display/runtime settings (timezone, boost-RX) into
        // their in-RAM atomics SYNCHRONOUSLY here — before any task that
        // reads them starts rendering. Previously the load lived inside the
        // BLE task's init block, which races against the display task: on a
        // quick reboot the display could draw a frame using the static
        // default (TIMEZONE_OFFSET = 0 → UTC) before the BLE task had a
        // chance to load the persisted offset.
        bornhack_aegg::TIMEZONE_OFFSET.store(
            settings::get_timezone().await,
            core::sync::atomic::Ordering::Relaxed,
        );
        bornhack_aegg::BOOSTED_RX_GAIN.store(
            settings::get_boost_rx().await,
            core::sync::atomic::Ordering::Relaxed,
        );

        // Flood-scope key: load the MeshCore-1.15 `def_scope` slot
        // (CMD_SET_DEFAULT_FLOOD_SCOPE / 0x3F) if the phone has set one,
        // else derive the dk-bornhack key inline so badges ship
        // region-scoped to BornHack repeaters out of the box — no flash
        // write at first boot.  `flood_route()` wraps every outbound
        // flood packet in this key.
        {
            let scope = match settings::get_default_flood_scope().await {
                Some(s) => Some(s.key),
                None => Some(settings::dk_bornhack_default_scope()),
            };
            bornhack_aegg::fw::mesh::FLOOD_SCOPE_KEY.lock(|c| c.set(scope));
        }

        {
            let rp = settings::get_radio_params_or_default().await;
            use core::sync::atomic::Ordering::Relaxed;
            bornhack_aegg::LORA_FREQ_HZ.store(rp.freq_hz, Relaxed);
            bornhack_aegg::LORA_BW_HZ.store(rp.bw_hz, Relaxed);
            bornhack_aegg::LORA_SF.store(rp.sf, Relaxed);
            bornhack_aegg::LORA_CR.store(rp.cr, Relaxed);
            bornhack_aegg::LORA_TX_POWER.store(rp.tx_power, Relaxed);
            bornhack_aegg::LORA_CLIENT_REPEAT.store(rp.client_repeat, Relaxed);
        }
        {
            use core::sync::atomic::Ordering::Relaxed;
            if let Some(op) = settings::get_other_params().await {
                bornhack_aegg::ADVERT_LOC_POLICY.store(op.advert_loc_policy != 0, Relaxed);
                // Clamp persisted value into the menu-exposed range (1 or 2).
                let ma = if op.multi_acks == 0 {
                    1
                } else {
                    op.multi_acks.min(2)
                };
                bornhack_aegg::MULTI_ACKS.store(ma, Relaxed);
                // Only telemetry_mode_base is exposed via the badge UI;
                // loc/env stay 0 (no GPS, no env sensors).
                bornhack_aegg::TELEMETRY_MODE_BASE.store(op.telemetry_mode_base.min(2), Relaxed);
            }
            bornhack_aegg::fw::mesh::PATH_HASH_MODE
                .store(settings::get_path_hash_mode().await.min(2), Relaxed);

            let adv = settings::get_advert_config_or_default().await;
            bornhack_aegg::ADVERT_ENABLED.store(adv.enabled, Relaxed);
            bornhack_aegg::ADVERT_INTERVAL_HOURS.store(adv.interval_hours, Relaxed);

            bornhack_aegg::IGNORE_BLINK.store(settings::get_ignore_blink().await, Relaxed);
            bornhack_aegg::LORA_DISABLED.store(!settings::get_lora_enabled().await, Relaxed);
            bornhack_aegg::BLE_DISABLED.store(!settings::get_ble_enabled().await, Relaxed);
        }

        let identity = settings::load_or_create_identity().await;
        // Cache the public key for the "My QR" screen — it builds a
        // meshcore://contact/add?... URL on demand and needs the 32-byte
        // pubkey to render.  Identity is otherwise consumed by the
        // mesh task and not accessible from the draw thread.
        bornhack_aegg::MY_PUB_KEY.lock(|cell| {
            cell.borrow_mut().copy_from_slice(&identity.pub_key);
        });

        // BLE companion needs HFXO (the SoftDevice Controller's radio
        // demands 32 MHz crystal accuracy).  Spawn it only when our
        // probe found a working crystal; otherwise log + skip and the
        // rest of the firmware (LoRa, watch, game, display) carries on.
        if hw.hfxo_ok {
            let ble_prng_seed = bornhack_aegg::fw::mesh::device_identity::trng_seed();
            static SDC_MEM: StaticCell<
                nrf_sdc::Mem<{ bornhack_aegg::fw::mesh::ble::SDC_MEM_SIZE }>,
            > = StaticCell::new();
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
        } else {
            defmt::warn!(
                "BLE companion disabled this boot — HFXO unavailable, \
                 LoRa mesh continues on HFINT"
            );
        }
        identity
    };

    // ── Game engine + sprite loader ─────────────────────────────────────
    #[cfg(feature = "game")]
    {
        bornhack_aegg::game::lifecycle::init().await;
        bornhack_aegg::game::sprite_loader::init().await;
    }

    // ── Temperature ──────────────────────────────────────────────────────
    let temp_celsius = bornhack_aegg::fw::temperature::read_and_cache().await;
    defmt::info!("Die temperature: {} °C", temp_celsius);

    // (EPD display was initialised earlier — see the "Hardware probe"
    // section.  The display is already past its boot-time blank and
    // is in deep_sleep until the display task takes over.)

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
    let battery_monitor = match init_battery(
        p.SAADC,
        board!(p, vbat),
        board!(p, vbat_rd).into(),
        board!(p, charge).into(),
    )
    .await
    {
        Ok(m) => m,
        Err(e) => {
            defmt::error!("Battery init failed: {:?}", e);
            show_battery_critical(&mut display, &e).await;
            return;
        }
    };
    spawner.must_spawn(battery_task(battery_monitor));
    spawner.must_spawn(minute_tick_task());
    #[cfg(feature = "mesh")]
    spawner.must_spawn(advert_ticker_task());

    // ── USB mass storage ──────────────────────────────────────────────────
    // Spawn BEFORE the sponsor slideshow so the host can mount the FAT
    // (USB mass storage is now spawned much earlier — see the block
    // right after EPD init so the factory-test halt can co-exist
    // with host-side asset copy in a single plug-cycle.)

    // ── Boot-complete chime ───────────────────────────────────────────────
    // Plays once on every boot (first boot included) when the user
    // hasn't disabled it via Settings → Boot chime.  The audible
    // signal that init has finished and the badge is ready.  Fires
    // before the first-boot sponsor slideshow so the sound and the
    // slideshow don't compete for attention, and so the chime always
    // plays at the same wall-clock moment in the boot sequence
    // regardless of whether the slideshow is going to run.
    if bornhack_aegg::BOOT_CHIME_ENABLED.load(core::sync::atomic::Ordering::Relaxed) {
        bornhack_aegg::fw::buzzer::play(bornhack_aegg::SONG_STARTUP_INDEX as usize);
    }

    // ── First-boot sponsor slideshow ────────────────────────────────────
    if !bornhack_aegg::fw::sponsors::already_shown().await {
        defmt::info!("Running first-boot sponsor slideshow");
        bornhack_aegg::fw::sponsors::run(&mut display, &mut button_rcvr).await;
    }

    // ── Display loop + concurrent tasks ──────────────────────────────────
    defmt::info!("Entering main loop...");

    // Boot breadcrumb #3 — single green pulse, then idle.  Tells the
    // user the badge is past init and the screen carousel is live.
    // `Duty50Once` auto-resets to Off after one 500 ms pulse.
    led::set_led(&led::LED_BLUE, led::LedState::Off);
    led::set_led(&led::LED_GREEN, led::LedState::Duty50Once);
    let main_loop = display_loop(&mut display, &mut button_rcvr, &mut partial_state);

    // USB mass storage is a separately-spawned task (see above), so it's
    // not in these joins.
    #[cfg(feature = "mesh")]
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
    #[cfg(not(feature = "mesh"))]
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
    partial_state: &mut ssd1675::partial::PartialState,
) {
    use embassy_futures::select::{Either, select};

    #[cfg(feature = "game")]
    let mut sprite_frame: u8 = 0;
    // Last animation id observed by the sprite-frame advance.  When
    // it changes, `sprite_frame` resets to 0 so each new animation
    // starts at its first frame regardless of where the previous one
    // left the counter.
    #[cfg(feature = "game")]
    let mut last_anim_id: u8 = 0xFF;

    // Track the previously-rendered screen so we can promote the
    // first refresh after a screen switch to a full panel-clearing
    // drive — clears ghosting from the outgoing screen.  0xFF =
    // sentinel for "no prior screen" (boot path already does its
    // own clears).
    let mut last_screen: u8 = 0xFF;

    loop {
        // Process any pending sponsor flag clear request from the menu.
        bornhack_aegg::fw::sponsors::process_clear_request().await;

        display.clear(Color::White);
        let active_screen = DISPLAY_STATE.lock(|f| f.borrow().active_screen());

        // ── Game cycle: update engine, render animation ────────────────
        // Reset the sprite-frame counter on animation change *before*
        // the render so the new anim shows frame 0 immediately rather
        // than waiting for the next sprite-tick fire.
        #[cfg(feature = "game")]
        if active_screen == bornhack_aegg::SCREEN_GAME {
            let anim = bornhack_aegg::game::lifecycle::display_anim();
            let id = bornhack_aegg::game::engine::anim_files::anim_id_for(anim);
            if id != last_anim_id {
                last_anim_id = id;
                sprite_frame = 0;
            }
            bornhack_aegg::game::render(display, sprite_frame).await;
        }

        #[cfg(feature = "mesh")]
        if active_screen == SCREEN_PM {
            bornhack_aegg::PM_UNREAD.store(false, core::sync::atomic::Ordering::Relaxed);
            led::set_led(&led::LED_BLUE, led::LedState::Off);
        }

        let health_str = with_health!(|f| f.to_string());
        let bat_str = battery::read_pct();
        if draw_graphics(display, &health_str, &bat_str).is_err() {
            health_err!(epd, "Failed to draw graphics");
        }

        // Screen switches and game menu open/close redraw the WHOLE frame
        // as a partial (mark every pixel dirty) rather than running a full
        // OTP refresh.  Mini-games / menus set `FULL_REFRESH_PENDING` on
        // open and close; we consume it with `swap` so the redraw applies
        // once.  The expensive full refresh is now promoted automatically
        // inside `update_partial` once the cumulative changed-pixel counter
        // reaches `full_after_screens` (5 screens) — see `should_force_full`.
        // First iteration after boot (last_screen == 0xFF) skips since boot
        // already drove a clean frame.
        let screen_changed = last_screen != 0xFF && last_screen != active_screen;
        let mark_all = bornhack_aegg::FULL_REFRESH_PENDING
            .swap(false, core::sync::atomic::Ordering::Relaxed)
            || screen_changed;

        // Feed the SSD1675's per-temperature LUT lookup from the nRF52840
        // die sensor (the chip itself has no on-die sensor — datasheet pg 6).
        // Bias is variant-aware — SSD1675A blooms without warm-band offset.
        bornhack_aegg::fw::temperature::refresh_if_stale();
        let panel_c10 = bornhack_aegg::fw::epd::panel_temp_c10(display.variant());
        if panel_c10 != i16::MIN {
            display.set_active_temperature(panel_c10);
        }

        // SSD1675A: every screen routes through the partial / delta
        // refresh path.  Bridge GraphicDisplay's drawing buffers into
        // the PartialState before each refresh so set_pixel marks
        // dirty wherever the new bitmap diverges from the shadow.
        // do_full re-routes through force_full_refresh so shadow
        // stays consistent with the panel state.
        // Delta refresh now supports both SSD1675 family variants —
        // partial.rs picks the right LUT layout (7-phase A vs 10-phase B).
        let use_partial_path = matches!(
            display.variant(),
            ssd1675::DisplayVariant::Ssd1675 | ssd1675::DisplayVariant::Ssd1675B,
        );
        if use_partial_path {
            let (black_buf, red_buf, _work) = display.all_buffers_mut();
            let black_snapshot = &*black_buf;
            let red_snapshot = &*red_buf;
            ssd1675::partial::sync_from_planes(partial_state, black_snapshot, red_snapshot);
            // Screen switch / menu open-close → redraw every pixel this
            // refresh (full-frame partial).  Done after sync so pending
            // already holds the new screen's content.
            if mark_all {
                partial_state.mark_all_dirty();
            }
        }

        // NoOp skip: when the partial path would have nothing to drive
        // (no dirty pixels) and no full-refresh is queued, bypass the
        // entire SPI ceremony (reset + LUT + plane upload + deep_sleep)
        // and just wait for the next event.  Without this, every
        // wakeup that doesn't change pixels (animation tick that hit
        // the same frame, mesh signal on the wrong screen, etc.) still
        // pays ~500 ms of dead time on the bus and re-blinks the
        // status LED — making the device look like it's continuously
        // refreshing even when nothing is changing.
        let partial_idle = use_partial_path && partial_state.bbox_of_dirty().is_none();

        let sprite_advance = if partial_idle {
            last_screen = active_screen;
            wait_display_event(button_rcvr, active_screen, true).await
        } else {
            match select(
                async {
                    let _ = display.reset().await;
                    let speed = bornhack_aegg::fw::epd::current_lut_speed();
                    if use_partial_path {
                        // Delta path for both A and B, driven by each panel's
                        // OWN probed OTP.  Normal refreshes use the no-invert
                        // patched LUT (select_no_invert) → non-flashing; the
                        // path auto-promotes to a full (flashing) OTP refresh
                        // once the changed-pixel counter trips, to de-ghost.
                        //
                        // Exception: the start screen is a heavy full-frame
                        // graphic (large solid-black + a red accent).  Laid down
                        // via the non-inverting delta LUT it under-drives and
                        // ghosts badly into the next screen (e.g. the sponsor
                        // slideshow).  While it's shown, force a genuine
                        // inverting full refresh so it's fully driven and clears
                        // cleanly.  The `partial_idle` short-circuit above
                        // already skips redundant redraws, so this only fires on
                        // an actual content change — no repeated flashing.
                        #[cfg(feature = "game")]
                        let start_screen = !bornhack_aegg::game::lifecycle::is_started();
                        #[cfg(not(feature = "game"))]
                        let start_screen = false;
                        if start_screen {
                            let _ = display.force_full_refresh(partial_state, speed).await;
                        } else {
                            let _ = display.update_partial(partial_state, speed).await;
                        }
                    } else {
                        let _ = display.update_bw(UpdateMode::Mode1, speed).await;
                    }
                    let _ = display.deep_sleep().await;
                },
                // `allow_sprite = false`: the animation frame timer must NOT
                // cancel an in-flight refresh — only buttons/signals do (so
                // menus stay responsive).  A slow red/full sweep therefore
                // plays to completion instead of being interrupted mid-wave,
                // which is what caused the ghosting.
                wait_display_event(button_rcvr, active_screen, false),
            )
            .await
            {
                Either::First(_) => {
                    // Refresh finished uninterrupted.  Commit the screen-
                    // transition tracker, then wait the frame timer
                    // (`allow_sprite = true`).  The pet animation advances a
                    // frame only here — after BOTH the refresh completed AND
                    // the frame timer elapsed — so a frame never advances
                    // mid-sweep.  A button during this wait redraws without
                    // advancing.
                    last_screen = active_screen;
                    wait_display_event(button_rcvr, active_screen, true).await
                }
                Either::Second(_) => false, // button/signal interrupted the refresh — redraw, no advance
            }
        };

        led::set_led(&led::LED_RED, led::LedState::BlinkOnce);

        // Advance animation frame only when the sprite timer fired.
        // Anim-change detection lives just before `render()` above so
        // the reset is visible on the same frame as the change.
        #[cfg(feature = "game")]
        if sprite_advance {
            let anim = bornhack_aegg::game::lifecycle::display_anim();
            let kind = bornhack_aegg::game::lifecycle::pet_kind();
            let count = bornhack_aegg::game::engine::anim_files::frame_count(kind, anim);
            if count > 0 {
                let next = sprite_frame + 1;
                // During hatching, clamp to the last frame instead of wrapping.
                let is_hatching = matches!(
                    anim,
                    bornhack_aegg::game::engine::DisplayAnim::Hatching { .. }
                );
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
    // When false the sprite frame timer is suppressed (treated as `pending`)
    // so this waiter only returns on real interrupts (buttons/signals). Used
    // while a refresh is in flight so the frame timer can't cancel it.
    allow_sprite: bool,
) -> bool {
    use embassy_futures::select::{Either, Either3, select, select3};

    #[cfg(feature = "game")]
    let sprite_active = allow_sprite
        && active_screen == bornhack_aegg::SCREEN_GAME
        && (bornhack_aegg::game::sprite_loader::frame_count() > 0
            || bornhack_aegg::game::lifecycle::is_started());
    #[cfg(not(feature = "game"))]
    let _ = allow_sprite;

    loop {
        #[cfg(feature = "game")]
        let sprite_tick = async {
            if sprite_active {
                // Compute frame interval from the current animation.
                // Timed animations (hatching, actions) spread frames evenly
                // over their duration so every frame is shown.
                // Idle/warning/etc: 10 seconds per frame.
                let anim = bornhack_aegg::game::lifecycle::display_anim();
                let kind = bornhack_aegg::game::lifecycle::pet_kind();
                let frame_count =
                    bornhack_aegg::game::engine::anim_files::frame_count(kind, anim) as u64;
                if frame_count <= 1 {
                    // Single frame — no animation to advance; sleep until
                    // the game state changes (next wake tick).
                    let wake = bornhack_aegg::game::lifecycle::next_wake_tick();
                    let now = bornhack_aegg::game::lifecycle::now_tick();
                    let wait_ticks = wake.saturating_sub(now).max(1) as u64;
                    Timer::after_secs(wait_ticks * 10).await;
                } else {
                    let interval_secs = match anim {
                        bornhack_aegg::game::engine::DisplayAnim::Hatching { ticks_remaining } => {
                            let total_secs = ticks_remaining as u64 * 10;
                            total_secs / frame_count
                        }
                        bornhack_aegg::game::engine::DisplayAnim::Feeding
                        | bornhack_aegg::game::engine::DisplayAnim::Healing
                        | bornhack_aegg::game::engine::DisplayAnim::Relaxing
                        | bornhack_aegg::game::engine::DisplayAnim::Playing => {
                            // Spread frames over the remaining action time.
                            let stats = bornhack_aegg::game::lifecycle::cycle();
                            let remaining_ticks =
                                stats.map_or(0, |s| s.action_ticks_remaining as u64);
                            let total_secs = remaining_ticks * 10;
                            total_secs / frame_count
                        }
                        _ => 3,
                    };
                    // Minimum pacing between frames. The refresh itself now
                    // guarantees the full waveform played (the frame timer no
                    // longer cancels an in-flight refresh — see the display
                    // loop), so this only sets idle cadence, not anti-ghost
                    // timing. Floor 2 s.
                    Timer::after_secs(interval_secs.max(2)).await;
                }
            } else {
                core::future::pending::<()>().await;
            }
        };
        #[cfg(not(feature = "game"))]
        let sprite_tick = core::future::pending::<()>();

        // Compose button + TOAST_SIGNAL + TOKEN_SIGNAL into a single
        // wakeup so the outer select shape stays unchanged.  All three
        // result in a non-sprite (`return false`) wake-up.  TOKEN_SIGNAL
        // fires when a new token value arrives over MeshCore or NFC —
        // joining it here lets the token screen redraw immediately
        // rather than waiting for the next minute tick.
        let button_or_toast = async {
            use embassy_futures::select::{Either3, select3};
            match select3(
                button_rcvr.changed(),
                bornhack_aegg::TOAST_SIGNAL.wait(),
                bornhack_aegg::TOKEN_SIGNAL.wait(),
            )
            .await
            {
                Either3::First(_) | Either3::Second(_) | Either3::Third(_) => {}
            }
        };

        #[cfg(feature = "mesh")]
        {
            use embassy_futures::select::{Either4, select4};
            match select(
                select3(
                    button_or_toast,
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
                Either::Second(Either4::First(_))  => return true,  // sprite timer
                Either::First(Either3::First(_))   => return false, // button / toast
                Either::First(Either3::Second(_))  => return false, // BLE pairing
                Either::First(Either3::Third(_))                    // minute tick
                    if active_screen == SCREEN_MAIN => return false,
                Either::First(Either3::Third(_))                    // minute tick
                    if active_screen == SCREEN_WATCH => return false,
                Either::First(Either3::Third(_))                    // minute tick
                    if active_screen == SCREEN_TOKEN => return false,
                // Calendar is intentionally left off the minute-tick
                // redraw list.  The fast-LUT refresh path doesn't update
                // the red plane (today highlight, event dots, day-view
                // "now" line all live there), so a per-minute wakeup
                // would re-render the B/W layer for no visible gain
                // while still costing an EPD update.  Calendar redraws
                // only on button input — the user navigating in or
                // pressing anything will pick up wall-clock changes.
                Either::Second(Either4::Second(_))                  // PM activity
                    if active_screen == SCREEN_PM => return false,
                Either::Second(Either4::Third(_))                   // LoRa msg
                    if active_screen == SCREEN_CHANNEL => return false,
                Either::Second(Either4::Fourth(_))                  // self-advert
                    if active_screen == SCREEN_ADVERT => return false,
                _ => {}
            }
        }

        #[cfg(not(feature = "mesh"))]
        match select(
            select3(button_or_toast, BLE_PAIRING_SIGNAL.wait(), MINUTE_TICK.wait()),
            sprite_tick,
        )
        .await
        {
            Either::Second(_)                  => return true,  // sprite timer
            Either::First(Either3::First(_))   => return false, // button / toast
            Either::First(Either3::Second(_))  => return false, // BLE pairing
            Either::First(Either3::Third(_))                    // minute tick
                if active_screen == SCREEN_MAIN => return false,
            Either::First(Either3::Third(_))                    // minute tick
                if active_screen == SCREEN_WATCH => return false,
            Either::First(Either3::Third(_))                    // minute tick
                if active_screen == SCREEN_TOKEN => return false,
            // Calendar deliberately ignores the minute tick — see the
            // matching arm in the mesh-feature branch above for why.
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
        if !bornhack_aegg::ADVERT_ENABLED.load(Relaxed) {
            bornhack_aegg::ADVERT_CHANGED_SIGNAL.wait().await;
            bornhack_aegg::ADVERT_CHANGED_SIGNAL.reset();
            continue;
        }

        // Send an advert now, then sleep until the next tick (or wake early
        // if the menu changes the schedule).
        let _ = bornhack_aegg::fw::mesh::tx_send(bornhack_aegg::fw::mesh::TxRequest::Advert(
            bornhack_aegg::fw::mesh::meshcore::AdvertMode::Flood,
        ));

        let hours = bornhack_aegg::ADVERT_INTERVAL_HOURS
            .load(Relaxed)
            .clamp(2, 96) as u64;
        let sleep = embassy_time::Duration::from_secs(hours * 3600);

        match select(
            Timer::after(sleep),
            bornhack_aegg::ADVERT_CHANGED_SIGNAL.wait(),
        )
        .await
        {
            Either::First(_) => {}
            Either::Second(_) => {
                bornhack_aegg::ADVERT_CHANGED_SIGNAL.reset();
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
        #[cfg(feature = "watch")]
        bornhack_aegg::watch::check_and_fire_alarm();
    }
}

#[embassy_executor::task]
async fn pet_watchdog_task(mut handle: embassy_nrf::wdt::WatchdogHandle) {
    loop {
        handle.pet();
        Timer::after_secs(2).await;
    }
}

/// Draw a "Battery voltage critical" screen on the EPD and commit it.
/// Called from the main battery-init error path before main() returns.
/// The EPD retains the image after deep_sleep, so the message stays visible
/// until the operator intervenes.
async fn show_battery_critical(display: &mut EpdGfx<'_>, err: &battery::BatteryError) {
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

    display.clear(Color::White);

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, Color::Black);

    let _ =
        Text::with_text_style("Battery voltage", Point::new(76, 50), font, centered).draw(display);
    let _ = Text::with_text_style("critical", Point::new(76, 66), font, centered).draw(display);

    let mut detail: heapless::String<32> = heapless::String::new();
    let _ = match err {
        battery::BatteryError::TooLow(mv) => {
            core::fmt::Write::write_fmt(&mut detail, format_args!("{} mV — too low", mv))
        }
        battery::BatteryError::TooHigh(mv) => {
            core::fmt::Write::write_fmt(&mut detail, format_args!("{} mV — too high", mv))
        }
    };
    let _ =
        Text::with_text_style(detail.as_str(), Point::new(76, 90), font, centered).draw(display);
    let _ =
        Text::with_text_style("Check / replace", Point::new(76, 114), font, centered).draw(display);
    let _ = Text::with_text_style("battery", Point::new(76, 130), font, centered).draw(display);

    let _ = display.reset().await;
    let _ = display
        .update_bw(
            UpdateMode::Mode1,
            bornhack_aegg::fw::epd::current_lut_speed(),
        )
        .await;
    let _ = display.deep_sleep().await;
}

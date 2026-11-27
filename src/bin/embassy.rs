#![no_std]
#![no_main]

use bornhack_aegg::fw::battery::{self, battery_task, init as init_battery};
use bornhack_aegg::fw::button::{BTN_WATCH, run_buttons};
use bornhack_aegg::fw::buzzer::{Buzzer, buzzer_task};
#[cfg(feature = "ssd1675-driver")]
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
    BLE_PAIRING_SIGNAL, DISPLAY_STATE, MINUTE_TICK, SCREEN_MAIN, SCREEN_WATCH, board,
    draw_graphics, health_err, unix_now, with_health,
};
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_nrf::config::HfclkSource;
use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
use embassy_nrf::pac::wdt::vals::{Halt as WdtHalt, Sleep as WdtSleep};
use embassy_nrf::pwm::SimplePwm;
use embassy_nrf::wdt::{Config as WdtConfig, Watchdog};
use embassy_time::{Duration, Timer};
use panic_probe as _;
#[cfg(feature = "ssd1675-driver")]
use ssd1675::UpdateMode;
#[cfg(feature = "ssd1675-driver")]
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

    // Buck/boost mode pin. Come up in boost (LOW = high power) and hold that
    // for all of boot init via a Boot vote; released just before the main loop
    // so the badge settles to power-save unless an EPD/LoRa event votes.
    let ps_sync = Output::new(board!(p, ps_sync), Level::Low, OutputDrive::Standard);
    bornhack_aegg::fw::power::init(ps_sync);
    let boot_boost = bornhack_aegg::fw::power::boost(bornhack_aegg::fw::power::Source::Boot);

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
    // ContactStore::init wipes a legacy store and lights the LED.  PWM1 mixes
    // the three channels (PWM0 is the buzzer).
    spawner.must_spawn(led::led_task(SimplePwm::new_3ch(
        p.PWM1,
        board!(p, led_red),
        board!(p, led_green),
        board!(p, led_blue),
        &led::pwm_config(),
    )));
    spawner.must_spawn(led::about_rainbow_task());
    led::set_color(led::Rgb::WHITE, Duration::from_millis(200));
    Timer::after_millis(400).await;

    // Boot breadcrumb #1 — steady orange while the rest of init runs (KV,
    // watch, sprites, mesh, EPD).  Orange, not red, so it isn't confused
    // with DFU mode's red blink.  Replaced at EPD init below.
    led::set_color(led::Rgb::ORANGE, led::FOREVER);

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
    // ── SSD1675 (shipping panel) display init ─────────────────────────────
    // Only compiled for the SSD1675 driver build. The SSD1680 build wires its
    // own SPI bus + planes further down and skips this LUT-patch / partial-
    // state / boot tri-colour setup entirely.
    #[cfg(feature = "ssd1675-driver")]
    static BLACK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    #[cfg(feature = "ssd1675-driver")]
    static RED_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    #[cfg(feature = "ssd1675-driver")]
    static WORK_BUF: StaticCell<[u8; EpdConfig::BUF_SIZE]> = StaticCell::new();
    // Boot-time escape hatch for a bad custom LUT: sample Fire (`joy_fire` in
    // board.rs) before init_epd. Held → force the safe OTP
    // waveform and ignore LUT.CFG. `steal` is sound here: this is a
    // one-shot read that completes before the button task claims the pin
    // (board!(p, joy_fire) at spawn time below).
    #[cfg(feature = "ssd1675-driver")]
    let force_otp_lut = {
        use embassy_nrf::gpio::{Input, Pull};
        let fire = Input::new(
            board!(unsafe { embassy_nrf::Peripherals::steal() }, joy_fire),
            Pull::Up,
        );
        let held = fire.is_low();
        drop(fire);
        held
    };
    #[cfg(feature = "ssd1675-driver")]
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
        force_otp_lut,
    )
    .await
    .unwrap();
    #[cfg(feature = "ssd1675-driver")]
    defmt::info!("EPD initialized");
    // EPD tuning and the timezone are persisted in the shared "settings" KV
    // namespace, in every build (mesh or not). Load them synchronously here,
    // before any task that renders with them starts.
    bornhack_aegg::TZ_OFFSET_QH.store(
        bornhack_aegg::fw::settings::get_timezone().await,
        core::sync::atomic::Ordering::Relaxed,
    );
    #[cfg(feature = "ssd1675-driver")]
    {
        bornhack_aegg::fw::epd::load_persisted_lut_speed().await;
        bornhack_aegg::fw::epd::load_persisted_temp_bias().await;
    }
    // A `speed=` in LUT.CFG (a calibration bundle) takes precedence over
    // the persisted value — apply it last.
    #[cfg(feature = "ssd1675-driver")]
    bornhack_aegg::fw::epd::apply_lut_file_speed();

    // Host-side partial-refresh state — lazily allocates ~46 KB
    // .bss buffers (shadow + pending + sent_pending + dirty + 2
    // plane scratches).  Initialised to all-White to match the
    // post-boot panel-clear refresh below.
    #[cfg(feature = "ssd1675-driver")]
    let dims = EpdConfig::to_dimensions();
    #[cfg(feature = "ssd1675-driver")]
    let mut partial_state = bornhack_aegg::fw::epd::partial_state_take(dims.rows, dims.cols);
    #[cfg(feature = "ssd1675-driver")]
    partial_state.clear_to(ssd1675::graphics::Color::White);
    // Charge-ledger / halo-kick tuning from `EPD.CFG` (FAT12 is formatted at
    // the top of `main`); no file = the variant's defaults. Clamped by the
    // driver crate, so a bad file cannot harm the panel.
    #[cfg(feature = "ssd1675-driver")]
    {
        let tuning = bornhack_aegg::fw::epd_cfg::load(ssd1675::EpdTuning::for_variant(
            display.variant(),
        ))
        .await;
        partial_state.set_tuning(tuning);
    }

    // Boot breadcrumb #2 — switch from orange to blue while the boot
    // tri-color refresh + remaining task spawns finish.  This is the
    // longest dark phase in the original boot, ~13s, so a colour change
    // here gives users a "no, it's not stuck" signal partway through.
    led::set_color(led::Rgb::BLUE, led::FOREVER);

    // Boot-time full blank: clear both planes to white and push with the
    // tri-color waveform so red ink particles get cycled too.  Wipes any
    // residual ghosting from the previous power-on session before the
    // first fast-LUT refresh paints over it.
    // Temperature BEFORE the first refresh: the boot clear (and every refresh
    // after) must pick the panel's real LUT band, not the 20 °C default — a
    // cold default over-drives a warm panel.  Safe to read TEMP directly here:
    // MPSL / SoftDevice isn't up yet, so it owns nothing.
    #[cfg(feature = "ssd1675-driver")]
    {
        let _ = bornhack_aegg::fw::temperature::read_and_cache().await;
        let panel_c10 = bornhack_aegg::fw::epd::panel_temp_c10(display.variant());
        if panel_c10 != i16::MIN {
            display.set_active_temperature(panel_c10);
        }
        // Frame period of the active band (sets the halo / correction
        // pulse length: HALO_FRAMES x this).
        defmt::info!("EPD: frame {=u32} ns", display.frame_ns());

        display.clear(Color::White);
        let _ = display.reset().await;
        let _ = display
            .update_tc(bornhack_aegg::fw::epd::current_lut_speed())
            .await;
    }

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
    // Both panels run it: the SSD1675 here on the raw `EpdGfx`, the SSD1680 further
    // down once its unified driver exists (same position relative to the boot clear).
    #[cfg(feature = "ssd1675-driver")]
    if !bornhack_aegg::fw::factory_test::is_passed().await {
        bornhack_aegg::fw::factory_test::run_first_boot_interactive(
            &hw,
            &mut bornhack_aegg::fw::epd_1675_driver::FactoryGfx(&mut display),
        )
        .await;
    }

    #[cfg(feature = "ssd1675-driver")]
    let _ = display.deep_sleep().await;

    // ── Unified EPD driver ────────────────────────────────────────────────
    // Wrap the panel into the shared `EpdDriver` so the whole UI (battery-
    // critical, the display loop) renders through one trait. The SSD1675
    // adapter owns the freshly-seeded `PartialState`; the SSD1680 path builds
    // its own SPI bus + planes here.
    #[cfg(feature = "ssd1675-driver")]
    let mut driver = bornhack_aegg::fw::epd_1675_driver::Ssd1675Driver::new(display, partial_state);

    #[cfg(feature = "ssd1680-driver")]
    let mut driver = {
        use ssd1680::{Display, Interface};
        use embassy_nrf::spim::{Config, Frequency, Spim};
        use embedded_hal_bus::spi::ExclusiveDevice;

        // 152×152 badge panel: 152/8 * 152 = 2888 bytes per plane.
        //
        // Every buffer is a zero-initialised `ConstStaticCell` taken in place:
        // it lives in `.bss` and no copy of the array is ever built on the
        // stack. (`StaticCell::init([..; N])` materialises its argument in
        // `main`'s poll frame first; with ~34 KiB of planes that frame alone
        // grew past 40 KiB and, with flip-link putting the stack under
        // `.bss`, overflowed the boot stack → HardFault → WDT reset loop, the
        // flashing white LED.) `Driver::new` fills each plane with its real
        // starting value.
        use static_cell::ConstStaticCell;
        const PLANE_N: usize = 2888;
        static BLACK_1680: ConstStaticCell<[u8; PLANE_N]> = ConstStaticCell::new([0; PLANE_N]);
        static RED_1680: ConstStaticCell<[u8; PLANE_N]> = ConstStaticCell::new([0; PLANE_N]);
        static PREV_1680: ConstStaticCell<[u8; PLANE_N]> = ConstStaticCell::new([0; PLANE_N]);
        static PREV_RED_1680: ConstStaticCell<[u8; PLANE_N]> =
            ConstStaticCell::new([0; PLANE_N]);
        // Damage bitmap: 1 bit per panel pixel, so the same PLANE_N bytes.
        // Records every pixel a waveform has touched but not confirmed (an
        // interrupted drive, or a preview awaiting its settle), and is
        // cleared only when a full-length drive completes — this is what
        // lets an interrupted refresh be resumed instead of diffed away.
        static DIRTY_1680: ConstStaticCell<[u8; PLANE_N]> = ConstStaticCell::new([0; PLANE_N]);
        static CODE_BW_1680: ConstStaticCell<[u8; PLANE_N]> = ConstStaticCell::new([0; PLANE_N]);
        static CODE_RED_1680: ConstStaticCell<[u8; PLANE_N]> =
            ConstStaticCell::new([0; PLANE_N]);
        // Charge ledger: a signed 4-bit V·t value per panel pixel (152·152/2
        // = 11552 bytes, the size of the old top-up counter). Schedules short
        // opposite-rail correction pulses for pixels carrying excess charge;
        // see `ssd1680::ledger`.
        const LEDGER_N: usize = ssd1680::ChargeLedger::bytes_for(152, 152);
        static LEDGER_1680: ConstStaticCell<[u8; LEDGER_N]> = ConstStaticCell::new([0; LEDGER_N]);
        // 152×152 UI canvas planes (the driver scales these into the panel).
        use bornhack_aegg::fw::epd_1680_driver::CANVAS_BYTES;
        static CANVAS_BW_1680: ConstStaticCell<[u8; CANVAS_BYTES]> =
            ConstStaticCell::new([0; CANVAS_BYTES]);
        static CANVAS_RED_1680: ConstStaticCell<[u8; CANVAS_BYTES]> =
            ConstStaticCell::new([0; CANVAS_BYTES]);

        // Read the panel's OWN OTP full waveform back over cmd 0x33 and halve
        // its shake repeats. Runs BEFORE the display Spim is built (it borrows
        // SPI3 + the EPD pins, restores TX-only, then forgets them), and is
        // installed as the full() LUT just after Display::new below.
        let otp_full_lut = halve_shake_rp_1680(probe_otp_full_lut_1680().await);

        // SPI3 bus + control pins (mirrors `epd::init_epd`).
        let mut cfg = Config::default();
        cfg.frequency = Frequency::M16;
        let bus = Spim::new_txonly(
            board!(p, epd_spi),
            Irqs1680,
            board!(p, epd_sck),
            board!(p, epd_mosi),
            cfg,
        );
        let csn = Output::new(board!(p, epd_csn), Level::High, OutputDrive::Standard);
        let rst = Output::new(board!(p, epd_reset), Level::Low, OutputDrive::Standard);
        let dc = Output::new(board!(p, epd_dc), Level::Low, OutputDrive::Standard);
        let busy = Input::new(board!(p, epd_busy), Pull::Down);
        let spi_dev = ExclusiveDevice::new(bus, csn, embassy_time::Delay)
            .expect("ExclusiveDevice::new is infallible for an OutputPin CS");
        let iface = Interface::new(spi_dev, busy, dc, rst);
        // x_offset = 8: the panel's leftmost visible pixel sits at controller
        // source 8, not 0 (SSD1680 has 176 sources; this module uses 152 of
        // them starting one byte in). Without it the image sits 8 px left.
        let mut display = Display::new(iface, 152, 152, 8);
        // Install the boot-probed OTP full waveform (shake repeats halved).
        display.set_full_lut(otp_full_lut);

        bornhack_aegg::fw::epd_1680_driver::Driver::new(
            display,
            152,
            152,
            CANVAS_BW_1680.take(),
            CANVAS_RED_1680.take(),
            BLACK_1680.take(),
            RED_1680.take(),
            PREV_1680.take(),
            PREV_RED_1680.take(),
            DIRTY_1680.take(),
            CODE_BW_1680.take(),
            CODE_RED_1680.take(),
            LEDGER_1680.take(),
            false, // scale_fill: canvas matches the 152×152 panel 1:1
        )
    };

    // Waveform tuning from `EPD.CFG` on the badge volume (FAT12 is formatted
    // at the top of `main`); no file = built-in defaults. Clamped by the
    // driver crate, so a bad file cannot harm the panel.
    #[cfg(feature = "ssd1680-driver")]
    driver.set_tuning(bornhack_aegg::fw::epd_cfg::load().await);

    // Boot paint on the unified driver (SSD1680 only — the SSD1675 boot clear
    // already ran on the raw panel above so the factory-test forensic trail and
    // the temperature-compensated tri-colour wipe stay intact).
    #[cfg(feature = "ssd1680-driver")]
    {
        use bornhack_aegg::epd_driver::{EpdDriver, RefreshMode};
        use embedded_graphics::draw_target::DrawTarget;
        let _ = EpdDriver::init(&mut driver).await;
        let _ = driver.clear(bornhack_aegg::color::EpdColor::White);
        let _ = driver.refresh(RefreshMode::Full).await;
        // No deep_sleep: the SSD1680 driver intentionally keeps the controller
        // awake between refreshes (see `Driver::refresh`).
    }

    // First-boot interactive sign-off on the SSD1680 (see the SSD1675 call
    // above): only on a badge without the `hwtest:passed` KV flag; halts
    // forever on fail or on the ship image, exactly like the 1675 path.
    #[cfg(feature = "ssd1680-driver")]
    if !bornhack_aegg::fw::factory_test::is_passed().await {
        bornhack_aegg::fw::factory_test::run_first_boot_interactive(&hw, &mut driver).await;
    }

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
    //    (defaults to Classic on a fresh badge); `BPETS_V3.CFG` on the
    //    USB-MSC partition, if present, overrides individual fields on
    //    top of the preset.
    #[cfg(feature = "game")]
    {
        let mode = bornhack_aegg::game::settings::load_mode_from_kv().await;
        let _ = bornhack_aegg::game::bornpets_cfg::load_and_install(mode).await;
        // Register custom pets from PETS.CFG (if any) before the game reads
        // the roster.  No file → built-in roster only.
        bornhack_aegg::game::pets_cfg::load_and_install().await;
        spawner.must_spawn(bornhack_aegg::game::settings::persister_task());
        // Apply the persisted "game enabled" flag to the display state so a
        // previously disabled game screen stays hidden across reboot (default
        // is enabled).
        let game_on = bornhack_aegg::game::settings::load_enabled_from_kv().await;
        DISPLAY_STATE.lock(|cell| {
            cell.borrow_mut()
                .set_screen_enabled(bornhack_aegg::SCREEN_GAME as usize, game_on);
        });
    }

    // ── Device settings persister (non-mesh builds) ──────────────────────
    // Mesh builds persist timezone + EPD tuning from `persister::run`.
    #[cfg(not(feature = "mesh"))]
    spawner.must_spawn(bornhack_aegg::fw::settings::persister_task());

    // ── Mesh stack (contacts, identity, BLE) ─────────────────────────────
    // Must come before temperature/EPD because it consumes p.RNG, p.RTC0,
    // p.TIMER0, p.TEMP, and PPI channels.
    #[cfg(feature = "mesh")]
    let identity = {
        spawner.must_spawn(bond_task());
        spawner.must_spawn(persister::run());
        spawner.must_spawn(bornhack_aegg::fw::mesh::contacts_screen::refresh_cache_task());
        spawner.must_spawn(bornhack_aegg::fw::mesh::contacts_screen::mutation_persister_task());
        spawner.must_spawn(bornhack_aegg::fw::mesh::pm_inbox::pm_led_task());
        ContactStore::new().init().await;

        // Seed channel KV slots before the meshcore listener or BLE task
        // load them.  Previously this lived inside `run_ble_peripheral`
        // after a spin-wait on `INITIAL_BONDS`, which let the meshcore
        // listener load an empty channel set on a fresh-flash boot.
        channels::init().await;
        channels::ensure_shdw_channel().await;
        defmt::info!(
            "channels: store ready ({} active)",
            channels::count_active().await
        );

        // Load persisted boost-RX into its in-RAM atomic SYNCHRONOUSLY here —
        // before any task that reads it starts. (The timezone is loaded
        // earlier, right after EPD init, for all builds.)
        bornhack_aegg::BOOSTED_RX_GAIN.store(
            settings::get_boost_rx().await,
            core::sync::atomic::Ordering::Relaxed,
        );
        bornhack_aegg::RX_POWER_SAVE.store(
            settings::get_rx_power_save().await,
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
            bornhack_aegg::fw::mesh::DEFAULT_SCOPE_KEY.lock(|c| c.set(scope));
        }

        {
            let rp = settings::get_radio_params_or_default().await;
            rp.store_live();
        }
        {
            use core::sync::atomic::Ordering::Relaxed;
            if let Some(op) = settings::get_other_params().await {
                op.store_live();
            }
            bornhack_aegg::fw::mesh::PATH_HASH_MODE
                .store(settings::get_path_hash_mode().await.min(2), Relaxed);

            let adv = settings::get_advert_config_or_default().await;
            bornhack_aegg::ADVERT_ENABLED.store(adv.enabled, Relaxed);
            bornhack_aegg::ADVERT_INTERVAL_HOURS.store(adv.interval_hours, Relaxed);

            bornhack_aegg::IGNORE_BLINK.store(settings::get_ignore_blink().await, Relaxed);
            bornhack_aegg::REGION_FILTER.store(settings::get_region_filter().await, Relaxed);
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
            let ble_prng_seed = bornhack_aegg::fw::trng::seed32();
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
                    sec_key: identity.sec_key,
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
        bornhack_aegg::game::sprite_loader::init().await;
        bornhack_aegg::game::pet_registry::drop_without_sprites(|id| {
            let idle = bornhack_aegg::game::engine::anim_files::anim_id_for(
                bornhack_aegg::game::engine::DisplayAnim::Idle,
            );
            bornhack_aegg::game::sprite_loader::count_anim_frames(id, idle) > 0
        });
        bornhack_aegg::game::lifecycle::init().await;
        defmt::info!(
            "game: retire hidden pet: {}",
            bornhack_aegg::game::lifecycle::retire_hidden_pet()
        );
    }

    // ── Temperature ──────────────────────────────────────────────────────
    let temp_celsius = bornhack_aegg::fw::temperature::read_and_cache().await;
    defmt::info!("Die temperature: {} °C", temp_celsius);

    // (EPD display was initialised earlier — see the "Hardware probe"
    // section.  The display is already past its boot-time blank and
    // is in deep_sleep until the display task takes over.)

    // LEDs are initialised earlier (above the mesh stack) so the led_task is
    // already running when the contact store lights the green LED during a
    // legacy-format wipe.

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
            show_battery_critical(&mut driver, &e).await;
            return;
        }
    };
    spawner.must_spawn(battery_task(battery_monitor));
    // Drives the transient screen-lock padlock (5 s auto-hide, refresh on key).
    spawner.must_spawn(bornhack_aegg::lock::overlay_task());
    spawner.must_spawn(minute_tick_task());
    #[cfg(feature = "mesh")]
    spawner.must_spawn(advert_ticker_task());
    #[cfg(all(feature = "mesh", feature = "game"))]
    spawner.must_spawn(pet_beacon_ticker_task());
    #[cfg(feature = "game")]
    spawner.must_spawn(battle_timer_task());

    // ── USB mass storage ──────────────────────────────────────────────────
    // Spawn BEFORE the sponsor slideshow so the host can mount the FAT
    // (USB mass storage is now spawned much earlier — see the block
    // right after EPD init so the factory-test halt can co-exist
    // with host-side asset copy in a single plug-cycle.)

    // ── Boot-complete chime ───────────────────────────────────────────────
    // Plays once on every boot (first boot included) when the user
    // hasn't disabled it via Settings → Boot chime.  The audible
    // signal that init has finished and the badge is ready.
    if bornhack_aegg::BOOT_CHIME_ENABLED.load(core::sync::atomic::Ordering::Relaxed) {
        bornhack_aegg::fw::buzzer::play(bornhack_aegg::SONG_STARTUP_INDEX as usize);
    }

    // Boot init done — release the boot boost vote. From here the buck/boost
    // pin is driven by EPD-refresh and LoRa-TX votes only.
    drop(boot_boost);

    // ── Display loop + concurrent tasks ──────────────────────────────────
    defmt::info!("Entering main loop...");

    // Boot breadcrumb #3 — single 500 ms green pulse, then idle.  Tells the
    // user the badge is past init and the screen carousel is live.
    led::set_color(led::Rgb::GREEN, Duration::from_millis(500));
    // Qwiic I2C bus (external connector, TWISPI0 + P1_10/P1_11).  Owned by a
    // dedicated `keyboard_task` so the optional I2C keyboard can be polled
    // independently of the (slow) e-paper display loop — a wedged I2C read
    // then can't stall the display, and the Qwiic Scan runs there too.
    static QWIIC_TX: StaticCell<[u8; bornhack_aegg::fw::qwiic::TX_BUF_LEN]> = StaticCell::new();
    let qwiic_tx = QWIIC_TX.init([0u8; bornhack_aegg::fw::qwiic::TX_BUF_LEN]);
    let qwiic_bus = bornhack_aegg::fw::qwiic::new_bus(
        p.TWISPI0,
        board!(p, qwiic_sda).into(),
        board!(p, qwiic_scl).into(),
        qwiic_tx,
    );
    spawner.must_spawn(keyboard_task(qwiic_bus));

    #[cfg(feature = "ssd1675-driver")]
    let main_loop = display_loop(&mut driver, &mut button_rcvr);
    #[cfg(feature = "ssd1680-driver")]
    let main_loop = display_loop_1680(&mut driver, &mut button_rcvr);

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

/// Signalled by `keyboard_task` whenever it injects one or more keys from the
/// I2C keyboard.  The display loop waits on this alongside the buttons — and,
/// crucially, WITHOUT the `allow_sprite` gate — so a keystroke interrupts an
/// in-flight e-paper refresh and redraws with the new text, exactly the way a
/// joystick move does (`button_rcvr.changed()` aborts the refresh the same
/// way).  An e-paper refresh is a fixed ~500 ms+ whole-panel drive regardless
/// of how few pixels changed; blocking on one per keystroke is what made fast
/// typing lag seconds behind.  Interrupting instead means fast keys keep
/// aborting and restarting the refresh, and the last one completes once typing
/// pauses — so the screen always chases the latest text and feels responsive.
static KBD_REDRAW: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

/// Per-loop sprite animation state shared by both panel loops (empty without
/// the `game` feature).
#[cfg(any(feature = "ssd1675-driver", feature = "ssd1680-driver"))]
struct AnimState {
    #[cfg(feature = "game")]
    sprite_frame: u8,
    // Last animation id observed by the sprite-frame advance.  When
    // it changes, `sprite_frame` resets to 0 so each new animation
    // starts at its first frame regardless of where the previous one
    // left the counter.
    #[cfg(feature = "game")]
    last_anim_id: u8,
    // Absolute deadline of the next idle frame; survives loop re-entries and
    // redraws so minute ticks and wakes cannot push it back.
    #[cfg(feature = "game")]
    pacer: bornhack_aegg::game::pacing::FramePacer,
}

#[cfg(any(feature = "ssd1675-driver", feature = "ssd1680-driver"))]
impl AnimState {
    const fn new() -> Self {
        Self {
            #[cfg(feature = "game")]
            sprite_frame: 0,
            #[cfg(feature = "game")]
            last_anim_id: 0xFF,
            #[cfg(feature = "game")]
            pacer: bornhack_aegg::game::pacing::FramePacer::new(),
        }
    }

    /// Advance the animation frame; call only when the sprite timer fired.
    /// Anim-change detection lives in `render_frame`, just before the render,
    /// so the reset is visible on the same frame as the change.
    fn advance_sprite(&mut self, sprite_advance: bool) {
        #[cfg(feature = "game")]
        if sprite_advance {
            self.pacer.on_frame();
            let anim = bornhack_aegg::game::lifecycle::display_anim();
            let kind = bornhack_aegg::game::lifecycle::pet_kind();
            let count = bornhack_aegg::game::engine::anim_files::frame_count(kind, anim);
            if count > 0 {
                // Hatching clamps to the last frame instead of wrapping.
                self.sprite_frame = bornhack_aegg::game::engine::anim_files::clamp_or_wrap(
                    self.sprite_frame as u32 + 1,
                    count,
                    anim,
                );
            }
        }
        #[cfg(not(feature = "game"))]
        let _ = sprite_advance;
    }
}

/// Clear the frame, render the game (when active) and the UI into `display`;
/// returns the active screen.
#[cfg(any(feature = "ssd1675-driver", feature = "ssd1680-driver"))]
async fn render_frame<D>(display: &mut D, anim_state: &mut AnimState) -> u8
where
    D: embedded_graphics::draw_target::DrawTarget<Color = bornhack_aegg::EpdColor>
        + bornhack_aegg::epd_driver::PlaneAccess,
{
    let _ = display.clear(bornhack_aegg::EpdColor::White);
    let active_screen = DISPLAY_STATE.lock(|f| f.borrow().active_screen());

    // ── Game cycle: update engine, render animation ────────────────
    // Reset the sprite-frame counter on animation change *before*
    // the render so the new anim shows frame 0 immediately rather
    // than waiting for the next sprite-tick fire.
    #[cfg(feature = "game")]
    if active_screen == bornhack_aegg::SCREEN_GAME {
        let anim = bornhack_aegg::game::lifecycle::display_anim();
        let id = bornhack_aegg::game::engine::anim_files::anim_id_for(anim);
        if id != anim_state.last_anim_id {
            anim_state.last_anim_id = id;
            anim_state.sprite_frame = 0;
        }
        bornhack_aegg::game::render(display, anim_state.sprite_frame).await;
    }
    #[cfg(not(feature = "game"))]
    let _ = anim_state;

    #[cfg(feature = "mesh")]
    if active_screen == SCREEN_PM {
        bornhack_aegg::PM_UNREAD.store(false, core::sync::atomic::Ordering::Relaxed);
    }

    let health_str = with_health!(|f| f.to_string());
    let bat_str = battery::read_pct();
    if draw_graphics(display, &health_str, &bat_str).is_err() {
        health_err!(epd, "Failed to draw graphics");
    }
    active_screen
}

#[cfg(feature = "ssd1675-driver")]
async fn display_loop(
    driver: &mut bornhack_aegg::fw::epd_1675_driver::Ssd1675Driver<'_>,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) {
    #[cfg(feature = "game")]
    use bornhack_aegg::EpdColor;
    use embassy_futures::select::{Either, select};

    let mut anim_state = AnimState::new();

    // Track the previously-rendered screen so we can promote the
    // first refresh after a screen switch to a full panel-clearing
    // drive — clears ghosting from the outgoing screen.  0xFF =
    // sentinel for "no prior screen" (boot path already does its
    // own clears).
    let mut last_screen: u8 = 0xFF;

    loop {
        // (The optional I2C keyboard and the Qwiic Scan now run in
        // `keyboard_task`, which owns the Qwiic bus — see main().  Keeping I2C
        // off the display loop means a wedged read can't stall the display.)

        // Hidden-combo de-ghost: a deliberate, user-triggered full black →
        // white flush, run before this iteration's normal content draw
        // below. Each half is a genuine inverting full refresh (same
        // primitive the boot-time clear and heavy-graphic screens use),
        // so the panel visibly settles solid black, then solid white,
        // before the real screen content goes down. FULL_REFRESH_PENDING
        // (already checked further down) then forces that redraw to mark
        // every pixel dirty, so nothing here needs its own "mark all"
        // bookkeeping.
        // ── Battle animation takeover ──────────────────────────────────
        // A resolved battle (local challenge or an incoming mesh result) plays
        // a full-screen two-stage animation — 5 s per stage — over whatever
        // screen is active. Each stage is one e-paper refresh.
        #[cfg(feature = "game")]
        if bornhack_aegg::game::battle_anim_active() {
            let anim_speed = bornhack_aegg::fw::epd::current_lut_speed();
            // The challenger's exact chance (the ACCEPT just came in) shows
            // once, as one refresh, before the animation (spec 12.4).
            if bornhack_aegg::game::battle_view::take_pre_show() {
                use embedded_graphics::draw_target::DrawTarget;
                let _ = driver.clear(EpdColor::White);
                {
                    let (display, _ps) = driver.staged_parts_mut();
                    let _ = display.reset().await;
                }
                let _ = bornhack_aegg::game::battle_view::draw_queued_pre(driver);
                {
                    let (display, ps) = driver.staged_parts_mut();
                    let _ = display.update_tc(anim_speed).await;
                    ps.reset_ledger();
                    let _ = display.deep_sleep().await;
                }
                Timer::after_millis(2_000).await;
            }
            // Drive the two stages EXPLICITLY, holding each ~5 s after its
            // refresh completes. Deriving the stage from wall-clock elapsed
            // would skip the result frame: a single e-paper refresh takes
            // several seconds, so by the time the standing frame + 5 s hold
            // finish, the elapsed clock has already passed the result window.
            for stage in [
                bornhack_aegg::game::BattleStage::Standing,
                bornhack_aegg::game::BattleStage::Result,
            ] {
                use embedded_graphics::draw_target::DrawTarget;
                let _ = driver.clear(EpdColor::White);
                {
                    let (display, _ps) = driver.staged_parts_mut();
                    let _ = display.reset().await;
                }
                bornhack_aegg::game::battle_view::render_anim(driver, stage).await;
                {
                    let (display, ps) = driver.staged_parts_mut();
                    let _ = display.update_tc(anim_speed).await;
                    ps.reset_ledger();
                    let _ = display.deep_sleep().await;
                }
                Timer::after_millis(5_000).await;
            }
            bornhack_aegg::game::clear_battle_anim();
            // Force a clean full redraw of the screen we interrupted.
            bornhack_aegg::FULL_REFRESH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
            continue;
        }

        if bornhack_aegg::FORCE_FLUSH_PENDING.swap(false, core::sync::atomic::Ordering::Relaxed) {
            let speed = bornhack_aegg::fw::epd::current_lut_speed();
            let _deghost_boost = bornhack_aegg::fw::power::boost(
                bornhack_aegg::fw::power::Source::Epd,
            );
            let (display, ps) = driver.staged_parts_mut();
            display.clear(Color::Black);
            let _ = display.reset().await;
            let _ = display.update_tc(speed).await;
            display.clear(Color::White);
            let _ = display.reset().await;
            let _ = display.update_tc(speed).await;
            ps.reset_ledger();
            let _ = display.deep_sleep().await;
            drop(_deghost_boost);
            bornhack_aegg::FULL_REFRESH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
        }

        let active_screen = render_frame(driver, &mut anim_state).await;

        // Screen switches and game menu open/close redraw the WHOLE frame
        // as a partial (mark every pixel dirty) rather than running a full
        // OTP refresh.  Mini-games / menus set `FULL_REFRESH_PENDING` on
        // open and close; we consume it with `swap` so the redraw applies
        // once.  The expensive full refresh is now promoted automatically
        // inside `update_partial` once the cumulative changed-pixel counter
        // reaches `full_after_screens` (3 screens, the vendor default) — see `should_force_full`.
        // First iteration after boot (last_screen == 0xFF) skips since boot
        // already drove a clean frame.
        let screen_changed = last_screen != 0xFF && last_screen != active_screen;
        let mark_all = bornhack_aegg::FULL_REFRESH_PENDING
            .swap(false, core::sync::atomic::Ordering::Relaxed)
            || screen_changed;

        // UI content is now composed into the adapter's `EpdGfx` buffers. Drop
        // to the raw panel + host delta ledger for the SSD1675-specific drive
        // (variant probe, plane sync, partial/tri-colour waveform selection);
        // this borrow holds until the end of the iteration.
        let (display, partial_state) = driver.staged_parts_mut();

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
            wait_display_event(button_rcvr, active_screen, &mut anim_state, true).await
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
                        // The "Hello my name is" badge is also a heavy
                        // full-frame graphic (big solid name band + red
                        // accents) that under-drives and ghosts on the
                        // non-inverting delta LUT.  Force a genuine inverting
                        // full refresh while it's shown so it de-ghosts and
                        // lays down cleanly.  `partial_idle` above already
                        // skips redundant redraws, so this only flashes on an
                        // actual content change (arriving / name edited).
                        let name_screen = active_screen == bornhack_aegg::SCREEN_NAME;
                        // Pet screen showing a red status message (toast /
                        // "gone" prompt): run the full tri-color refresh so the
                        // red text seats — the delta LUT under-drives red.
                        // `partial_idle` above keeps this from re-flashing once
                        // the message is drawn.
                        #[cfg(feature = "game")]
                        // A change that leaves the red plane alone (sky switch,
                        // floor pile under a red line) needs no red seating, so
                        // it stays on the quiet delta path.
                        let game_status = active_screen == bornhack_aegg::SCREEN_GAME
                            && bornhack_aegg::game::status_wants_full_refresh()
                            && partial_state.bbox_of_dirty().is_some_and(|b| {
                                bornhack_aegg::refresh_decision::touches_red(
                                    b.x,
                                    b.y,
                                    b.w,
                                    b.h,
                                    |x, y| {
                                        partial_state.is_dirty(x, y)
                                            && (partial_state.pending_color(x, y) == Some(Color::Red)
                                                || partial_state.shadow_color(x, y)
                                                    == Some(Color::Red))
                                    },
                                )
                            });
                        #[cfg(not(feature = "game"))]
                        let game_status = false;
                        // Text-entry is a fast-changing overlay on top of
                        // whatever screen is active — never do the multi-second
                        // full/inverting refresh while typing (e.g. the
                        // start-screen flag would otherwise force one per key).
                        let text_entry = bornhack_aegg::text_entry::is_active();
                        if !text_entry && (start_screen || name_screen || game_status) {
                            // Render via the full tri-color path (the same one
                            // the sponsor slideshow uses), NOT the delta
                            // no-invert LUT.  The no-invert LUT leaves the red
                            // row (LUT2) undriven, so the red band / accents
                            // render transparent.  `update_tc` drives red via
                            // LUT2.  Keep the delta shadow consistent with the
                            // panel afterwards so the next screen diffs cleanly.
                            let _ = bornhack_aegg::fw::power::boosted(
                                bornhack_aegg::fw::power::Source::Epd,
                                display.update_tc(speed),
                            )
                            .await;
                            partial_state.commit_refresh();
                            // A full tri-colour drive: start the charge
                            // ledger from zero, as update_partial's own full does.
                            partial_state.reset_ledger();
                        } else {
                            let _ = bornhack_aegg::fw::power::boosted(
                                bornhack_aegg::fw::power::Source::Epd,
                                display.update_partial(partial_state, speed),
                            )
                            .await;
                        }
                    } else {
                        let _ = bornhack_aegg::fw::power::boosted(
                            bornhack_aegg::fw::power::Source::Epd,
                            display.update_bw(UpdateMode::Mode1, speed),
                        )
                        .await;
                    }
                    let _ = display.deep_sleep().await;
                },
                // `allow_sprite = true`: ANY draw event — buttons/signals AND
                // the animation frame timer — cancels an in-flight refresh.
                // The dropped future leaves `in_flight` + dirty set, so the
                // next `update_partial` resets the controller and re-drives
                // (see display.rs recovery). Simple case: everything interrupts;
                // ghosting corner-cases handled later.
                wait_display_event(button_rcvr, active_screen, &mut anim_state, true),
            )
            .await
            {
                Either::First(_) => {
                    // Refresh finished uninterrupted.  Commit the screen-
                    // transition tracker, then wait the frame timer.
                    last_screen = active_screen;
                    wait_display_event(button_rcvr, active_screen, &mut anim_state, true).await
                }
                // Interrupted mid-refresh.  Propagate the waiter's verdict:
                // sprite-timer fire → advance the frame (true); button/signal
                // → redraw without advancing (false).
                Either::Second(sprite) => sprite,
            }
        };

        led::set_color(led::Rgb::RED, Duration::from_millis(50));

        anim_state.advance_sprite(sprite_advance);
    }
}

/// SSD1680 display loop — renders the same UI through the panel-agnostic
/// [`EpdDriver`]/[`PlaneAccess`] surface. The SSD1680 de-ghosts on its own
/// non-blink delta waveform, so every runtime update uses [`RefreshMode::Fast`]
/// (the boot paint in `main` is the only full refresh) and screen switches ride
/// the same delta path.
///
/// A button or signal arriving mid-drive cancels the refresh rather than
/// queueing behind it; the controller is recovered by hardware reset on the
/// next refresh. See the `select` at the bottom of the loop for the mechanism
/// and its trade-offs.
#[cfg(feature = "ssd1680-driver")]
async fn display_loop_1680<D>(
    display: &mut D,
    button_rcvr: &mut embassy_sync::watch::Receiver<
        '_,
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        u8,
        2,
    >,
) where
    D: bornhack_aegg::epd_driver::EpdDriver
        + bornhack_aegg::epd_driver::PlaneAccess
        + embedded_graphics::geometry::OriginDimensions,
{
    use bornhack_aegg::epd_driver::RefreshMode;
    use embassy_futures::select::{Either, select};

    let mut anim_state = AnimState::new();

    loop {
        // ── Battle animation takeover ──────────────────────────────────
        // Same two-stage takeover as the SSD1675 loop: standing, then the
        // won/lost poses, each one uninterrupted refresh held ~5 s, before the
        // screen underneath (the battle result card) is drawn again.
        #[cfg(feature = "game")]
        if bornhack_aegg::game::battle_anim_active() {
            // The challenger's exact chance shows once before the animation.
            if bornhack_aegg::game::battle_view::take_pre_show() {
                let _ = display.clear(bornhack_aegg::color::EpdColor::White);
                let _ = bornhack_aegg::game::battle_view::draw_queued_pre(display);
                let _ = display.refresh(RefreshMode::Fast).await;
                Timer::after_millis(2_000).await;
            }
            for stage in [
                bornhack_aegg::game::BattleStage::Standing,
                bornhack_aegg::game::BattleStage::Result,
            ] {
                let _ = display.clear(bornhack_aegg::color::EpdColor::White);
                bornhack_aegg::game::battle_view::render_anim(display, stage).await;
                let _ = display.refresh(RefreshMode::Fast).await;
                Timer::after_millis(5_000).await;
            }
            bornhack_aegg::game::clear_battle_anim();
            continue;
        }

        // Hidden de-ghost combo (`display_flush`): black -> white full-refresh
        // cycle, then force a whole-frame redraw. Mirrors the SSD1675 loop's
        // FORCE_FLUSH_PENDING handling, using the driver's `Full` path.
        if bornhack_aegg::FORCE_FLUSH_PENDING.swap(false, core::sync::atomic::Ordering::Relaxed) {
            let _ = display.clear(bornhack_aegg::color::EpdColor::Black);
            let _ = display.refresh(RefreshMode::Full).await;
            let _ = display.clear(bornhack_aegg::color::EpdColor::White);
            let _ = display.refresh(RefreshMode::Full).await;
            let _ = display.deep_sleep().await;
            bornhack_aegg::FULL_REFRESH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
        }

        let active_screen = render_frame(display, &mut anim_state).await;

        // The SSD1680 de-ghosts on the non-blink delta, so screen switches use
        // the fast windowed delta too — consume any pending full-refresh request
        // so the flag doesn't leak.
        let _ = bornhack_aegg::FULL_REFRESH_PENDING
            .swap(false, core::sync::atomic::Ordering::Relaxed);
        let mode = RefreshMode::Fast;

        // Interrupt the in-flight LUT on a new draw event instead of waiting the
        // whole drive out — the SSD1675 path does the same, and it is what makes
        // button presses feel immediate rather than one refresh behind.
        //
        // The SSD1680 has no abort command, so the mechanism is: drop the
        // refresh future (it parks in `busy_wait_for_completion`), which leaves
        // `Display::in_flight` set; the next refresh sees the flag and runs
        // `hw_reset_and_init` — RESET pin, SoftReset, re-init — before driving
        // the new frame. See the "Cancellation safety" section in
        // `vendor/ssd1680/src/display.rs`. Losing controller RAM to that reset
        // costs nothing here because `update_delta` re-uploads both full code
        // planes every call. The adapter commits `prev`/`prev_red` only after
        // the await returns, so a cancelled frame's pixels stay dirty and the
        // next delta re-drives them.
        //
        // `allow_sprite = false`: only buttons/signals cut a drive short. If the
        // animation frame timer could interrupt too, a timer faster than the
        // refresh would cancel every drive in turn and the panel would never
        // finish one — the sprite would freeze while the controller thrashed
        // through resets. Animation ticks therefore wait their turn; user input
        // does not.
        let sprite_advance = match select(
            display.refresh(mode),
            wait_display_event(button_rcvr, active_screen, &mut anim_state, false),
        )
        .await
        {
            // Drive completed untouched — wait for the next event. The driver
            // leaves the analog rails up so a quick follow-up update skips the
            // booster soft-start; drop them once nothing has happened for
            // `ANALOG_IDLE_MS`. The power-off runs beside the wait (not before
            // it) so the sprite timer inside `wait_display_event` keeps its
            // original deadline. An event during the power-off drops it
            // mid-command; the next refresh then resets the controller.
            //
            // A preview leaves pixels short of saturation: when one is
            // pending, the settle pass runs first, after `SETTLE_MS` of idle
            // (rails still up), then the usual power-off delay follows. An
            // event during the settle cancels it like any refresh; the pixels
            // stay dirty and the next refresh or idle settles them.
            Either::First(_) => {
                const ANALOG_IDLE_MS: u64 = 1000;
                let idle_power_off = async {
                    if let Some(ms) = display.settle_due_in_ms() {
                        Timer::after_millis(ms as u64).await;
                        let _ = display.settle().await;
                    }
                    Timer::after_millis(ANALOG_IDLE_MS).await;
                    let _ = display.deep_sleep().await;
                    core::future::pending::<core::convert::Infallible>().await
                };
                match select(
                    wait_display_event(button_rcvr, active_screen, &mut anim_state, true),
                    idle_power_off,
                )
                .await
                {
                    Either::First(advance) => advance,
                    Either::Second(never) => match never {},
                }
            }
            // Interrupted: redraw immediately without advancing the animation
            // frame (the sprite timer is suppressed above, so this is always a
            // button or signal).
            Either::Second(_) => false,
        };

        led::set_color(led::Rgb::RED, Duration::from_millis(50));

        anim_state.advance_sprite(sprite_advance);
    }
}

// SSD1680 SPI3 interrupt binding. The SSD1675 path binds its own handler in
// `fw::epd`, which is not compiled on the 1680 build, so there is no conflict.
#[cfg(feature = "ssd1680-driver")]
embassy_nrf::bind_interrupts!(struct Irqs1680 {
    SPIM3 => embassy_nrf::spim::InterruptHandler<embassy_nrf::peripherals::SPI3>;
});

/// Halve every group's RP (repeat) byte in a 153-byte SSD1680 waveform LUT,
/// leaving TP drive-frame counts untouched — "fewer shake repeats, same drive".
///
/// Timing block = bytes 60..=143, 7 per group `[TP_A, TP_B, SR_AB, TP_C, TP_D,
/// SR_CD, RP]`; RP of group `g` is byte `66 + 7*g` (g = 0..11). Halving RP keeps
/// each phase's drive length (TP) but runs it half as many times.
#[cfg(feature = "ssd1680-driver")]
fn halve_shake_rp_1680(mut lut: [u8; 153]) -> [u8; 153] {
    for g in 0..12usize {
        lut[66 + 7 * g] /= 2;
    }
    lut
}

/// Boot-probe the SSD1680's OTP full-refresh waveform (register `0x32`, 153 B).
///
/// Loads the temperature-appropriate OTP band into the controller's LUT RAM
/// (`0x22 = 0xB1`: LoadTemp | LoadOTP | Mode1, then `0x20`), then reads it back
/// over cmd `0x33`: `0x33` is clocked out on MOSI (TX), the same data line is
/// flipped to input (`new_rxonly`) to clock the reply in, then TX-only is
/// restored — the single-wire read trick used by `fw::epd::probe_lut` on the
/// SSD1675. Pins are stolen through the `board!` map and the GPIO wrappers `mem::forget`'d
/// so the real display Spim re-owns them.
///
/// ⚠ UNVALIDATED on this panel: the raw bytes are logged over defmt so the OTP
/// structure can be confirmed and the load/read sequence fixed on hardware if
/// the reply comes back empty/garbage.
#[cfg(feature = "ssd1680-driver")]
async fn probe_otp_full_lut_1680() -> [u8; 153] {
    use embassy_nrf::gpio::{Input, Level, Output, OutputDrive, Pull};
    use embassy_nrf::spim::{Config, Frequency, Spim};

    // EPD pins come from the shared `board!` map; each use steals a fresh
    // handle, as the SPI3 `Spim`s below are built and dropped repeatedly.
    macro_rules! sp {
        () => {
            unsafe { embassy_nrf::Peripherals::steal() }
        };
    }

    let mut cs = Output::new(board!(sp!(), epd_csn), Level::High, OutputDrive::Standard);
    let mut dc = Output::new(board!(sp!(), epd_dc), Level::Low, OutputDrive::Standard);
    let mut rst = Output::new(board!(sp!(), epd_reset), Level::Low, OutputDrive::Standard);
    let busy = Input::new(board!(sp!(), epd_busy), Pull::Down);

    let mut cfg = Config::default();
    cfg.frequency = Frequency::M16;

    // Hardware reset — flat 100 ms settle.
    Timer::after_millis(10).await;
    rst.set_high();
    Timer::after_millis(100).await;

    // Phase 1: soft reset, select the internal temp sensor, load the OTP band.
    cs.set_low();
    {
        let mut tx = Spim::new_txonly(
            board!(sp!(), epd_spi),
            Irqs1680,
            board!(sp!(), epd_sck),
            board!(sp!(), epd_mosi),
            cfg.clone(),
        );
        dc.set_low();
        tx.write(&[0x12]).await.ok(); // SoftReset
        dc.set_high();
        core::mem::forget(tx);
    }
    cs.set_high();
    for _ in 0..100u8 {
        if !busy.is_high() {
            break;
        }
        Timer::after_millis(10).await;
    }
    cs.set_low();
    {
        let mut tx = Spim::new_txonly(
            board!(sp!(), epd_spi),
            Irqs1680,
            board!(sp!(), epd_sck),
            board!(sp!(), epd_mosi),
            cfg.clone(),
        );
        dc.set_low();
        tx.write(&[0x18]).await.ok(); // TempSensorControl
        dc.set_high();
        tx.write(&[0x80]).await.ok(); // internal sensor
        dc.set_low();
        tx.write(&[0x22]).await.ok(); // UpdateDisplayOption2
        dc.set_high();
        tx.write(&[0xB1]).await.ok(); // LoadTemp | LoadOTP | Mode1
        dc.set_low();
        tx.write(&[0x20]).await.ok(); // MasterActivation
        core::mem::forget(tx);
    }
    cs.set_high();
    for _ in 0..100u8 {
        if !busy.is_high() {
            break;
        }
        Timer::after_millis(10).await;
    }

    // Phase 2: read 153 bytes from the LUT register (cmd 0x33).
    let mut lut = [0u8; 153];
    cs.set_low();
    {
        let mut tx = Spim::new_txonly(
            board!(sp!(), epd_spi),
            Irqs1680,
            board!(sp!(), epd_sck),
            board!(sp!(), epd_mosi),
            cfg.clone(),
        );
        dc.set_low();
        tx.write(&[0x33]).await.ok();
        dc.set_high();
        core::mem::forget(tx);
    }
    {
        // Same data pin, now clocked as input.
        let mut rx = Spim::new_rxonly(
            board!(sp!(), epd_spi),
            Irqs1680,
            board!(sp!(), epd_sck),
            board!(sp!(), epd_mosi),
            cfg.clone(),
        );
        rx.read(&mut lut).await.ok();
        drop(rx);
    }
    cs.set_high();

    // Restore SPI3 to TX-only so the display Spim built next can transmit.
    {
        let restore = Spim::new_txonly(
            board!(sp!(), epd_spi),
            Irqs1680,
            board!(sp!(), epd_sck),
            board!(sp!(), epd_mosi),
            cfg,
        );
        core::mem::forget(restore);
    }

    defmt::info!("1680 OTP full LUT read-back (153 B):");
    for (i, chunk) in lut.chunks(16).enumerate() {
        defmt::info!("  [{=usize:03}] {=[u8]:02x}", i * 16, chunk);
    }

    // Keep the GPIO pin config — the real display Output/Input own these next.
    core::mem::forget(cs);
    core::mem::forget(dc);
    core::mem::forget(rst);
    core::mem::forget(busy);

    lut
}

/// Background task owning the Qwiic bus (TWISPI0).  Polls the optional I2C
/// keyboard continuously while a text-entry screen is open — injecting keys
/// straight into the entry buffer — independent of the (slow) e-paper display
/// loop (which is on SPI3), so a wedged I2C read can never stall the display.
/// Also runs the Qwiic Scan when that screen requests it.  No key buffer: keys
/// go straight into the existing text_entry buffer.
#[embassy_executor::task]
async fn keyboard_task(mut bus: bornhack_aegg::fw::qwiic::QwiicBus<'static>) {
    use bornhack_aegg::fw::{i2c_keyboard, qwiic};
    use bornhack_aegg::text_entry;

    let mut present = false;
    let mut active = false;
    let mut prev = [0u8; 5];
    loop {
        // The Qwiic Scan screen borrows the bus while it is shown.
        if qwiic::is_active() {
            if qwiic::take_scan_pending() {
                qwiic::run_scan(&mut bus).await;
            }
            active = false;
            Timer::after_millis(100).await;
            continue;
        }

        let te = text_entry::is_active();
        if te && !active {
            // Entry just opened — probe for the keyboard, light LED A, and
            // tell the entry screen to show the keyboard hint if present.
            present = i2c_keyboard::present(&mut bus).await;
            prev = [0u8; 5];
            if present {
                i2c_keyboard::set_led_a(&mut bus, true).await;
            }
            text_entry::set_keyboard_active(present);
            active = true;
        } else if !te && active {
            if present {
                i2c_keyboard::set_led_a(&mut bus, false).await;
            }
            text_entry::set_keyboard_active(false);
            active = false;
        }

        if te
            && present
            && let Some(m) = i2c_keyboard::read_matrix(&mut bus).await
        {
            let mut keys: heapless::Vec<text_entry::ExtKey, 8> = heapless::Vec::new();
            i2c_keyboard::newly_pressed(&m, &prev, &mut keys);
            prev = m;
            if !keys.is_empty() {
                for k in keys {
                    text_entry::inject(k);
                }
                // Wake the display loop now, interrupting any in-flight refresh,
                // so the new text redraws immediately — the same mechanism a
                // joystick move uses (see KBD_REDRAW).
                KBD_REDRAW.signal(());
            }
        }

        Timer::after_millis(15).await;
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
    anim_state: &mut AnimState,
    // When false the sprite frame timer is suppressed (treated as `pending`)
    // so this waiter only returns on real interrupts (buttons/signals). Used
    // while a refresh is in flight so the frame timer can't cancel it.
    allow_sprite: bool,
) -> bool {
    use embassy_futures::select::{Either, Either3, select, select3};

    #[cfg(not(feature = "game"))]
    let _ = &anim_state;
    // Away from the pet screen the frame deadline lapses; clear it so coming
    // back does not fire a stale frame (and cancel the refresh) at once. Not
    // while `allow_sprite` is false: that is a drive in flight.
    #[cfg(feature = "game")]
    if allow_sprite && active_screen != bornhack_aegg::SCREEN_GAME {
        anim_state.pacer.on_frame();
    }
    #[cfg(feature = "game")]
    let sprite_active = allow_sprite
        && active_screen == bornhack_aegg::SCREEN_GAME
        && (bornhack_aegg::game::sprite_loader::frame_count() > 0
            || bornhack_aegg::game::lifecycle::is_started());
    #[cfg(not(feature = "game"))]
    let _ = allow_sprite;

    loop {
        // Idle animations advance one frame per 5 minutes; hatching and
        // actions spread their frames over their duration. The frame
        // deadline is absolute (FramePacer), recomputed on every pass, and
        // the wait always ends at the engine wake or UI deadline too. Pure
        // reads of the game only: nothing here updates the engine.
        #[cfg(feature = "game")]
        let sprite_plan = if sprite_active {
            use bornhack_aegg::game::{lifecycle, pacing};
            let anim = lifecycle::display_anim();
            let kind = lifecycle::pet_kind();
            let frame_count =
                bornhack_aegg::game::engine::anim_files::frame_count(kind, anim) as u32;
            let frame_secs =
                pacing::frame_interval_secs(anim, frame_count, lifecycle::action_ticks_remaining());
            let wake_secs = pacing::wake_secs(
                lifecycle::next_wake_tick(),
                bornhack_aegg::game::next_ui_deadline_tick(),
                lifecycle::now_tick(),
            );
            let anim_id = bornhack_aegg::game::engine::anim_files::anim_id_for(anim);
            Some(anim_state.pacer.wait(
                embassy_time::Instant::now().as_secs(),
                anim_id,
                frame_secs,
                wake_secs,
            ))
        } else {
            None
        };
        #[cfg(feature = "game")]
        let sprite_tick = async {
            match sprite_plan {
                Some((wait_secs, frame_due)) => {
                    Timer::after_secs(wait_secs).await;
                    frame_due
                }
                None => core::future::pending::<bool>().await,
            }
        };
        #[cfg(not(feature = "game"))]
        let sprite_tick = core::future::pending::<bool>();

        // Compose button + TOAST_SIGNAL into a single wakeup so the outer
        // select shape stays unchanged.  Both result in a non-sprite
        // (`return false`) wake-up.
        let button_or_toast = async {
            use embassy_futures::select::{Either3, select3};
            // KBD_REDRAW fires when the I2C keyboard injects keys.  It is NOT
            // gated on `allow_sprite`, so a keystroke interrupts an in-flight
            // e-paper refresh and forces an immediate redraw with the new text
            // — the same way `button_rcvr.changed()` (joystick / buttons) does.
            // That mid-refresh interrupt is what makes typing feel responsive
            // instead of blocking on each ~500 ms whole-panel refresh.
            match select3(
                button_rcvr.changed(),
                bornhack_aegg::TOAST_SIGNAL.wait(),
                KBD_REDRAW.wait(),
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
                Either::Second(Either4::First(frame_due)) => return frame_due, // sprite timer; false = wake
                Either::First(Either3::First(_))   => return false, // button / toast
                Either::First(Either3::Second(_))  => return false, // BLE pairing
                Either::First(Either3::Third(_))                    // minute tick
                    if active_screen == SCREEN_MAIN => return false,
                Either::First(Either3::Third(_))                    // minute tick
                    if active_screen == SCREEN_WATCH => return false,
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
            Either::Second(frame_due)          => return frame_due, // sprite timer; false = wake
            Either::First(Either3::First(_))   => return false, // button / toast
            Either::First(Either3::Second(_))  => return false, // BLE pairing
            Either::First(Either3::Third(_))                    // minute tick
                if active_screen == SCREEN_MAIN => return false,
            Either::First(Either3::Third(_))                    // minute tick
                if active_screen == SCREEN_WATCH => return false,
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

/// Periodic BornPets friend-discovery beacon on the private SHDW channel.
///
/// Wakes every 15 minutes; while a pet is active, broadcasts a `PetBeacon`
/// (identity + pet snapshot + current combat stats) as `GrpData` so other
/// badges on SHDW can record a new/returning friend — see
/// `game::friends::on_pet_beacon` — and cache combat stats fresh enough
/// to battle against later (`game::battle_link::challenge`).
/// Skipped entirely when no pet is active (nothing to announce).
#[cfg(all(feature = "mesh", feature = "game"))]
#[embassy_executor::task]
async fn pet_beacon_ticker_task() {
    use bornhack_aegg::game::friends;

    const PET_BEACON_INTERVAL_SECS: u64 = 15 * 60;

    // Brief delay on boot so the radio and mesh stack are up before our
    // first TX — same rationale as `advert_ticker_task` above.
    Timer::after(embassy_time::Duration::from_secs(45)).await;

    loop {
        if let Some(beacon) = friends::beacon_now() {
            let mut data: heapless::Vec<u8, { bornhack_aegg::fw::mesh::MAX_CHANNEL_DATA }> =
                heapless::Vec::new();
            let _ = data.extend_from_slice(&beacon.to_bytes());

            let _ =
                bornhack_aegg::fw::mesh::tx_send(bornhack_aegg::fw::mesh::TxRequest::ChannelData(
                    bornhack_aegg::fw::mesh::TxChannelData {
                        channel_idx: bornhack_aegg::fw::mesh::channels::SHDW_SLOT,
                        data_type: friends::PET_BEACON_TYPE,
                        // Zero-hop (RouteType::Direct, empty path): friends are
                        // discovered only among badges in direct radio range, and
                        // beacons never flood the wider mesh. Not OUT_PATH_UNKNOWN
                        // (0xFF), which would select flood routing.
                        path_len: 0,
                        path: heapless::Vec::new(),
                        data,
                    },
                ));
        }

        Timer::after(embassy_time::Duration::from_secs(PET_BEACON_INTERVAL_SECS)).await;
    }
}

/// Sleeps until the challenge Session's own deadline, then lets it act (the
/// target's window closing, the challenger's fallback fight). A one-shot
/// timer set to that single instant, re-armed when the deadline moves; with
/// no challenge pending it waits on the signal and costs nothing.
#[cfg(feature = "game")]
#[embassy_executor::task]
async fn battle_timer_task() {
    use bornhack_aegg::game::battle_link;
    use embassy_futures::select::{Either, select};

    loop {
        match battle_link::deadline_ms() {
            None => battle_link::DEADLINE_CHANGED.wait().await,
            Some(t) => {
                let wait = battle_link::ms_until(t, bornhack_aegg::platform::uptime_ms_u32());
                let timer = Timer::after(embassy_time::Duration::from_millis(wait as u64));
                if let Either::First(_) = select(timer, battle_link::DEADLINE_CHANGED.wait()).await
                {
                    battle_link::on_deadline();
                }
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
async fn show_battery_critical<D>(display: &mut D, err: &battery::BatteryError)
where
    D: bornhack_aegg::epd_driver::EpdDriver,
{
    use bornhack_aegg::EpdColor as Color;
    use embedded_graphics::prelude::*;
    use embedded_graphics::text::{Text};

    let _ = display.clear(Color::White);

    let centered = bornhack_aegg::ui::CENTERED;
    let font = bornhack_aegg::ui::TEXT_BOLD_BLACK;

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

    let _ = bornhack_aegg::fw::power::boosted(
        bornhack_aegg::fw::power::Source::Epd,
        bornhack_aegg::epd_driver::EpdDriver::refresh(
            display,
            bornhack_aegg::epd_driver::RefreshMode::Full,
        ),
    )
    .await;
}

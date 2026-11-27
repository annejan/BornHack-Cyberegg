//! Firmware library for the BornHack CyberÆgg badge — an nRF52840-based
//! e-paper / LoRa / BLE / NFC pet-and-mesh companion device.
//!
//! # Hardware target
//!
//! - **MCU** — Nordic nRF52840 (Cortex-M4F + radio + USB + NFC).
//! - **Display** — SSD1675 152×152 tri-color e-paper over SPI3.
//! - **LoRa radio** — Semtech SX1262 over SPI2, MeshCore protocol.
//! - **BLE** — softdevice via `nrf-sdc` / `trouble-host`.
//! - **NFC** — built-in NFCT controller; emulates an ISO 14443-A Type 4 tag
//!   with authenticated station commands (`signed_channel`).
//!
//! # Bin targets (`src/bin/`)
//!
//! - **`embassy`** — main firmware. Built with the `embassy*` features.
//! - **`simulator`** — desktop SDL2 UI simulator.  Built with `simulator`.
//! - **`simulate_game`** — headless game-balance simulator that runs all player
//!   profiles and prints a summary table matching the Python sim.
//! - **`hwtest`** — standalone factory hardware test.  Skips this library
//!   entirely (the `#![cfg]` gate below makes the lib content empty for it) so
//!   it links without dragging in the full graphics/menu stack.
//!
//! # Feature gates
//!
//! Compose to keep the flash budget under control on the 960 KiB app
//! partition:
//!
//! - `embassy-base` — async runtime, EPD driver, buttons, buzzer, kv, watch
//!   face, signed-channel NFC.  Always on for any firmware build.
//! - `embassy-watch` — `embassy-base` only (smallest fw configuration).
//! - `embassy-game` — `embassy-base` + virtual pet game + USB-MSC.
//! - `embassy-mesh` — `embassy-base` + LoRa mesh + BLE companion.
//! - `embassy` — full build = base + game + mesh + USB-MSC.
//! - `simulator` — host-side build with SDL2 rendering and `std`.
//! - `signed-channel` — Ed25519 challenge/response NFC station auth.
//!
//! # Module map
//!
//! - `fw` — driver-layer code: EPD, buzzer, battery ADC, button matrix, LEDs,
//!   NFC tag, LoRa radio, BLE companion, MeshCore plumbing, FAT12 reader,
//!   ekv-backed kv store, sponsors slideshow.  Embassy only.
//! - `game` — virtual-pet lifecycle (hunger / inspiration / health /
//!   tiredness), mini-games (black-hole, lights-out, tic-tac-toe,
//!   sprite engine), station bonuses, action-feedback toasts.  Gated by `game`.
//! - `watch` — Casio-style digital + analog clock face, alarms with per-day
//!   mask and weekly repeats, multi-slot alarm state.  Gated by `watch`.
//! - `menu` — declarative `MenuItem` / `MenuItemKind` items, the icon- grid
//!   `DisplayState`, and the scrolling 3-row menu renderer.  Always built.
//! - `text_entry` — full-screen quadrant-style on-screen keyboard for text
//!   input (node names, channel replies, etc.).
//! - `signed_channel` — Ed25519 challenge/response verification used by the NFC
//!   station-command flow.  Gated by `signed-channel`.
//! - `ui` — common drawing helpers (frame, layout constants).
//!
//! # Bootloader
//!
//! A custom USB-DFU bootloader (`nrf-aegg-bootloader`, in the
//! `bootloader/` directory) replaces the factory Adafruit UF2 stub.  It
//! lives at `0x00000000`–`0x0000FFFF` (64 KiB) and hands off to the app
//! at `0x00010000`.  The main app's `memory-fw.x` accordingly sets
//! `FLASH ORIGIN = 0x00010000` and `LENGTH = 960K`.  The bootloader is a
//! standalone Cargo project (tracked in git, but *not* part of this
//! workspace) — see the README for build steps.

// The library only has meaningful content when building the main firmware
// or simulator. Other binaries (e.g. `hwtest`) build against an empty
// library so their builds don't drag in the full graphics/menu stack.
#![cfg_attr(not(feature = "simulator"), no_std)]
#![cfg_attr(feature = "embassy-core", no_main)]
#![cfg(any(feature = "embassy-core", feature = "simulator"))]

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum ScreenError {
    NotFound,
    OutOfBounds,
    InvalidScreen,
}

#[cfg(feature = "embassy-core")]
pub mod fw;
/// Pure embedded-graphics emoji glyph rendering (no embassy/HAL deps); shared
/// by `ui::draw_text`, the `watch` face and the simulator.
pub mod emoji;
/// Screen lock — padlock overlay + lock state. The Cancel-hold toggle lives in
/// the firmware-only `fw::button`, so in the simulator the lock just stays
/// inactive unless toggled programmatically.
pub mod lock;
/// Companion text CLI — pure logic, compiled on the host so its unit tests run.
#[cfg(any(feature = "mesh", feature = "simulator"))]
pub mod mesh_cli;
#[cfg(feature = "game")]
pub mod game;
/// Unified EPD pixel colour (`EpdColor`; `TriColor` alias) shared by every panel
/// driver and the simulator.
pub mod color;
/// Panel-agnostic e-paper driver trait implemented by each controller adapter.
#[cfg(feature = "embassy-core")]
pub mod epd_driver;
pub mod button_seq;
pub mod cfg_text;
pub mod display_flush;
#[cfg(feature = "heapless")]
pub mod evict_table;
pub mod refresh_decision;
#[cfg(feature = "simulator")]
pub mod guide;
pub mod hex;
pub mod menu;
pub mod lut_file;
/// LoRa preamble and duty-cycled receive timing (pure; host-tested).
pub mod lora_timing;
pub mod name_screen;
pub mod nfc_ndef;
pub mod platform;
#[cfg(feature = "mesh")]
pub mod qr_screen;
#[cfg(feature = "signed-channel")]
pub mod signed_channel;
#[cfg(feature = "simulator")]
pub mod sim_support;
pub mod sync;
pub mod text_entry;
pub mod text_wrap;
pub mod ui;
#[cfg(feature = "watch")]
pub mod watch;
#[cfg(feature = "embassy-core")]
use core::cell::RefCell;
use core::result::Result;
use core::result::Result::Ok;
#[cfg(feature = "embassy-core")]
use core::sync::atomic::Ordering;
use core::sync::atomic::{AtomicBool, AtomicI8, AtomicU8, AtomicU32};

#[cfg(feature = "embassy-core")]
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Text};
#[cfg(feature = "embassy-core")]
use heapless::format;
pub use menu::{DISPLAY_STATE, DisplayState, MenuItem, MenuItemKind, ScreenState, draw_menu};
// Unified EPD pixel colour shared by every panel driver and the simulator.
// `TriColor` is a back-compat alias of `EpdColor`, so every existing
// `DrawTarget<Color = TriColor>` bound keeps resolving unchanged.
pub use color::{BLACK, EpdColor, RED, TriColor, WHITE};

// Conditional imports based on feature
#[cfg(feature = "embassy-core")]
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
#[cfg(feature = "embassy-core")]
use embassy_sync::signal::Signal;

/// Player-menu song indices into `crate::fw::buzzer::MELODIES`.
/// Defined here, not in `fw::buzzer`, so the `game::modal` music menu
/// can reference them on simulator builds (which don't link
/// `fw::buzzer`) without re-declaring the values.  `fw::buzzer` keeps
/// its `MELODIES` array in this order; reorder one and reorder the
/// other.
pub const SONG_STARTUP_INDEX: u8 = 0;
pub const SONG_RICKROLL_INDEX: u8 = 1;
pub const SONG_IMPERIAL_MARCH_INDEX: u8 = 2;
pub const SONG_SANDSTORM_INDEX: u8 = 3;
pub const SONG_PINK_PANTHER_INDEX: u8 = 4;
pub const SONG_TROLOLO_INDEX: u8 = 5;
// Indices 6, 7, 8 — system-only sounds (`PET_WARN`, `FUNNY_ENDING`,
// `ALARM`).  Not exposed in the player music menu, but every site
// that triggers them references these constants by name.  Defined
// here (not in `fw::buzzer`) for the same reason as the SONG_*
// constants — the simulator build doesn't link `fw::buzzer`.
pub const PET_WARN_INDEX: usize = 6;
pub const FUNNY_ENDING_INDEX: usize = 7;
pub const ALARM_INDEX: usize = 8;
pub const SONG_DAISY_BELL_INDEX: u8 = 9;
pub const SONG_NOKIA_INDEX: u8 = 10;
pub const SONG_OVER_THE_HORIZON_INDEX: u8 = 11;
/// Mini-game victory jingle — system-only, not exposed in the music menu.
pub const MINIGAME_WIN_INDEX: usize = 12;

/// Boosted RX gain toggle (0x96 vs 0x94 in register 0x08AC). Default: on,
/// matching MeshCore 1.15.0 (upstream commit `ff5aad71`).  Boot path
/// overwrites this from persisted KV via [`fw::mesh::settings::get_boost_rx`].
pub static BOOSTED_RX_GAIN: AtomicBool = AtomicBool::new(true);

/// Receive power save: duty-cycled RX sized for MeshCore 1.16+ preambles
/// ([`lora_timing::rx_power_save_periods`]). Default: off (continuous RX).
/// Boot path overwrites this from persisted KV via
/// [`fw::mesh::settings::get_rx_power_save`]; the SX1262 driver reads it every
/// time it re-arms receive.
pub static RX_POWER_SAVE: AtomicBool = AtomicBool::new(false);

/// UTC offset in whole hours (-12..=+14).  Default: +2 (Europe/Copenhagen
/// summer time / CEST) — matches Bornhack's typical July/August venue.
/// User can override via Settings → Timezone; the kv-stored value loads
/// at boot and shadows this default, so changing this only affects fresh
/// badges that have never had a timezone set.
///
/// Stored in quarter hours (UTC+2 = 8, UTC+5:30 = 22) so the half- and
/// quarter-hour zones MeshCore's `set tz.offset` accepts since 1.18 fit.
pub static TZ_OFFSET_QH: core::sync::atomic::AtomicI8 = core::sync::atomic::AtomicI8::new(8);
/// Lowest offset, UTC-12, in quarter hours.
pub const TZ_OFFSET_QH_MIN: i8 = -48;
/// Highest offset, UTC+14, in quarter hours.
pub const TZ_OFFSET_QH_MAX: i8 = 56;

/// The local-time offset from UTC in seconds.
pub fn tz_offset_secs() -> i64 {
    TZ_OFFSET_QH.load(core::sync::atomic::Ordering::Relaxed) as i64 * 900
}

/// Fired when `TZ_OFFSET_QH` changes so the BLE task can persist it.
#[cfg(feature = "embassy-core")]
pub static TZ_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// LoRa radio parameters stored on-device.
///
/// Serialisation to/from the 12-byte `"settings:radio"` KV record lives in
/// [`fw::mesh::settings`], which re-exports this type.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub struct RadioParams {
    /// Carrier frequency in Hz (e.g. 869_618_000).
    pub freq_hz: u32,
    /// Bandwidth in Hz (e.g. 62_500).
    pub bw_hz: u32,
    /// Spreading factor (5–12).
    pub sf: u8,
    /// Coding rate — 5 = 4/5, 6 = 4/6, 7 = 4/7, 8 = 4/8.
    pub cr: u8,
    /// TX power in dBm.
    pub tx_power: i8,
    /// Client-repeat mode enabled.
    pub client_repeat: bool,
}

/// Default radio parameters — **EU/UK Narrow**, the stock MeshCore channel
/// (matches `fw::mesh::sx1262::MeshCoreConfig::UK_NARROW_BAND`).
///
/// 869.618 MHz · BW 62.5 kHz · SF8 · CR 4/5 · 22 dBm TX
///
/// Out-of-the-box the badge shares airtime with stock MeshCore badges on the
/// standard EU/UK Narrow channel.  TX power defaults to 22 dBm (the SX1262
/// maximum) — the antenna is not fully efficient, so radiated power stays
/// within the band limit; trim it via the Power menu if needed.
///
/// Coding rate uses **MeshCore protocol encoding**: 5 = CR 4/5, 6 = CR 4/6,
/// etc. (distinct from the sx126x hardware register encoding where CR4_5 = 1).
///
/// Single source of truth: the `LORA_*` atomics below are seeded from it, and
/// `fw::mesh::settings::get_radio_params_or_default()` falls back to it when
/// flash holds no stored record.
pub const DEFAULT_RADIO: RadioParams = RadioParams {
    freq_hz: 869_618_000,
    bw_hz: 62_500,
    sf: 8,
    cr: 5, // CR 4/5 in MeshCore protocol encoding
    tx_power: 22,
    client_repeat: false,
};

/// Current LoRa radio parameters exposed as atomics so the menu can read them
/// synchronously. Populated on boot from flash and kept in sync with
/// `settings::get_radio_params_or_default()`; seeded from [`DEFAULT_RADIO`].
pub static LORA_FREQ_HZ: AtomicU32 = AtomicU32::new(DEFAULT_RADIO.freq_hz);
pub static LORA_BW_HZ: AtomicU32 = AtomicU32::new(DEFAULT_RADIO.bw_hz);
pub static LORA_SF: AtomicU8 = AtomicU8::new(DEFAULT_RADIO.sf);
pub static LORA_CR: AtomicU8 = AtomicU8::new(DEFAULT_RADIO.cr);
/// LoRa TX power in dBm (−9..=22, matches the companion validation range).
pub static LORA_TX_POWER: AtomicI8 = AtomicI8::new(DEFAULT_RADIO.tx_power);
/// Client-repeat mode — re-transmit received flood packets back onto the mesh.
/// Menu-togglable; runtime relay behavior is wired up via the BLE task.
pub static LORA_CLIENT_REPEAT: AtomicBool = AtomicBool::new(DEFAULT_RADIO.client_repeat);

impl RadioParams {
    /// Snapshot of the live `LORA_*` atomics.
    pub fn load_live() -> Self {
        use core::sync::atomic::Ordering::Relaxed;
        Self {
            freq_hz: LORA_FREQ_HZ.load(Relaxed),
            bw_hz: LORA_BW_HZ.load(Relaxed),
            sf: LORA_SF.load(Relaxed),
            cr: LORA_CR.load(Relaxed),
            tx_power: LORA_TX_POWER.load(Relaxed),
            client_repeat: LORA_CLIENT_REPEAT.load(Relaxed),
        }
    }

    /// Write every field to the live `LORA_*` atomics. Does not signal the
    /// persister or the radio; callers fire `LORA_RADIO_CHANGED_SIGNAL`
    /// themselves when needed.
    pub fn store_live(&self) {
        use core::sync::atomic::Ordering::Relaxed;
        LORA_FREQ_HZ.store(self.freq_hz, Relaxed);
        LORA_BW_HZ.store(self.bw_hz, Relaxed);
        LORA_SF.store(self.sf, Relaxed);
        LORA_CR.store(self.cr, Relaxed);
        LORA_TX_POWER.store(self.tx_power, Relaxed);
        LORA_CLIENT_REPEAT.store(self.client_repeat, Relaxed);
    }
}

/// `OtherParams.advert_loc_policy` — share this node's position in adverts.
pub static ADVERT_LOC_POLICY: AtomicBool = AtomicBool::new(false);
/// `OtherParams.multi_acks` — number of aggregated ACKs (1 or 2).
pub static MULTI_ACKS: AtomicU8 = AtomicU8::new(1);
/// Telemetry-base permission for incoming on-air `REQ_TYPE_GET_TELEMETRY_DATA`
/// requests, matching upstream MeshCore `_prefs.telemetry_mode_base`:
///
/// - `0` — `TELEM_MODE_DENY`: drop every request.
/// - `1` — `TELEM_MODE_ALLOW_FLAGS`: respond only when the requester's stored
///   `Contact.flags` has bit 1 set (per-contact opt-in).
/// - `2` — `TELEM_MODE_ALLOW_ALL`: respond to every authenticated request.
///
/// Loc/env modes stay 0 — the badge has no GPS or environment sensors. The
/// 6-bit packed wire value emitted via `OtherParams.telemetry_mode` is
/// `(0 << 4) | (0 << 2) | base`. BLE/companion self-telemetry reads bypass
/// this gate entirely.
pub static TELEMETRY_MODE_BASE: AtomicU8 = AtomicU8::new(0);

/// Periodic self-advert scheduling — driven by the `advert_ticker_task` in
/// `bin/embassy.rs`.
pub static ADVERT_ENABLED: AtomicBool = AtomicBool::new(true);
pub static ADVERT_INTERVAL_HOURS: AtomicU8 = AtomicU8::new(16);

/// Fired by the menu when advert scheduling changes (toggle or interval).
/// The BLE task persists the new config; the advert ticker task wakes up and
/// re-reads the interval for its next sleep.
#[cfg(feature = "embassy-core")]
pub static ADVERT_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired when the menu or BLE companion changes the LoRa radio params so
/// the persister task can write them to flash.  Single-consumer: only
/// `persister::lora_radio_loop` waits on this.  After the persister
/// finishes the flash write it fans out to `LORA_RADIO_APPLY_SIGNAL` so
/// the listener task can reprogram the SX1262 live (no reboot needed).
#[cfg(feature = "embassy-core")]
pub static LORA_RADIO_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by `persister::lora_radio_loop` after a successful flash write
/// of the new radio params or the boost-RX flag.  Consumed by `meshcore::run_meshcore_listener`,
/// which puts the SX1262 into standby, calls `reconfigure_radio`, and
/// resumes RX.  Decoupling this from `LORA_RADIO_CHANGED_SIGNAL` is
/// required because `embassy_sync::signal::Signal` only wakes a single
/// waiter per signal — having two consumers race for the same signal
/// is how the persister kept winning and the listener never reprogrammed.
#[cfg(feature = "embassy-core")]
pub static LORA_RADIO_APPLY_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired when the menu changes `OtherParams` fields (advert_loc / multi_acks)
/// so the BLE task can persist them.
#[cfg(feature = "embassy-core")]
pub static OTHER_PARAMS_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired when the menu changes `PATH_HASH_MODE`.
#[cfg(feature = "embassy-core")]
pub static PATH_HASH_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu's Factory Reset action — wipes the entire KV store and
/// resets the device.
#[cfg(feature = "embassy-core")]
pub static FACTORY_RESET_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired when the menu's text entry submits a new node name.
#[cfg(feature = "embassy-core")]
pub static NODE_NAME_CHANGED_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// One-shot request for the next EPD refresh to use the slow full
/// tri-colour waveform (`RefreshMode::Full`) instead of the fast partial
/// update (`RefreshMode::Fast`).  The display loop in `embassy.rs`
/// `swap`s the flag back to `false` after consuming it, so a mini-game
/// can simply set this to `true` on close to clear residual ghosting
/// from its many fast updates in one full-cycle refresh.
pub static FULL_REFRESH_PENDING: AtomicBool = AtomicBool::new(false);

/// One-shot request for the display loop to run a full black → white →
/// redraw flush cycle before its next normal draw — a deliberate,
/// user-triggered de-ghost distinct from `FULL_REFRESH_PENDING`'s "just
/// mark the current content dirty". Set by the hidden button sequence in
/// [`display_flush`]; consumed (swapped back to `false`) by the
/// display loop in `embassy.rs`.
pub static FORCE_FLUSH_PENDING: AtomicBool = AtomicBool::new(false);

/// When true, #blinkme channel LED commands are ignored.
pub static IGNORE_BLINK: AtomicBool = AtomicBool::new(false);

/// When true, incoming region-scoped (TransportFlood) packets whose transport
/// code does not match our flood-scope key are dropped.  When false (default)
/// they are still shown, but never relayed by client-repeat.  Toggled from the
/// MeshCore → Region filter menu entry.
pub static REGION_FILTER: AtomicBool = AtomicBool::new(false);

/// When true, the pet severity-change buzzer alert is suppressed.
/// Toggled from the Bornagotchi → Mute menu entry.
pub static GAME_MUTE: AtomicBool = AtomicBool::new(false);

/// When true, `bin/embassy::main` plays the Startup melody once after
/// boot init finishes.  Default `true` — the badge boots quietly only
/// if the user has explicitly turned it off in Settings → Boot chime.
/// Loaded from the `"watch"` kv namespace at boot; menu toggles signal
/// `watch::SETTINGS_DIRTY_SIGNAL` so the existing watch persister task
/// writes the new value to flash.
pub static BOOT_CHIME_ENABLED: AtomicBool = AtomicBool::new(true);

/// When true, the LoRa radio is put into standby and the meshcore task
/// pauses all RX/TX until re-enabled.
pub static LORA_DISABLED: AtomicBool = AtomicBool::new(false);

/// Fired when `LORA_DISABLED` changes so the meshcore task wakes up.
#[cfg(feature = "embassy-core")]
pub static LORA_DISABLED_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// When true, the BLE task stops advertising and waits until re-enabled.
pub static BLE_DISABLED: AtomicBool = AtomicBool::new(false);

/// Fired when `BLE_DISABLED` changes, waking the BLE task out of its
/// disabled-wait loop or persisting the new state.
#[cfg(feature = "embassy-core")]
pub static BLE_DISABLED_CHANGED: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired by the menu to clear all stored BLE bond/pairing data and reboot.
#[cfg(feature = "embassy-core")]
pub static CLEAR_BONDS_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// Re-export mesh types and statics so existing `crate::SomeType` paths keep
// working.
#[cfg(feature = "mesh")]
pub use fw::mesh::*;

/// Active BLE pairing passkey (6-digit, 0–999999). `u32::MAX` means no pairing
/// in progress.
pub static BLE_PASSKEY: AtomicU32 = AtomicU32::new(u32::MAX);

/// Set to `true` while a BLE companion is connected, `false` on disconnect.
pub static BLE_CONNECTED: AtomicBool = AtomicBool::new(false);

/// Set to `true` when an unread PM arrives; cleared when the PM screen is
/// viewed.
pub static PM_UNREAD: AtomicBool = AtomicBool::new(false);

/// Fired by the BLE task whenever the pairing passkey changes (new passkey or
/// cleared).
#[cfg(feature = "embassy-core")]
pub static BLE_PAIRING_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired when something off-screen needs the display to redraw — e.g.
/// `game::show_toast` posting a station bonus from the NFC task.
/// The display loop wakes on this and the active screen renderer
/// picks up the new state.
#[cfg(feature = "embassy-core")]
pub static TOAST_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fired every minute by `minute_tick_task` so the display redraws the clock.
#[cfg(feature = "embassy-core")]
pub static MINUTE_TICK: Signal<CriticalSectionRawMutex, ()> = Signal::new();

// ---------------------------------------------------------------------------
// Wall clock
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-core")]
struct WallClock {
    unix_base: u32,
    ticks_base: u64,
}

#[cfg(feature = "embassy-core")]
static WALL_CLOCK: Mutex<CriticalSectionRawMutex, RefCell<Option<WallClock>>> =
    Mutex::new(RefCell::new(None));

/// Called by the BLE task when `SET_DEVICE_TIME` (0x06) is received,
/// and by the on-air clock-seeding path (`fw::mesh::repeater_time`)
/// for both the initial seed and later refinements.  Does NOT latch
/// `BLE_TIME_LOCKED` on its own — the BLE caller sets that flag
/// explicitly after calling here.
#[cfg(feature = "embassy-core")]
pub fn set_wall_clock(unix_secs: u32) {
    WALL_CLOCK.lock(|cell| {
        *cell.borrow_mut() = Some(WallClock {
            unix_base: unix_secs,
            ticks_base: embassy_time::Instant::now().as_ticks(),
        });
    });
    // First clock of the boot: let the pet catch up on the off-time.
    game::lifecycle::on_wall_clock_set(unix_secs);
}

/// Latched once the BLE companion has set the wall clock via
/// `SET_DEVICE_TIME` (0x06).  The on-air seeder checks this and
/// stops refining once true — BLE is authoritative.  Never cleared
/// until reboot — a BLE disconnect does NOT re-enable on-air
/// refinement.
#[cfg(feature = "embassy-core")]
pub static BLE_TIME_LOCKED: AtomicBool = AtomicBool::new(false);

/// Current unix time in seconds, or `None` if the clock has never been synced.
#[cfg(feature = "embassy-core")]
pub fn unix_now() -> Option<u32> {
    WALL_CLOCK.lock(|cell| {
        cell.borrow().as_ref().map(|wc| {
            let elapsed = embassy_time::Instant::now()
                .as_ticks()
                .saturating_sub(wc.ticks_base);
            wc.unix_base
                .saturating_add((elapsed / embassy_time::TICK_HZ) as u32)
        })
    })
}

/// Like [`unix_now`], but strictly increasing across calls (the reference's
/// `getCurrentTimeUnique`), so back-to-back CLI commands never reuse a
/// timestamp and trip the receiver's replay protection.
#[cfg(feature = "embassy-core")]
pub fn unix_now_unique() -> Option<u32> {
    static LAST_UNIQUE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    let now = unix_now()?;
    let prev = LAST_UNIQUE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
            Some(if now <= last { last + 1 } else { now })
        })
        .unwrap_or(0);
    Some(if now <= prev { prev + 1 } else { now })
}

/// Maximum node-name length in bytes (MeshCore `node_name[32] - 1`).
pub const MAX_NODE_NAME: usize = 31;

/// MeshCore node name cached from KV for synchronous access by the display
/// renderer. Populated by the BLE task at startup (after reading from flash)
/// and on every SET_ADVERT_NAME update.  Empty until the BLE task has
/// initialized.
pub static NODE_NAME: sync::SharedCell<heapless::String<MAX_NODE_NAME>> =
    sync::SharedCell::new(heapless::String::new());

/// This badge's Ed25519 public key, populated once at boot by
/// `bin/embassy.rs` after `device_identity::load_or_create()` completes.
/// Read by the "My QR" screen to build the meshcore contact URL; all zeros
/// before mesh init runs (the QR screen treats that as "key not ready yet"
/// and shows a placeholder).
#[cfg(all(any(feature = "embassy-core", feature = "simulator"), feature = "mesh"))]
pub static MY_PUB_KEY: sync::SharedCell<[u8; 32]> = sync::SharedCell::new([0u8; 32]);

/// Lowercase hex of `bytes` (`n_bytes` of them) into a `String<32>`.
/// Used to render pub_key prefixes (`pub_key[..8]` → 16-char hex) and
/// to format short fingerprints for the Contacts/PM screens.  Caps
/// internally so the output never exceeds 16 bytes' worth of hex (32
/// chars); pass smaller `n_bytes` for shorter fingerprints.
pub fn hex_prefix(bytes: &[u8], n_bytes: usize) -> heapless::String<32> {
    let mut out: heapless::String<32> = heapless::String::new();
    let take = n_bytes.min(bytes.len()).min(16);
    hex::push_lower(&mut out, &bytes[..take]);
    out
}

/// Truncate a UTF-8 string to fit within `max_bytes` on a char boundary.
pub fn truncate_str(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Store `name` (raw UTF-8 bytes) into [`NODE_NAME`].  Invalid UTF-8 is
/// ignored.
pub fn update_node_name(name: &[u8]) {
    if let Ok(s) = core::str::from_utf8(name) {
        NODE_NAME.with_mut(|stored| {
            stored.clear();
            let _ = stored.push_str(truncate_str(s, 31));
        });
    }
}

// (mesh statics moved to fw/mesh/mod.rs)

/// Access the shared `DisplayState` immutably.
/// Usage: `with_display_state!(|s| s.active_screen())`
#[macro_export]
macro_rules! with_display_state {
    ($f:expr) => {
        DISPLAY_STATE.with(|state| {
            let f: &dyn Fn(&$crate::menu::DisplayState<{ $crate::menu::SCREEN_COUNT }>) -> _ = &$f;
            f(state)
        })
    };
}

/// Access the shared `DisplayState` mutably.
/// Usage: `with_display_state_mut!(|s| s.dispatch_button(btn))`
#[macro_export]
macro_rules! with_display_state_mut {
    ($f:expr) => {
        DISPLAY_STATE.with_mut(|state| {
            let f: &dyn Fn(&mut $crate::menu::DisplayState<{ $crate::menu::SCREEN_COUNT }>) -> _ =
                &$f;
            f(state)
        })
    };
}

// Position of the animated circle

// Re-export screen indices from ScreenId for convenience.
// The game screen is always at index 0 but disabled when the game feature is
// off. Navigation automatically skips disabled screens.
pub use menu::ScreenId;
pub const SCREEN_GAME: u8 = ScreenId::Game as u8;
pub const SCREEN_MAIN: u8 = ScreenId::Main as u8;
pub const SCREEN_PM: u8 = ScreenId::Pm as u8;
pub const SCREEN_CHANNEL: u8 = ScreenId::Channel as u8;
pub const SCREEN_ADVERT: u8 = ScreenId::Advert as u8;
pub const SCREEN_WATCH: u8 = ScreenId::Watch as u8;
pub const SCREEN_CALENDAR: u8 = ScreenId::Calendar as u8;
pub const SCREEN_NAME: u8 = ScreenId::Name as u8;
pub const SCREEN_QR: u8 = ScreenId::Qr as u8;

/// Dispatch to the correct screen renderer based on the active screen.
pub fn draw_graphics<D>(display: &mut D, health_str: &str, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Text entry: full-screen text input takes priority over all screens.
    if text_entry::is_active() {
        return text_entry::TEXT_ENTRY.with(|entry| {
            if let Some(entry) = entry {
                text_entry::draw_text_entry(display, entry)
            } else {
                Ok(())
            }
        });
    }

    // Qwiic Scan: full-screen I2C bus-scanner overlay.
    #[cfg(feature = "embassy-core")]
    if fw::qwiic::is_active() {
        return fw::qwiic::draw(display);
    }

    // Unicorn Realm: full-screen past-pets view.
    #[cfg(feature = "game")]
    if game::realm_view::is_active() {
        return game::realm_view::draw(display);
    }

    // Rolled stats: full-screen live-pet traits view.
    #[cfg(feature = "game")]
    if game::traits_view::is_active() {
        return game::traits_view::draw(display);
    }

    // Diary: the last cause events.
    #[cfg(feature = "game")]
    if game::diary_view::is_active() {
        return game::diary_view::draw(display);
    }

    // Friends: full-screen list of pets met over the SHDW mesh channel.
    #[cfg(feature = "game")]
    if game::friends_view::is_active() {
        return game::friends_view::draw(display);
    }

    // Battle animation: full-screen two-stage result takeover. On firmware the
    // embassy display loop drives this (timed stages + refreshes); the simulator
    // has no such loop, so it renders the current frame here. Takes priority over
    // the battle result card below.
    #[cfg(all(feature = "game", feature = "simulator", not(feature = "embassy-core")))]
    if game::battle_anim_active() {
        if game::battle_anim_stage() == game::BattleStage::Done {
            game::clear_battle_anim();
        } else {
            display.clear(WHITE)?;
            game::battle_view::draw_anim_sim(display);
            return Ok(());
        }
    }

    // Battle: full-screen friend picker + result report.
    #[cfg(feature = "game")]
    if game::battle_view::is_active() {
        return game::battle_view::draw(display);
    }

    let active = with_display_state!(|state| state.active_screen());
    match active {
        #[cfg(feature = "game")]
        SCREEN_GAME => game::draw_screen_game(display, game::nav::get_nav()),
        SCREEN_MAIN => draw_screen_main(display, health_str, bat_prc),
        #[cfg(feature = "mesh")]
        SCREEN_PM => fw::mesh::pm_inbox::draw(display, bat_prc),
        #[cfg(feature = "mesh")]
        SCREEN_CHANNEL => fw::mesh::channel_browser::draw(display, bat_prc),
        #[cfg(not(feature = "mesh"))]
        SCREEN_CHANNEL => draw_screen_lora(display, bat_prc),
        #[cfg(feature = "mesh")]
        SCREEN_ADVERT => fw::mesh::contacts_screen::draw(display, bat_prc),
        #[cfg(feature = "watch")]
        SCREEN_WATCH => watch::draw(display),
        #[cfg(feature = "watch")]
        SCREEN_CALENDAR => watch::calendar::draw(display),
        SCREEN_NAME => name_screen::draw(display, bat_prc),
        #[cfg(feature = "mesh")]
        SCREEN_QR => qr_screen::draw(display),
        _ => draw_screen_main(display, health_str, bat_prc),
    }?;

    // Screen-lock padlock — above the active screen but below the BLE PIN
    // overlay, so pairing still takes priority. Transient: shown for a few
    // seconds after each key touch while locked, then hidden (keys stay
    // locked) so the screen underneath stays readable.
    if lock::overlay_visible() {
        lock::draw(display)?;
    }

    // BLE pairing PIN overlay — drawn last so it appears on every screen,
    // including over the game screen and any in-game modal.
    draw_ble_pin_overlay(display)
}

/// Draw the BLE passkey PIN dialog centred on screen.
///
/// Does nothing when no pairing is in progress (`BLE_PASSKEY == u32::MAX`).
/// The double-border box signals urgency and renders on top of all other
/// content.
#[cfg(feature = "embassy-core")]
fn draw_ble_pin_overlay<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use embedded_graphics::mono_font::ascii::FONT_10X20;
    let passkey_val = BLE_PASSKEY.load(Ordering::Relaxed);
    if passkey_val == u32::MAX {
        return Ok(());
    }
    let centered = crate::ui::CENTERED;
    Rectangle::new(Point::new(20, 48), Size::new(112, 62))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    Rectangle::new(Point::new(20, 48), Size::new(112, 62))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;
    Rectangle::new(Point::new(24, 52), Size::new(104, 54))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;
    Text::with_text_style("BT PIN:", Point::new(76, 66), ui::TEXT_BLACK, centered).draw(display)?;
    let code_str = format!(8; "{:06}", passkey_val).unwrap();
    Text::with_text_style(
        &code_str,
        Point::new(76, 86),
        MonoTextStyle::new(&FONT_10X20, BLACK),
        centered,
    )
    .draw(display)
    .map(|_| ())
}

#[cfg(feature = "simulator")]
fn draw_ble_pin_overlay<D>(_display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Ok(())
}

/// Draw a standard screen frame with optional header and footer.
///
/// Returns `(body_y_start, body_y_end)` — the vertical pixel range available
/// for screen-specific content.
///
/// - `header`: if `Some`, draws bold title (left) + battery % (right) +
///   divider.
/// - `footer`: if `Some`, draws bold text centered in a bottom bar + divider
///   above.
pub fn draw_frame<D>(
    display: &mut D,
    header: Option<(&str, &u8)>,
    footer: Option<&str>,
) -> Result<(i32, i32), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let bottom = crate::ui::BOTTOM_LEFT;

    let body_start = if let Some((title, bat_prc)) = header {
        ui::draw_text(display, title, Point::new(4, 14), ui::TEXT_BOLD_BLACK, bottom)?;
        draw_battery_icon(display, 128, 2, *bat_prc)?;
        Rectangle::new(Point::new(0, 16), Size::new(152, 1))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        18
    } else {
        0
    };

    let body_end = if let Some(text) = footer {
        let footer_y = 140;
        Rectangle::new(Point::new(0, footer_y - 2), Size::new(152, 1))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        ui::draw_text(
            display,
            text,
            Point::new(76, footer_y + 5),
            ui::TEXT_BOLD_BLACK,
            crate::ui::CENTERED,
        )?;
        footer_y - 4
    } else {
        152
    };

    Ok((body_start, body_end))
}

/// Convenience: draw a frame with header only (no footer).
#[cfg(feature = "mesh")]
pub fn draw_header<D>(display: &mut D, title: &str, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_frame(display, Some((title, bat_prc)), None)?;
    Ok(())
}

/// Width of the [`draw_battery_bar`] indicator, in pixels.
pub const BATTERY_BAR_W: u32 = 4;
/// Height of the [`draw_battery_bar`] indicator, in pixels.
///
/// Four segments of 3 px separated by three single-pixel gaps: `4*3 + 3 = 15`.
pub const BATTERY_BAR_H: u32 = 15;
/// Number of charge segments in [`draw_battery_bar`].
const BATTERY_BAR_SEGMENTS: u32 = 4;

/// Draw a compact vertical battery bar with its top-left corner at `(x, y)`.
///
/// [`BATTERY_BAR_W`]×[`BATTERY_BAR_H`] px, split into [`BATTERY_BAR_SEGMENTS`]
/// segments that fill from the bottom up — one segment per 25 %. Any charge at
/// all lights the first segment, so a live battery never reads as empty.
///
/// This is the game screen's indicator: the full [`draw_battery_icon`] is 24 px
/// wide and crowds the game menu icon, so the game screen uses this and hugs
/// the right edge instead.
///
/// Below 5 % the segments are drawn in red, matching [`draw_battery_icon`].
///
/// Unlit segments are left blank rather than outlined — at 4×3 px an outline
/// leaves a 2×1 interior and reads as noise. There is also no charging
/// indicator: the bolt [`draw_battery_icon`] draws needs more width than this
/// has.
pub fn draw_battery_bar<D>(display: &mut D, x: i32, y: i32, pct: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let color = if pct < 5 { RED } else { BLACK };
    // Segment height, derived so segments + 1 px gaps exactly fill the bar.
    let gaps = BATTERY_BAR_SEGMENTS - 1;
    let seg_h = (BATTERY_BAR_H - gaps) / BATTERY_BAR_SEGMENTS;

    // Round up, so 1 % lights one segment and only 0 % reads as empty.
    let lit = (pct.min(100) as u32 * BATTERY_BAR_SEGMENTS).div_ceil(100);

    for seg in 0..lit {
        // seg 0 is the bottom segment; walk upward from the bar's base.
        let seg_y = y + (BATTERY_BAR_H - (seg + 1) * seg_h - seg) as i32;
        Rectangle::new(Point::new(x, seg_y), Size::new(BATTERY_BAR_W, seg_h))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(display)?;
    }
    Ok(())
}

/// Draw a battery icon at `(x, y)` using 2x2 pixel blocks.
///
/// - Nob: 4w × 6h on the left, centered vertically
/// - Body: 20w × 12h, 2px border
/// - Fill: proportional to `pct`, fills right-to-left (full = all black)
/// - Below 5%: rendered in red
/// - Charging: a lightning bolt drawn in the centre of the body in inverted
///   pixels (white over the black fill, black over the empty white interior)
pub fn draw_battery_icon<D>(display: &mut D, x: i32, y: i32, pct: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let color = if pct < 5 { RED } else { BLACK };

    // Nob on the left (4×6, centered vertically in 12px body)
    Rectangle::new(Point::new(x, y + 3), Size::new(4, 6))
        .into_styled(PrimitiveStyle::with_fill(color))
        .draw(display)?;

    // Body outline (20×12, 2px border)
    let bx = x + 4;
    Rectangle::new(Point::new(bx, y), Size::new(20, 12))
        .into_styled(PrimitiveStyle::with_stroke(color, 2))
        .draw(display)?;

    // Interior: 16×8 (body minus 2px border). Fill from right to left.
    let interior_w = 16u32;
    let fill_w = (pct as u32).min(100) * interior_w / 100;
    let fill_x = bx + 2 + (interior_w - fill_w) as i32;
    if fill_w > 0 {
        Rectangle::new(Point::new(fill_x, y + 2), Size::new(fill_w, 8))
            .into_styled(PrimitiveStyle::with_fill(color))
            .draw(display)?;
    }

    // Charging indicator: lightning bolt in the centre of the body,
    // drawn as inverted pixels — WHITE where fill is BLACK, BLACK
    // where the interior is empty (WHITE).  Visible regardless of
    // current charge level.
    let charging = platform::is_charging();

    if charging {
        // 4×8 lightning bolt mask — top bit = leftmost pixel.
        const BOLT: [u8; 8] = [
            0b0110, 0b1100, 0b1100, 0b1110, 0b0111, 0b0011, 0b0011, 0b0110,
        ];
        // Centre 4-wide bolt in 16-wide interior.
        let bolt_x = bx + 2 + 6;
        let bolt_y = y + 2;
        let pixels = (0..8i32).flat_map(|row| {
            (0..4i32).filter_map(move |col| {
                if (BOLT[row as usize] >> (3 - col)) & 1 == 0 {
                    return None;
                }
                let px = bolt_x + col;
                let py = bolt_y + row;
                let underlying_black = fill_w > 0 && px >= fill_x;
                let inverted = if underlying_black { WHITE } else { BLACK };
                Some(Pixel(Point::new(px, py), inverted))
            })
        });
        display.draw_iter(pixels)?;
    }

    Ok(())
}

/// True while the About screen is open.
#[cfg(feature = "embassy-core")]
pub fn about_active() -> bool {
    with_display_state!(|state| state.current_screen().is_about())
}

fn draw_screen_main<D>(display: &mut D, _health_str: &str, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // About screen: full-screen credits, no other elements.
    let (about, about_page) = with_display_state!(|state| {
        let s = state.current_screen();
        (s.is_about(), s.about_page())
    });
    if about {
        #[cfg(feature = "embassy-core")]
        fw::led::ABOUT_RAINBOW.signal(());
        return menu::draw_about(display, about_page);
    }

    // LoRa radio preset picker: full-screen custom rendering.
    let (lora, lora_page) = with_display_state!(|state| {
        let s = state.current_screen();
        (s.is_lora_radio(), s.lora_page())
    });
    if lora {
        return menu::draw_lora_radio(display, lora_page);
    }

    // Confirmation dialog: full-screen "Are you sure?" overlay.
    let confirm_prompt = with_display_state!(|state| state.current_screen().confirm_prompt());
    if let Some(prompt) = confirm_prompt {
        return menu::draw_confirm(display, prompt);
    }

    // Device label ("Cyber Ægg <id>") for the header title.
    let device_id = platform::device_label();

    // Build the footer: node name + time
    #[cfg(feature = "embassy-core")]
    let footer_text = {
        let mut f: heapless::String<24> = heapless::String::new();
        NODE_NAME.lock(|cell| {
            let name = cell.borrow();
            if !name.is_empty() {
                let _ = f.push_str(truncate_str(name.as_str(), 16));
            }
        });
        if let Some(unix) = unix_now() {
            let offset_secs = tz_offset_secs();
            let local = (unix as i64 + offset_secs) as u32;
            let h = (local % 86400) / 3600;
            let m = (local % 3600) / 60;
            if !f.is_empty() {
                let _ = f.push_str(" ");
            }
            let _ = core::fmt::Write::write_fmt(&mut f, format_args!("{:02}:{:02}", h, m));
        }
        // Unread-PM indicator — appears as ` +N` suffix when at
        // least one peer has unread incoming messages.  Lets the
        // user spot a new PM without entering the Messages screen.
        #[cfg(feature = "mesh")]
        {
            let unread = fw::mesh::pm_inbox::unread_total();
            if unread > 0 {
                let _ = core::fmt::Write::write_fmt(&mut f, format_args!(" +{}", unread));
            }
        }
        f
    };
    #[cfg(not(feature = "embassy-core"))]
    let footer_text: heapless::String<24> = heapless::String::new();

    let footer = if footer_text.is_empty() {
        None
    } else {
        Some(footer_text.as_str())
    };

    draw_frame(display, Some((device_id.as_str(), bat_prc)), footer)?;

    let (items, pos, stepper_active) = with_display_state!(|state| {
        let screen = state.current_screen();
        (
            screen.current_items(),
            screen.current_pos(),
            screen.is_stepper_active(),
        )
    });

    menu::draw_menu(display, items, pos, stepper_active)?;

    Ok(())
}

#[cfg(not(feature = "mesh"))]
fn draw_screen_lora<D>(display: &mut D, _bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let center = crate::ui::CENTERED;
    Text::with_text_style(
        "Channels (no mesh)",
        Point::new(76, 76),
        ui::TEXT_BLACK,
        center,
    )
    .draw(display)?;
    Ok(())
}

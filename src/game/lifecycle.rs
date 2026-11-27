//! Game lifecycle — initialisation, save/restore, and the per-cycle
//! update that embassy.rs calls from the display loop.
//!
//! The game state lives in a static cell, accessed through the functions
//! in this module.  All functions are async (flash access goes through
//! the shared QSPI mutex).

use crate::sync::SyncCell;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use super::engine::{DisplayAnim, GameState, PET_NAME_MAX, PetRealm, PetRecord, PetStats};
#[cfg(feature = "embassy-core")]
use super::engine::{
    NAME_KEY, NAME_KEY_V2, REALM_SAVE_SIZE, SAVE_KEY, SAVE_KEY_V2, SAVE_SIZE, SAVE_SIZE_V2,
    pick_name, pick_save,
};

// ---------------------------------------------------------------------------
// Static game state
// ---------------------------------------------------------------------------

static GAME: SyncCell<Option<GameState>> = SyncCell::new(None);
static REALM: SyncCell<PetRealm> = SyncCell::new(PetRealm::new());

/// Current pet name (not part of GameState to keep the engine pure).
static PET_NAME: SyncCell<[u8; PET_NAME_MAX]> = SyncCell::new([0u8; PET_NAME_MAX]);
static PET_NAME_LEN: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Uptime tick counter.  Incremented by embassy.rs on each game cycle.
/// 1 tick = 10 seconds.  Starts at 0 on boot, never jumps.
static UPTIME_TICKS: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Uptime tick source
// ---------------------------------------------------------------------------

/// Get the current game tick (uptime-based, starts at 0 on boot).
/// 1 tick = 10 seconds.  Uses embassy uptime on hardware, the std
/// monotonic clock in the simulator (so animations / hatching /
/// stat decay actually progress when running `make sim`), and the
/// manually-advanced `UPTIME_TICKS` counter elsewhere (tests).
pub fn now_tick() -> u32 {
    #[cfg(any(feature = "embassy-core", feature = "simulator"))]
    {
        (crate::platform::uptime_ms() / 10_000) as u32
    }
    #[cfg(not(any(feature = "embassy-core", feature = "simulator")))]
    {
        UPTIME_TICKS.load(Ordering::Relaxed)
    }
}

/// Seconds in a minute, for the wall-clock arithmetic.
const SECS_PER_MIN: i64 = 60;

/// The local minute of the day (0..1440), or `None` while the wall clock is
/// not set. The demo pet's bedtime follows it.
pub fn local_minute() -> Option<u16> {
    crate::platform::unix_secs().map(minute_of_day)
}

/// The local minute of the day at unix time `unix` (time zone applied).
fn minute_of_day(unix: u32) -> u16 {
    let local_min = (unix as i64 + crate::tz_offset_secs()).div_euclid(SECS_PER_MIN);
    local_min.rem_euclid(super::engine::demo::MINUTES_PER_DAY as i64) as u16
}

/// What the background shows now, without updating the pet; `None` without a
/// game (or where no background is drawn).
pub fn background_state() -> Option<super::engine::scene::BackgroundState> {
    let s = unsafe { (*GAME.get()).as_ref() }?;
    super::engine::scene::background_state(s)
}

/// Set the live pet's dirt (guide shots and tests only).
#[cfg(any(test, feature = "simulator"))]
pub fn debug_set_dirt_pct(pct: u8) {
    if let Some(s) = unsafe { (*GAME.get()).as_mut() } {
        s.dirt = (pct.min(100) as i32 * super::engine::needs::P) as u16;
    }
}

/// A clock sync from `before` to `after` (local minutes of day) needs an
/// immediate redraw: the sky differs, or the next switch moved earlier by
/// more than a minute (a later switch waits for the planned wake).
pub fn clock_sync_needs_redraw(before: Option<u16>, after: Option<u16>) -> bool {
    use super::engine::scene::{minutes_to_switch, sky};
    if sky(before) != sky(after) {
        return true;
    }
    match (minutes_to_switch(before), minutes_to_switch(after)) {
        (Some(was), Some(now)) => was > now + 1,
        _ => false,
    }
}

/// Advance the uptime tick by `delta` ticks (for tests/simulator only).
pub fn advance_ticks(delta: u32) {
    UPTIME_TICKS.fetch_add(delta, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Init — load from flash or create new egg
// ---------------------------------------------------------------------------

/// Initialise the game state.  Tries to load from ekv (if mesh feature
/// is enabled); if not found or corrupt, creates a fresh egg.
///
/// Call once at startup after flash and ekv are initialised.
/// Initialise the game.  Loads from flash if a save exists.
/// If no save is found, the game state stays `None` until the player
/// presses Fire to start (see [`start_new_game`]).
#[cfg(feature = "embassy-core")]
pub async fn init() {
    let state = try_load().await;
    if state.is_some() {
        defmt::info!("game: resumed from save");
    } else {
        defmt::info!("game: no save — waiting for player to start");
    }
    unsafe {
        *GAME.get() = state;
    }

    // Load pet name and Unicorn Realm from KV (always present under embassy-base).
    {
        use crate::fw::kv;
        let ns = kv::namespace("game");

        // Pet name.
        let mut v3_buf = [0u8; PET_NAME_MAX];
        let mut old_buf = [0u8; PET_NAME_MAX];
        let v3 = ns.get(NAME_KEY, &mut v3_buf).await.ok().map(|n| &v3_buf[..n]);
        let old = ns.get(NAME_KEY_V2, &mut old_buf).await.ok().map(|n| &old_buf[..n]);
        if let Some(name) = pick_name(v3, old) {
            set_pet_name(name);
        }

        // If a saved game is active but has no name, prompt for one.
        if pet_name().is_empty()
            && let Some(s) = unsafe { (*GAME.get()).as_mut() }
            && s.phase == super::engine::Phase::Active
        {
            s.naming_pending = true;
        }

        // Unicorn Realm (past pets).
        let mut buf = [0u8; REALM_SAVE_SIZE];
        if let Ok(n) = ns.get("realm", &mut buf).await {
            let realm = PetRealm::from_bytes(&buf[..n]);
            defmt::info!("game: loaded {} past pets", realm.count);
            unsafe {
                *REALM.get() = realm;
            }
        }
    }

    // Friends met over the SHDW mesh channel — own KV namespace.
    super::friends::init().await;
}

/// Read the saved game. The v3 save lives under `state_v3`; the old key
/// `state` is read only for the format-2 upgrade and never written or
/// deleted, so the previous firmware can still find its pet. `pick_save`
/// chooses between them (and flags an upgrade for an immediate save).
#[cfg(feature = "embassy-core")]
async fn try_load() -> Option<GameState> {
    use crate::fw::kv;
    let ns = kv::namespace("game");
    let mut v3_buf = [0u8; SAVE_SIZE];
    let mut v2_buf = [0u8; SAVE_SIZE_V2];
    let v3 = ns.get(SAVE_KEY, &mut v3_buf).await.ok().map(|n| &v3_buf[..n]);
    let v2 = ns.get(SAVE_KEY_V2, &mut v2_buf).await.ok().map(|n| &v2_buf[..n]);
    match pick_save(v3, v2) {
        Some((mut s, upgraded)) => {
            s.last_update_tick = 0;
            // Clock already known: catch up now. Otherwise `on_wall_clock_set`
            // does it when the clock first becomes known.
            let saved = s.saved_unix;
            BOOT_SAVED_UNIX.store(saved, Ordering::Relaxed);
            if let Some(now) = crate::unix_now() {
                let uptime_s = (crate::platform::uptime_ms() / 1000) as u32;
                apply_boot_catch_up(&mut s, &CATCH_UP_DONE, saved, now, uptime_s);
            }
            defmt::info!(
                "game: restored from flash (gen={} age={} upgraded={})",
                s.generation,
                s.age_ticks,
                upgraded
            );
            Some(s)
        }
        None => {
            if v3.is_some() || v2.is_some() {
                defmt::warn!("game: corrupt save data");
            }
            None
        }
    }
}

#[cfg(feature = "embassy-core")]
fn new_egg(kind: super::engine::PetKind) -> GameState {
    let id = crate::fw::device_id::get_bytes();
    let seed = u64::from_le_bytes([
        id[0],
        id[1],
        id[2],
        id[3],
        id[0] ^ 0xAA,
        id[1] ^ 0x55,
        id[2] ^ 0xCC,
        id[3] ^ 0x33,
    ]);
    GameState::new_egg(seed, kind)
}

#[cfg(not(feature = "embassy-core"))]
fn new_egg(kind: super::engine::PetKind) -> GameState {
    GameState::new_egg(42, kind)
}

// ---------------------------------------------------------------------------
// Game lifecycle queries
// ---------------------------------------------------------------------------

/// Returns true if a game is active (egg hatching or pet alive).
/// False on first boot before the player presses start.
pub fn is_started() -> bool {
    unsafe { (*GAME.get()).is_some() }
}

/// Returns true if the pet is alive enough to receive a station buff
/// over NFC — i.e. a game has started and the pet has not left.
/// `Hatching`, `Active` and `Leaving` all qualify; `Gone` does not.
pub fn can_use_station() -> bool {
    let state = unsafe { (*GAME.get()).as_ref() };
    matches!(state, Some(s) if s.phase != super::engine::Phase::Gone)
}

/// Longest boot catch-up, in minutes (12 h).
pub const CATCH_UP_MAX_MIN: u32 = 720;

/// Minutes the pet catches up on boot: the wall-clock gap since the save,
/// capped at 12 h. 0 when either time is unknown (0) or the clock ran back.
pub fn catch_up_minutes(saved_unix: u32, now_unix: u32) -> u32 {
    if saved_unix == 0 || now_unix == 0 || now_unix < saved_unix {
        return 0;
    }
    ((now_unix - saved_unix) / 60).min(CATCH_UP_MAX_MIN)
}

/// Off-time to catch up when the wall clock becomes known `uptime_s` seconds
/// after boot: the gap from the save to the boot moment (`now - uptime`), so
/// the time already run this session is not counted twice.
pub fn deferred_catch_up_minutes(saved_unix: u32, now_unix: u32, uptime_s: u32) -> u32 {
    match now_unix.checked_sub(uptime_s) {
        Some(boot_unix) => catch_up_minutes(saved_unix, boot_unix),
        None => 0,
    }
}

/// Catch the pet up once per boot. `done` is the once-per-boot latch: the
/// first call takes it and applies the off-time (then requests a save so
/// `saved_unix` is refreshed); later calls do nothing. Returns true if the
/// catch-up was taken.
pub fn apply_boot_catch_up(
    s: &mut GameState,
    done: &AtomicBool,
    boot_saved_unix: u32,
    now_unix: u32,
    uptime_s: u32,
) -> bool {
    boot_catch_up_minutes(s, done, boot_saved_unix, now_unix, uptime_s).is_some()
}

/// [`apply_boot_catch_up`] reporting the minutes applied: `None` when the
/// catch-up was already taken, `Some(0)` when taken with nothing to apply.
pub fn boot_catch_up_minutes(
    s: &mut GameState,
    done: &AtomicBool,
    boot_saved_unix: u32,
    now_unix: u32,
    uptime_s: u32,
) -> Option<u32> {
    if done.swap(true, Ordering::Relaxed) {
        return None;
    }
    let minutes = deferred_catch_up_minutes(boot_saved_unix, now_unix, uptime_s);
    if minutes > 0 {
        // The replay stands for the time before boot: the demo clock reads
        // the time of day it started at, then the pet's own time again.
        let boot_unix = now_unix.saturating_sub(uptime_s);
        let start_unix = boot_unix.saturating_sub(minutes * SECS_PER_MIN as u32);
        s.sync_clock(s.last_update_tick, Some(minute_of_day(start_unix)));
        s.catch_up(minutes);
        s.sync_clock(s.last_update_tick, Some(minute_of_day(boot_unix)));
        s.request_save();
    }
    Some(minutes)
}

/// `saved_unix` as loaded at boot (RAM only). A save made before the clock
/// is known rewrites `GameState::saved_unix`, so the deferred catch-up reads
/// this copy instead.
static BOOT_SAVED_UNIX: AtomicU32 = AtomicU32::new(0);

/// The `saved_unix` a save writes. Clock known: now. Clock unknown and the
/// boot catch-up still pending: the boot value plus the uptime, a lower bound
/// of the real time (so a reboot catches up the unknown gap but never counts
/// time the badge ran). Otherwise 0 (unknown).
pub fn saved_unix_to_write(
    now_unix: Option<u32>,
    boot_saved_unix: u32,
    uptime_s: u32,
    catch_up_done: bool,
) -> u32 {
    match now_unix {
        Some(now) => now,
        None if !catch_up_done && boot_saved_unix != 0 => boot_saved_unix.saturating_add(uptime_s),
        None => 0,
    }
}

/// Set once the boot catch-up was taken (RAM only, one per boot).
static CATCH_UP_DONE: AtomicBool = AtomicBool::new(false);

/// A new pet replaces the loaded one: drop the old pet's boot gap so the
/// egg is never caught up on it, and no later save writes it either.
pub fn forget_boot_gap(boot_saved: &AtomicU32, done: &AtomicBool) {
    boot_saved.store(0, Ordering::Relaxed);
    done.store(true, Ordering::Relaxed);
}

/// Called by `set_wall_clock` the moment the clock is set. The first call
/// of a boot catches the loaded pet up on the time the badge was off; later
/// calls (re-syncs) and a missing pet do nothing.
#[cfg(feature = "embassy-core")]
pub fn on_wall_clock_set(now_unix: u32) {
    if let Some(s) = unsafe { (*GAME.get()).as_mut() } {
        let uptime_s = (crate::platform::uptime_ms() / 1000) as u32;
        let saved = BOOT_SAVED_UNIX.load(Ordering::Relaxed);
        let applied = boot_catch_up_minutes(s, &CATCH_UP_DONE, saved, now_unix, uptime_s);
        // The pet learns the time of day (or a re-sync moves it).
        let before = s.day_min;
        s.sync_clock(now_tick(), Some(minute_of_day(now_unix)));
        // Wake the display loop now, not at the next scheduled wake: the pet
        // jumped ahead (catch-up), or the sky differs or its next switch now
        // comes before the planned wake. The loop plans the next wake again.
        if applied.is_some_and(|m| m > 0) || clock_sync_needs_redraw(before, s.day_min) {
            crate::TOAST_SIGNAL.signal(());
        }
    }
}

/// Create a new egg and begin the hatching countdown.
/// Called after the player selects a pet kind (and money mode) on the
/// selection screen. The egg always starts with `money = 100` (see
/// `GameState::new_egg`); `money_enabled` decides whether that balance
/// is actually in play for this generation. `hard_mode` is only
/// meaningful when `money_enabled` is true — the pet-select screen only
/// ever sets it alongside `money_enabled = true` (see `pet_select::confirm`).
pub fn start_new_game(kind: super::engine::PetKind, money_enabled: bool, hard_mode: bool) {
    let mut egg = new_egg(kind);
    egg.last_update_tick = now_tick();
    egg.money_enabled = money_enabled;
    egg.hard_mode = hard_mode;
    forget_boot_gap(&BOOT_SAVED_UNIX, &CATCH_UP_DONE);
    clear_diary();
    super::battle_link::cancel();
    unsafe {
        *GAME.get() = Some(egg);
    }
}

// ---------------------------------------------------------------------------
// Pet naming
// ---------------------------------------------------------------------------

/// Short Danish and Dutch names used as random defaults.
const DEFAULT_NAMES: &[&str] = &[
    "Arie", "Bert", "Bjorn", "Bob", "Bram", "Daan", "Femke", "Freja", "Ida", "Jens", "Kees",
    "Koen", "Lars", "Lotte", "Mette", "Niels", "Rupert", "Stijn", "Sven", "Anja",
];

/// Set the pet name from raw bytes (called by text entry callback).
///
/// Also flags the game state for immediate save so the name is persisted
/// to flash on the next `save_if_needed()` call, rather than waiting for
/// the 15-minute periodic save (which would lose the name on an early
/// reboot).
pub fn set_pet_name(name: &[u8]) {
    let len = name.len().min(PET_NAME_MAX);
    let buf = unsafe { &mut *PET_NAME.get() };
    buf[..len].copy_from_slice(&name[..len]);
    buf[len..].fill(0);
    PET_NAME_LEN.store(len as u8, core::sync::atomic::Ordering::Relaxed);

    if let Some(state) = unsafe { (*GAME.get()).as_mut() } {
        state.request_save();
    }
}

/// Get the current pet name as a str.
pub fn pet_name() -> &'static str {
    let len = PET_NAME_LEN.load(core::sync::atomic::Ordering::Relaxed);
    let buf = unsafe { &*PET_NAME.get() };
    super::engine::name_str(buf, len)
}

/// Get the current pet name as raw bytes.
fn pet_name_bytes_sync() -> &'static [u8] {
    let len = PET_NAME_LEN.load(core::sync::atomic::Ordering::Relaxed) as usize;
    let buf = unsafe { &*PET_NAME.get() };
    &buf[..len]
}

/// Check if the engine wants us to prompt for a name (hatching just completed).
/// Clears the flag and returns true once.
pub fn take_naming_pending() -> bool {
    let state = unsafe { (*GAME.get()).as_mut() };
    match state {
        Some(s) if s.naming_pending => {
            s.naming_pending = false;
            true
        }
        _ => false,
    }
}

/// Pick a random default name based on a seed.
pub fn random_default_name(seed: u32) -> &'static str {
    DEFAULT_NAMES[(seed as usize) % DEFAULT_NAMES.len()]
}

// ---------------------------------------------------------------------------
// Game cycle — called from the display loop
// ---------------------------------------------------------------------------

/// Run one game cycle: update state, return stats for display.
/// Returns `None` if no game is started yet.
pub fn cycle() -> Option<PetStats> {
    let state = unsafe { (*GAME.get()).as_mut()? };
    let tick = now_tick();
    state.sync_clock(tick, local_minute());
    let stats = state.stats(tick);
    check_severity_transition(state);
    check_diabetes_onset(state);
    check_alcoholism_onset(state);
    log_causes(state);
    Some(stats)
}

/// Log what the engine blamed since the last cycle and toast the first. The
/// waiter never calls `cycle`, so draining here cannot starve any view.
fn log_causes(state: &mut GameState) {
    let at_min = state.age_ticks / super::engine::TICKS_PER_MINUTE;
    let causes = state.take_causes();
    for cause in causes.iter() {
        log_cause(*cause, at_min);
    }
    // The diary logs always; the toast only where it can be seen and cleared:
    // not behind a full-screen view, nor under a modal's popover. A demo pet
    // stays quiet about its own causes; the player who just stepped in
    // (`pause_min` running) is answered.
    let answers_player = !state.demo.on || state.demo.pause_min > 0;
    if answers_player
        && let Some(first) = causes.iter().next()
        && !super::full_screen_view_up()
        && !super::modal::is_open()
    {
        super::show_cause(*first);
    }
}

/// The pet's age in minutes (0 without a game).
pub fn pet_age_min() -> u32 {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.map_or(0, |s| s.age_ticks / super::engine::TICKS_PER_MINUTE)
}

/// Causes logged for the current pet. RAM only: never saved.
static DIARY: SyncCell<super::diary::Diary> = SyncCell::new(super::diary::Diary::new());

/// A copy of the diary for the view.
pub fn diary() -> super::diary::Diary {
    unsafe { (*DIARY.get()).clone() }
}

pub(super) fn clear_diary() {
    unsafe { (*DIARY.get()).clear() }
}

pub(super) fn log_cause(cause: super::engine::causes::Cause, at_min: u32) {
    unsafe { (*DIARY.get()).push(cause, at_min) }
}

/// Ticks left in the running action (cheap read, no update; 0 when none or
/// before a game exists). Counted from the last engine update, so it stays
/// fresh between passes. The display waiter uses it to pace action frames
/// without calling [`cycle`], which updates the engine.
pub fn action_ticks_remaining() -> u8 {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.map_or(0, |s| {
        let end = s.action_end_tick();
        end.saturating_sub(now_tick()).min(s.action_ticks_remaining as u32) as u8
    })
}

/// What is written under the pet (cheap read, no update, like
/// [`display_anim`]). `None` before a game exists.
pub fn pet_line() -> Option<super::engine::PetLine> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    Some(state.pet_line())
}

/// Whether the current pet is in demo mode (false without a game).
pub fn is_demo() -> bool {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.is_some_and(|s| s.is_demo())
}

/// Switch demo mode: the new state, or why it cannot be switched. Catches
/// the pet up first, like any player event; the engine saves at once.
pub fn toggle_demo() -> Result<bool, super::engine::doses::Reason> {
    let Some(s) = (unsafe { (*GAME.get()).as_mut() }) else {
        return Err(super::engine::doses::Reason::Gone);
    };
    event_at(s, now_tick(), |_| {});
    let on = !s.is_demo();
    s.set_demo(on)?;
    // A challenge belongs to the league it was made in.
    super::battle_link::cancel();
    Ok(on)
}

/// The demo rows' labels in the game settings menu.
const DEMO_LABEL_ON: &str = "Demo mode: on";
const DEMO_LABEL_OFF: &str = "Demo mode: off";
const DEMO_LABEL_FIX_ME_FIRST: &str = "Demo: fix me first";
const DEMO_LABEL_FROZEN: &str = "Demo: frozen";
const DEMO_LABEL_NO_PET: &str = "Demo: no pet";
const DEMO_CARE_ANIMATED: &str = "Demo care: animated";
const DEMO_CARE_INSTANT: &str = "Demo care: instant";

/// Every demo row label (host-only, for the guide's word list).
#[cfg(any(test, feature = "simulator"))]
pub fn demo_menu_words() -> [&'static str; 7] {
    [
        DEMO_LABEL_ON,
        DEMO_LABEL_OFF,
        DEMO_LABEL_FIX_ME_FIRST,
        DEMO_LABEL_FROZEN,
        DEMO_LABEL_NO_PET,
        DEMO_CARE_ANIMATED,
        DEMO_CARE_INSTANT,
    ]
}

/// The label that says why the demo switch is refused. Every reason is named,
/// with no catch-all, so a new `Reason` or `Refusal` fails to compile here
/// until someone decides what the row says.
fn demo_refusal_label(why: super::engine::doses::Reason) -> &'static str {
    use super::engine::doses::{Reason, Refusal};
    match why {
        Reason::Refused(Refusal::FixMeFirst) => DEMO_LABEL_FIX_ME_FIRST,
        Reason::Hibernating => DEMO_LABEL_FROZEN,
        Reason::Egg | Reason::Gone => DEMO_LABEL_NO_PET,
        // `GameState::demo_refusal` never answers these (it drops `Busy`, and
        // the rest belong to an action, not the switch). The test
        // `the_demo_refusal_has_a_label` pins that.
        Reason::Busy
        | Reason::CantAfford
        | Reason::Wait(_)
        | Reason::Refused(
            Refusal::NotThirsty
            | Refusal::AlreadyClean
            | Refusal::Injured
            | Refusal::Asleep
            | Refusal::NoNeed
            | Refusal::Tired
            | Refusal::TooWeak
            | Refusal::TooDrunk
            | Refusal::DemoOnly
            | Refusal::NoDemo,
        ) => DEMO_LABEL_NO_PET,
    }
}

/// Why pressing the demo switch now would do nothing, or `None` when it would
/// switch: off always works, on follows `GameState::demo_refusal`.
pub fn demo_switch_refusal() -> Option<super::engine::doses::Reason> {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        None => Some(super::engine::doses::Reason::Gone),
        Some(s) if s.is_demo() => None,
        Some(s) => s.demo_refusal(),
    }
}

/// The demo row of the game settings menu. The refusal is the engine's, so
/// the label says why the press would do nothing.
pub fn demo_menu_label() -> &'static str {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state.map(|s| (s.is_demo(), s.demo_refusal())) {
        None => DEMO_LABEL_NO_PET,
        Some((true, _)) => DEMO_LABEL_ON,
        Some((false, None)) => DEMO_LABEL_OFF,
        Some((false, Some(why))) => demo_refusal_label(why),
    }
}

/// Flip the demo pet's care between instant and animated (it works while demo
/// is off too). Returns whether care is now animated; false without a pet.
pub fn toggle_demo_care() -> bool {
    let Some(s) = (unsafe { (*GAME.get()).as_mut() }) else {
        return false;
    };
    let animated = !s.demo.animated;
    s.set_demo_care(animated);
    animated
}

/// The care row of the game settings menu.
pub fn demo_care_label() -> &'static str {
    let state = unsafe { (*GAME.get()).as_ref() };
    if state.is_some_and(|s| s.demo.animated) {
        DEMO_CARE_ANIMATED
    } else {
        DEMO_CARE_INSTANT
    }
}

/// Install (or clear) the game state for a test. The caller holds
/// `game::lock_test_state`.
#[cfg(test)]
pub(crate) fn set_state_for_test(state: Option<GameState>) {
    unsafe {
        *GAME.get() = state;
    }
}

// ---------------------------------------------------------------------------
// Severity-change buzzer alert
//
// Whenever the pet moves *up* a severity level — neutral → warning,
// warning → severe, or severe → leaving — a short buzzer notification is
// fired.  Transitions downward (player fed/healed the pet) and moves into
// the terminal `Gone` state are silent.  Mute is honoured via the
// `GAME_MUTE` atomic set from the menu.
// ---------------------------------------------------------------------------

/// Severity ladder used by the alert.  The numeric encoding is the value
/// that `LAST_SEVERITY` stores between cycles; `Uninit` is only seen on
/// the very first call after boot and suppresses a spurious alert for the
/// seed transition.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Severity {
    Uninit = 0,
    Neutral = 1,
    Warning = 2,
    Severe = 3,
    Leaving = 4,
    Gone = 5,
}

static LAST_SEVERITY: AtomicU8 = AtomicU8::new(Severity::Uninit as u8);

/// Tracks whether the pet was diabetic as of the previous cycle, so the
/// one-shot onset alert (buzzer + toast) fires exactly once, the instant
/// `diabetic` flips false → true — not on every cycle it stays true.
static WAS_DIABETIC: AtomicBool = AtomicBool::new(false);
static WAS_ALCOHOLIC: AtomicBool = AtomicBool::new(false);

/// Set when the in-memory Unicorn Realm buffer has been mutated (a pet pushed)
/// and needs persisting on the next save cycle, but the record was NOT derived
/// from the current `GameState` (e.g. a manual "Reset Pet" in `new_generation`,
/// after which the live state is already the fresh egg). Distinct from
/// `GameState::realm_pending`, which means "derive+record the current pet on
/// the next save" (the natural-death path).
static REALM_DIRTY: AtomicBool = AtomicBool::new(false);

fn current_severity(state: &GameState) -> Severity {
    use super::engine::Phase;
    use super::engine::status::status_line;

    if state.phase == Phase::Gone {
        return Severity::Gone;
    }
    if state.phase == Phase::Leaving {
        return Severity::Leaving;
    }
    // Severity is derived from the status line (not the display animation)
    // so that active-action animations — Feeding/Healing/etc — don't
    // "mask" an existing warning and suppress the alert when the action ends.
    let line = status_line(state);
    if line.danger {
        Severity::Severe
    } else if line.alerts() {
        Severity::Warning
    } else {
        Severity::Neutral
    }
}

/// Should a buzzer alert play? The one place that decides it: not while
/// muted, and never in demo mode, which is silent. `upward` is whether the
/// event is an alert at all (a severity rise; the onset alerts and the
/// farewell pass `true`).
pub(super) fn alert_due(muted: bool, demo: bool, upward: bool) -> bool {
    upward && !muted && !demo
}

fn check_severity_transition(state: &GameState) {
    let now = current_severity(state) as u8;
    let prev = LAST_SEVERITY.swap(now, Ordering::Relaxed);

    // Suppress alerts during the very first cycle after boot so the
    // seed transition Uninit → <current> stays silent.
    if prev == Severity::Uninit as u8 {
        return;
    }

    let muted = crate::GAME_MUTE.load(Ordering::Relaxed);
    let demo = state.demo.on;

    // Pet just left: play the "funny ending" melody.  Triggers on any
    // transition into Gone so e.g. a natural Leaving → Gone fires the
    // melody; `new_generation()` goes via Hatching, not Gone, so it
    // does not trigger this path.
    if now == Severity::Gone as u8 && prev != Severity::Gone as u8 {
        if alert_due(muted, demo, true) {
            #[cfg(feature = "embassy-core")]
            crate::fw::buzzer::play(crate::FUNNY_ENDING_INDEX);
        }
        return;
    }

    // Alert only on upward transitions between tracked levels:
    //   Neutral(1) → Warning(2)
    //   Warning(2) → Severe(3)
    //   Severe(3)  → Leaving(4)
    let upward = now > prev && now <= Severity::Leaving as u8;
    if alert_due(muted, demo, upward) {
        #[cfg(feature = "embassy-core")]
        crate::fw::buzzer::play(crate::PET_WARN_INDEX);
    }
}

/// Should the onset alert for a permanent condition fire now? The one place
/// that decides it. `seen` starts false each boot, so a pet that still has
/// the condition alerts again after every boot, in case the player forgot
/// the pet is sick. While the pet hibernates nothing fires and `seen` is
/// left alone. The alert fires once per boot, when the condition is present
/// and the pet is not hibernating, so a pet that hibernated since boot alerts
/// on defrost; one that already alerted this boot does not alert again.
fn onset_alert_due(seen: &AtomicBool, present: bool, hibernating: bool) -> bool {
    if hibernating {
        return false;
    }
    let prev = seen.swap(present, Ordering::Relaxed);
    present && !prev
}

/// One-shot alert (buzzer + toast) the instant the pet becomes diabetic.
/// Separate from `check_severity_transition` since diabetes is a
/// permanent flag, not a point on the Neutral→Leaving ladder — it can
/// become true at any severity level and should announce itself
/// regardless of what else is going on.
fn check_diabetes_onset(state: &super::engine::GameState) {
    if onset_alert_due(&WAS_DIABETIC, state.diabetic, state.hibernating) {
        let muted = crate::GAME_MUTE.load(Ordering::Relaxed);
        if alert_due(muted, state.demo.on, true) {
            #[cfg(feature = "embassy-core")]
            crate::fw::buzzer::play(crate::PET_WARN_INDEX);
        }
        // Full-screen takeover, not just a toast — this is a rare,
        // one-time event worth making unmissable.
        super::show_diabetes_alert();
    }
}

/// One-shot alert (buzzer + full-screen takeover) the instant the pet
/// becomes alcoholic — the twin of [`check_diabetes_onset`]. Alcoholism
/// mirrors diabetes in the engine (permanent flag + coupled sick
/// penalty) but previously landed with no announcement at all.
fn check_alcoholism_onset(state: &super::engine::GameState) {
    if onset_alert_due(&WAS_ALCOHOLIC, state.dependent, state.hibernating) {
        let muted = crate::GAME_MUTE.load(Ordering::Relaxed);
        if alert_due(muted, state.demo.on, true) {
            #[cfg(feature = "embassy-core")]
            crate::fw::buzzer::play(crate::PET_WARN_INDEX);
        }
        super::show_alcoholism_alert();
    }
}

/// Tick at which the sleep animation should stop being shown.  When
/// the player taps Sleep, the engine may auto-wake within a single
/// tick (10 s) if `tired` was already low.  We pin a display floor of 4
/// ticks here, which keeps the sleeping pose on screen for 40 s.
static SLEEP_ANIM_UNTIL_TICK: AtomicU32 = AtomicU32::new(0);

/// Get the current display animation (cheap, no update).
pub fn display_anim() -> DisplayAnim {
    let state = unsafe { (*GAME.get()).as_ref() };
    // A v3 anim is shown only when this pet has its sprite row; else the
    // anim the pet showed before the v3 art (see `display_anim_with`).
    let raw = match state {
        Some(s) => s.display_anim_with(|a| super::engine::anim_files::frame_count(s.pet_kind, a) > 0),
        None => return DisplayAnim::Gone,
    };
    // Display-layer floor for the sleep animation — see
    // `SLEEP_ANIM_UNTIL_TICK`.  Only override the "neutral" idle
    // states; never mask hatching / leaving / gone / actions.
    if matches!(
        raw,
        DisplayAnim::Idle
            | DisplayAnim::Happy
            | DisplayAnim::WarningTired
            | DisplayAnim::WarningSick
            | DisplayAnim::WarningHungry
            | DisplayAnim::WarningMiserable
            // The v3 states that replaced the ones above.
            | DisplayAnim::Tipsy
            | DisplayAnim::Sore
            | DisplayAnim::NeedsRehab
            | DisplayAnim::Wasted
            | DisplayAnim::Thirsty
            | DisplayAnim::HungOver
            | DisplayAnim::Groggy
            | DisplayAnim::Injured
            | DisplayAnim::Wired
            | DisplayAnim::Stuffed
    ) && now_tick() < SLEEP_ANIM_UNTIL_TICK.load(Ordering::Relaxed)
    {
        return DisplayAnim::Sleeping;
    }
    raw
}

/// Why `action` (with the chosen `drink`) would be turned away now, or `None`
/// when it would be accepted (affordability aside).
pub fn refusal_for(
    action: super::engine::Action,
    drink: Option<super::engine::DrinkKind>,
) -> Option<super::engine::doses::Reason> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    state.refusal_for(action, drink)
}

/// What the pet says it needs now (the Stats menu's Hint row).
pub fn hint() -> Option<super::engine::hint::Hint> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    Some(super::engine::hint::hint(state))
}

/// The first warning that `action` would cause now (see `GameState::preview`).
pub fn preview(
    action: super::engine::Action,
    food: Option<super::engine::FoodKind>,
    drink: Option<super::engine::DrinkKind>,
) -> Option<super::engine::causes::Cause> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    state.preview(action, food, drink)
}

/// The first warning a night's sleep would cause now.
pub fn preview_sleep() -> Option<super::engine::causes::Cause> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    state.preview_sleep()
}

/// The first warning waking the pet now would cause.
pub fn preview_wake() -> Option<super::engine::causes::Cause> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    state.preview_wake()
}

/// Why the pet cannot be put to sleep now (`None` when it can).
pub fn sleep_refusal() -> Option<super::engine::doses::Reason> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    state.sleep_refusal()
}

/// Why a mini-game cannot start now.
pub fn game_refusal(game: super::engine::MiniGame) -> Option<super::engine::doses::Reason> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    state.game_refusal(game)
}

/// Why a Battle cannot start or be accepted now (`None` when it can).
pub fn battle_refusal() -> Option<super::engine::doses::Reason> {
    let state = unsafe { (*GAME.get()).as_ref() }?;
    state.battle_refusal()
}

/// Tick the display must be redrawn by for the sleep-animation floor to end
/// (`SLEEP_ANIM_UNTIL_TICK`), or `u32::MAX` when none is pending.
pub fn sleep_anim_deadline_tick() -> u32 {
    let until = SLEEP_ANIM_UNTIL_TICK.load(Ordering::Relaxed);
    if now_tick() < until { until } else { u32::MAX }
}

#[cfg(test)]
pub(super) fn clear_sleep_anim_for_test() {
    SLEEP_ANIM_UNTIL_TICK.store(0, Ordering::Relaxed);
}

/// Get the tick at which the engine wants to be woken next.
pub fn next_wake_tick() -> u32 {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.next_wake_tick(),
        None => u32::MAX,
    }
}

// ---------------------------------------------------------------------------
// Player actions
// ---------------------------------------------------------------------------

/// Execute a player action.  Returns true if the action was accepted.
pub fn feed(food: super::engine::FoodKind) -> bool {
    with_state(|s| s.feed(food))
}

pub fn wash() -> bool {
    with_state(|s| s.wash())
}

pub fn relax() -> bool {
    with_state(|s| s.relax())
}

pub fn heal() -> bool {
    with_state(|s| s.heal())
}

pub fn play() -> bool {
    with_state(|s| s.play())
}

/// Send the pet to "Only pets" to earn HAX. Timed action like Play; only
/// meaningful (and only surfaced in the menu) when `money_enabled()`.
pub fn only_pets() -> bool {
    with_state(|s| s.only_pets())
}

pub fn exercise() -> bool {
    with_state(|s| s.exercise())
}

pub fn medicate() -> bool {
    with_state(|s| s.medicate())
}

pub fn ozempic() -> bool {
    with_state(|s| s.ozempic())
}

pub fn drink(kind: super::engine::DrinkKind) -> bool {
    with_state(|s| s.drink(kind))
}

/// Apply a happiness boost from meeting/spending time with a mesh friend.
/// `big` = true for a brand-new friend, false for a cooldown-gated
/// recurring reunion. No-op if no game is active.
pub fn friend_boost(big: bool) {
    with_game_now(|s| s.friend_boost(big));
}

/// A fight was declined: mood target -3 for an hour.
pub fn declined() {
    with_game_now(|s| s.declined());
}

pub fn rehab() -> bool {
    with_state(|s| s.rehab())
}

// ---------------------------------------------------------------------------
// Debug menu — see `engine::debug` for what each poke does.
// ---------------------------------------------------------------------------

/// Run a debug change on the active game, brought up to now first so it
/// lands on the pet as it is. The change asks for a save itself; the frame
/// loop re-reads `next_wake_tick` on the redraw that follows the menu
/// closing, the same way it does after a care action, so no extra re-arm is
/// needed.
fn with_debug<T>(f: impl FnOnce(&mut GameState) -> T) -> Option<T> {
    let state = unsafe { (*GAME.get()).as_mut()? };
    let now = now_tick();
    state.sync_clock(now, local_minute());
    state.update(now);
    Some(f(state))
}

/// Set one thing on the pet. `Err` carries the reason it did not apply.
pub fn debug_poke(p: super::engine::DebugPoke) -> Result<(), super::engine::doses::Reason> {
    with_debug(|s| s.debug_poke(p)).unwrap_or(Err(super::engine::doses::Reason::Refused(
        super::engine::doses::Refusal::NoNeed,
    )))
}

/// Fast-forward the pet `minutes`, through `update` like real time.
pub fn debug_skip_minutes(minutes: u32) {
    with_debug(|s| s.debug_skip_minutes(minutes));
}

/// What the engine is doing now (None without a game).
pub fn debug_info() -> Option<super::engine::DebugInfo> {
    with_debug(|s| s.debug_info())
}

/// Reset both halves of the mesh Battle record — this pet's own
/// lifetime tally and every known friend's head-to-head numbers — back
/// to zero. See `GameState::debug_reset_battle_record`.
pub fn debug_reset_battle_record() {
    with_game(|s| s.debug_reset_battle_record());
    super::friends::reset_all_battle_records();
}

pub fn sleep() -> bool {
    let started = with_state(|s| s.sleep());
    if started {
        SLEEP_ANIM_UNTIL_TICK.store(now_tick().saturating_add(4), Ordering::Relaxed);
    }
    started
}

pub fn wake() -> bool {
    with_state(|s| s.wake())
}

pub fn hibernate() -> bool {
    with_state(|s| s.hibernate())
}

pub fn wake_from_hibernation() -> bool {
    with_state(|s| s.wake_from_hibernation())
}

/// Read-only accessor: is the pet currently hibernating?
///
/// Returns `false` when no game has been started yet.  Used by the modal
/// dispatcher to suppress action modals (Feed / Heal / Play / Rest) and
/// every mini-game while hibernating — only Stats and Defrost remain
/// reachable.
pub fn is_hibernating() -> bool {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.is_some_and(|s| s.hibernating)
}

/// Award the mini-game win (`GRADE_WIN`) or draw (`GRADE_DRAW`): fun dose, HAX and cooldown.
/// Starts only the matching game's cooldown so other games stay playable.
pub fn award_inspiration(game: super::engine::MiniGame, grade: u8) {
    with_game_now(|s| {
        s.award_inspiration(game, grade);
        // Celebratory jingle on a mini-game win — the main
        // active-engagement reward previously had no audio, only a toast.
        // Demo mode is silent, so the same gate as every other buzz.
        #[cfg(feature = "embassy-core")]
        if win_jingle_due(crate::GAME_MUTE.load(Ordering::Relaxed), s.demo.on) {
            crate::fw::buzzer::play(crate::MINIGAME_WIN_INDEX);
        }
    });
}

/// Whether the mini-game win jingle plays: the one buzz rule, always "rising".
#[cfg_attr(not(any(test, feature = "embassy-core")), allow(dead_code))]
fn win_jingle_due(muted: bool, demo: bool) -> bool {
    alert_due(muted, demo, true)
}

/// Start a new generation (after pet has left or manual reset).
/// Records the current pet in the Unicorn Realm before replacing it.
/// `money_enabled` (and `hard_mode`) are chosen fresh per generation,
/// same as `kind`.
pub fn new_generation(kind: super::engine::PetKind, money_enabled: bool, hard_mode: bool) {
    use super::engine::Phase;
    super::battle_link::cancel();
    let state = unsafe { (*GAME.get()).as_mut() };
    let Some(s) = state else {
        // Nothing to succeed — "Reset Pet" can be confirmed on a badge
        // that never started a game (or was reset earlier). This used to
        // be a silent no-op: the selection screen closed and left the
        // player back on the start screen with no game started, so the
        // pet had to be picked a second time.
        start_new_game(kind, money_enabled, hard_mode);
        return;
    };
    {
        // Record the departing pet exactly once.
        //   - Hatching: never lived, nothing to record.
        //   - Gone: the natural-death path already handles it. If `realm_pending`
        //     is still set the save cycle hasn't run yet, so record it here (and
        //     clear the flag) before we overwrite the state; if it's already
        //     cleared, save_if_needed recorded it — recording again would
        //     duplicate the Unicorn Realm entry.
        //   - Active/Leaving (manual "Reset Pet" on a living pet): never recorded
        //     by the death path, so record it here.
        let should_record = match s.phase {
            Phase::Hatching => false,
            Phase::Gone => s.realm_pending,
            // A hidden Active pet was recorded by `retire_hidden_pet`.
            _ => !is_hidden_kind(s.pet_kind),
        };
        let departing =
            should_record.then(|| PetRecord::from_game_state(s, pet_name_bytes_sync()));

        // Consume any pending death-record flag so save_if_needed doesn't also
        // record the (about-to-be-replaced) pet.
        s.realm_pending = false;

        let seed = now_tick() as u64 ^ 0xDEAD_BEEF;
        s.new_generation(seed, kind);
        clear_diary();
        forget_boot_gap(&BOOT_SAVED_UNIX, &CATCH_UP_DONE);
        s.money_enabled = money_enabled;
        s.hard_mode = hard_mode;
        s.last_update_tick = now_tick();

        // Push AFTER the state reset (so new_generation() can't clobber the
        // flag) and persist the realm buffer via REALM_DIRTY — the record was
        // taken from the old state above, not from the current fresh egg.
        if let Some(record) = departing {
            let realm = unsafe { &mut *REALM.get() };
            realm.push(record);
            REALM_DIRTY.store(true, Ordering::Relaxed);
        }
    }
}

/// What [`retire_hidden_pet`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Retired {
    Nothing,
    ToRealm,
    Egg,
}

/// A kind the badge cannot show: dropped from the roster for having no idle
/// sprite, while the roster has other pets.  A kind that `PETS.CFG` merely
/// omits is not hidden.
pub fn is_hidden_kind(kind: super::engine::PetKind) -> bool {
    super::pet_registry::is_hidden(kind)
}

/// Whether two records are the same pet.  Identity only: the age may differ
/// between two boots (catch-up), so it is not compared.
fn same_pet(a: &PetRecord, b: &PetRecord) -> bool {
    a.generation == b.generation
        && a.pet_kind == b.pet_kind
        && a.vitality == b.vitality
        && a.curiosity == b.curiosity
        && a.resilience == b.resilience
        && a.name[..a.name_len as usize] == b.name[..b.name_len as usize]
}

// retire_hidden_pet: begin
/// At boot, after `drop_without_sprites` and `init`: a pet whose kind has no
/// sprites on this badge moves to the Unicorn Realm (an egg goes back to pet
/// selection).  Does nothing while the sprite catalogue is incomplete.
pub fn retire_hidden_pet() -> Retired {
    use super::engine::Phase;
    if !super::sprite_loader::catalogue_complete() {
        return Retired::Nothing;
    }
    let state = unsafe { (*GAME.get()).as_mut() };
    let Some(s) = state else {
        return Retired::Nothing;
    };
    if !is_hidden_kind(s.pet_kind) {
        return Retired::Nothing;
    }
    match s.phase {
        Phase::Gone => Retired::Nothing,
        Phase::Hatching => {
            // Freeze the egg: it must not hatch behind pet selection and ask
            // for a name.  `new_generation` replaces the whole state.
            s.hibernate();
            super::pet_select::open_new_generation();
            Retired::Egg
        }
        Phase::Active | Phase::Leaving => {
            let record = PetRecord::from_game_state(s, pet_name_bytes_sync());
            let realm = unsafe { &mut *REALM.get() };
            // Newest first.  A power cut between the realm and state writes
            // leaves the record in the realm with the pet still alive.
            if realm.count == 0 || !same_pet(&realm.pets[0], &record) {
                realm.push(record);
            }
            REALM_DIRTY.store(true, Ordering::Relaxed);
            s.retire();
            Retired::ToRealm
        }
    }
}
// retire_hidden_pet: end

/// Get the current pet's kind (defaults to sprite prefix 0 if no game).
pub fn pet_kind() -> super::engine::PetKind {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.pet_kind,
        None => super::engine::PetKind::DEFAULT,
    }
}

/// Get the current pet's rolled traits (vitality, curiosity, resilience).
/// Returns `None` if no game is active.
pub fn pet_traits() -> Option<(u16, u16, u16)> {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.map(|s| (s.vitality, s.curiosity, s.resilience))
}

/// Mesh Battle record `(wins, losses)` of the league the current pet is in
/// (the demo record in demo mode), or `None` if no game is active.
pub fn battle_record() -> Option<(u16, u16)> {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.map(|s| s.battle_record())
}

/// A result for a fight the current pet did not take part in: the record
/// only. No-op if no game is active.
pub fn record_only(won: bool) {
    with_game_now(|s| s.record_only(won));
}

/// The current pet as the fight sees it, brought up to now.
pub fn snapshot() -> Option<super::engine::combat::Snapshot> {
    let mut snap = None;
    with_game_now(|s| snap = Some(s.snapshot()));
    snap
}

/// How the current pet rates against the reference pet.
pub fn battle_form() -> Option<super::engine::combat::Form> {
    snapshot().map(|s| super::engine::combat::form(&s))
}

/// Get the current generation (defaults to 0 if no game).
pub fn pet_generation() -> u16 {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.generation,
        None => 0,
    }
}

/// Get the current HAX balance (defaults to 0 if no game).
pub fn money() -> u32 {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.money,
        None => 0,
    }
}

/// Whether HAX is live for the current pet: money mode on, demo or not
/// (defaults to false if no game).
pub fn money_enabled() -> bool {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.money_live(),
        None => false,
    }
}

/// Whether the pet's money mode was chosen at creation, demo or not (false
/// if no game). Today the same as `money_enabled()` (demo no longer freezes
/// HAX); menu rows that exist because of the mode (Only pets) read it.
pub fn money_mode() -> bool {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.is_some_and(|s| s.money_enabled)
}

/// Whether Hard (US) prices are in effect for the current pet (defaults
/// to false if no game). Only meaningful when `money_enabled()` is also
/// true.
pub fn hard_mode() -> bool {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.hard_mode,
        None => false,
    }
}

/// Run `f` on the active game; no-op when no game has been started.
fn with_game(f: impl FnOnce(&mut GameState)) {
    if let Some(s) = unsafe { (*GAME.get()).as_mut() } {
        f(s);
    }
}

/// Bring `s` up to `now`, then apply the event `f`. An event is applied to
/// the pet as it is, never at a stale tick base (design 19.4): the gap since
/// the last frame is played first, so a cooldown or dose the event sets is
/// not eaten by that gap.
fn event_at(s: &mut GameState, now: u32, f: impl FnOnce(&mut GameState)) {
    s.sync_clock(now, local_minute());
    s.update(now);
    f(s);
}

/// [`with_game`] for state-changing events: catches up to now first.
pub(super) fn with_game_now(f: impl FnOnce(&mut GameState)) {
    with_game(|s| event_at(s, now_tick(), f));
}

pub(super) fn with_state(f: impl FnOnce(&mut GameState) -> bool) -> bool {
    let state = unsafe { (*GAME.get()).as_mut() };
    match state {
        Some(s) => f(s),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Unicorn Realm — past pets
// ---------------------------------------------------------------------------

/// Get the number of past pets in the Unicorn Realm.
pub fn realm_count() -> u8 {
    unsafe { (*REALM.get()).count }
}

/// Get a past pet record by index (0 = most recent).
pub fn realm_pet(index: usize) -> Option<PetRecord> {
    let realm = unsafe { &*REALM.get() };
    if index < realm.count as usize {
        Some(realm.pets[index])
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Save to flash
// ---------------------------------------------------------------------------

/// Save the game state to ekv if enough time has passed.
/// Returns true if a save was performed.
/// Available in any firmware build that pulls in `embassy-base` (which
/// brings the KV store).  Stubbed out for the simulator below.
#[cfg(feature = "embassy-core")]
pub async fn save_if_needed() -> bool {
    // Friends list has its own dirty flag and KV namespace, independent of
    // the main game-state save below — flush it even if no pet is active
    // (a beacon can still be received while between generations).
    super::friends::save_if_needed().await;

    let state = unsafe { (*GAME.get()).as_mut() };
    let Some(state) = state else {
        return false;
    };

    // Natural-death path: the pet just went Gone (engine set realm_pending) and
    // no manual reset intervened — record the current pet in the Unicorn Realm.
    if state.realm_pending {
        state.realm_pending = false;
        let record = PetRecord::from_game_state(state, pet_name_bytes_sync());
        let realm = unsafe { &mut *REALM.get() };
        realm.push(record);
        REALM_DIRTY.store(true, Ordering::Relaxed);
        defmt::info!(
            "game: pet recorded in Unicorn Realm (gen={})",
            record.generation
        );
    }

    // Persist the realm buffer if it changed — either from the death-record
    // above or from a manual reset in new_generation (which pushed + flagged it).
    if REALM_DIRTY.swap(false, Ordering::Relaxed) {
        let realm = unsafe { &*REALM.get() };
        let buf = realm.to_bytes();
        let ns = crate::fw::kv::namespace("game");
        if ns.set("realm", &buf, true).await.is_err() {
            REALM_DIRTY.store(true, Ordering::Relaxed); // retry next cycle
            defmt::warn!("game: realm save failed");
        }
    }

    if !state.needs_save() {
        return false;
    }

    // Note: `mark_saved` below can clear a save flag set between this write
    // and now; accepted, the state is self-consistent and persists at the
    // next autosave.
    state.saved_unix = saved_unix_to_write(
        crate::unix_now(),
        BOOT_SAVED_UNIX.load(Ordering::Relaxed),
        (crate::platform::uptime_ms() / 1000) as u32,
        CATCH_UP_DONE.load(Ordering::Relaxed),
    );
    let buf = state.to_bytes();
    let ns = crate::fw::kv::namespace("game");
    match ns.set(SAVE_KEY, &buf, true).await {
        Ok(()) => {
            state.mark_saved();
            // Also persist pet name alongside state.
            let name = pet_name_bytes_sync();
            let _ = ns.set(NAME_KEY, name, true).await;
            true
        }
        Err(_) => {
            defmt::warn!("game: save failed");
            false
        }
    }
}

/// No-op when ekv is not available (simulator build).
#[cfg(not(feature = "embassy-core"))]
pub async fn save_if_needed() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{
        Severity, alert_due, apply_boot_catch_up, boot_catch_up_minutes, catch_up_minutes,
        current_severity, deferred_catch_up_minutes, event_at, forget_boot_gap, onset_alert_due,
        saved_unix_to_write,
    };
    use crate::game::engine::status::{Word, status_line};
    use crate::game::engine::needs::P;
    use crate::game::engine::{Fought, GameState, PetKind, Phase};
    use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    fn pet() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s
    }

    #[test]
    fn miserable_words_animate_but_do_not_buzz() {
        let mut s = pet();
        assert!(current_severity(&s) == Severity::Neutral);
        s.dirt = (70 * P) as u16; // Grubby, not in danger
        assert!(current_severity(&s) == Severity::Neutral);
        s.fun = (5 * P) as u16; // Bored
        assert!(current_severity(&s) == Severity::Neutral);
    }

    #[test]
    fn real_problems_warn_and_danger_is_severe() {
        let mut s = pet();
        s.fullness = (20 * P) as u16; // Hungry
        assert!(current_severity(&s) == Severity::Warning);
        s.fullness = (10 * P) as u16;
        assert!(current_severity(&s) == Severity::Severe);
        let mut s = pet();
        s.dirt = (95 * P) as u16; // Grubby but in danger
        assert!(current_severity(&s) == Severity::Severe);
    }

    #[test]
    fn band_edge_grades_never_buzz() {
        // "peckish" and "sleepy" are information, not an alarm (spec 5.7).
        let mut s = pet();
        s.fullness = (30 * P) as u16;
        assert_eq!(status_line(&s).first, Word::Peckish);
        assert!(current_severity(&s) == Severity::Neutral);
        let mut s = pet();
        s.energy = (30 * P) as u16;
        assert_eq!(status_line(&s).first, Word::Sleepy);
        assert!(current_severity(&s) == Severity::Neutral);
    }

    #[test]
    fn graded_problems_buzz() {
        let mut s = pet();
        s.hydration = (20 * P) as u16;
        assert_eq!(status_line(&s).first, Word::Thirsty);
        assert!(current_severity(&s) == Severity::Warning);
        let mut s = pet();
        s.energy = (5 * P) as u16;
        assert_eq!(status_line(&s).first, Word::Exhausted);
        assert!(current_severity(&s) == Severity::Warning);
    }

    #[test]
    fn every_danger_word_is_severe() {
        for (name, s) in [
            (
                "starving",
                {
                    let mut s = pet();
                    s.fullness = (5 * P) as u16;
                    s
                },
            ),
            (
                "parched",
                {
                    let mut s = pet();
                    s.hydration = (5 * P) as u16;
                    s
                },
            ),
            (
                "wasted",
                {
                    let mut s = pet();
                    s.booze = (95 * P) as u16;
                    s
                },
            ),
            (
                "filthy",
                {
                    let mut s = pet();
                    s.dirt = (95 * P) as u16;
                    s
                },
            ),
            (
                "poorly",
                {
                    let mut s = pet();
                    s.health = (5 * P) as u16;
                    s
                },
            ),
        ] {
            let line = status_line(&s);
            assert!(line.danger, "{name}");
            assert!(line.alerts(), "{name}");
            assert!(current_severity(&s) == Severity::Severe, "{name}");
        }
    }

    #[test]
    fn demo_never_buzzes() {
        // The one rule every buzz site asks: severity rise, the funny ending
        // and both onset alerts. Demo is silent whatever else is true.
        for muted in [false, true] {
            for upward in [false, true] {
                assert!(!alert_due(muted, true, upward), "demo {muted} {upward}");
            }
        }
        assert!(alert_due(false, false, true));
        assert!(!alert_due(true, false, true), "muted");
        assert!(!alert_due(false, false, false), "not rising");
    }

    #[test]
    fn the_win_jingle_is_silent_in_demo() {
        for muted in [false, true] {
            assert!(!super::win_jingle_due(muted, true), "demo, muted {muted}");
        }
        assert!(super::win_jingle_due(false, false));
        assert!(!super::win_jingle_due(true, false), "muted");
    }

    /// A pet in the cause toast tests: installed, with one cause queued.
    fn install_with_cause(demo_on: bool, pause_min: u8) {
        let mut s = pet();
        s.demo.on = demo_on;
        s.demo.pause_min = pause_min;
        s.causes.push(crate::game::engine::causes::Cause::FoodComa);
        super::set_state_for_test(Some(s));
    }

    #[test]
    fn pet_causes_do_not_toast() {
        let _guard = crate::game::lock_test_state();
        install_with_cause(true, 0);
        super::cycle();
        assert_eq!(super::diary().len(), 1, "the diary has it");
        assert_eq!(crate::game::current_toast().unwrap_or(crate::game::Toast::None), crate::game::Toast::None);
        super::set_state_for_test(None);
    }

    #[test]
    fn player_causes_toast_in_demo() {
        let _guard = crate::game::lock_test_state();
        install_with_cause(true, 30);
        super::cycle();
        assert_eq!(super::diary().len(), 1);
        assert_eq!(crate::game::current_toast().unwrap_or(crate::game::Toast::None), crate::game::Toast::Cause);
        super::set_state_for_test(None);
    }

    #[test]
    fn causes_toast_with_demo_off() {
        let _guard = crate::game::lock_test_state();
        install_with_cause(false, 0);
        super::cycle();
        assert_eq!(crate::game::current_toast().unwrap_or(crate::game::Toast::None), crate::game::Toast::Cause);
        super::set_state_for_test(None);
    }

    #[test]
    fn onset_alert_rules() {
        // Awake at boot with the condition: alerts once (re-fire on boot is intended).
        let seen = AtomicBool::new(false);
        assert!(onset_alert_due(&seen, true, false));
        assert!(!onset_alert_due(&seen, true, false));

        // Hibernating: never, however often it is asked.
        let seen = AtomicBool::new(false);
        assert!(!onset_alert_due(&seen, true, true));
        assert!(!onset_alert_due(&seen, true, true));
        // Defrost with the condition still present: once.
        assert!(onset_alert_due(&seen, true, false));
        assert!(!onset_alert_due(&seen, true, false));

        // No condition: nothing.
        let seen = AtomicBool::new(false);
        assert!(!onset_alert_due(&seen, false, false));
        assert!(!onset_alert_due(&seen, false, true));
    }

    #[test]
    fn an_event_sees_the_gap_applied_first() {
        // The last frame was at tick 0; the event lands 18 ticks (3 min) later.
        let mut s = pet();
        s.last_update_tick = 0;
        event_at(&mut s, 18, |s| {
            s.fight_over(Fought { won: true, hp_left_pct: 50, costs: true, paid: false });
        });
        assert_eq!(s.last_update_tick, 18);
        // The battle cooldown the event set was not consumed by the gap.
        assert_eq!(s.cooldown_battle, crate::game::engine::thresholds::BATTLE_COOLDOWN());

        // Applied at the stale base instead, the next update eats it.
        let mut stale = pet();
        stale.last_update_tick = 0;
        stale.fight_over(Fought { won: true, hp_left_pct: 50, costs: true, paid: false });
        stale.update(18);
        assert!(stale.cooldown_battle < s.cooldown_battle);
    }

    #[test]
    fn boot_catch_up_reports_the_minutes_so_the_display_can_wake() {
        let done = AtomicBool::new(false);
        let mut s = pet();
        assert_eq!(boot_catch_up_minutes(&mut s, &done, 1000, 1000 + 3600, 0), Some(60));
        // Taken once; a re-sync does nothing and reports nothing.
        assert_eq!(boot_catch_up_minutes(&mut s, &done, 1000, 1000 + 7200, 0), None);
        let done = AtomicBool::new(false);
        // Clock unknown for the saved time: taken, nothing applied, no wake.
        assert_eq!(boot_catch_up_minutes(&mut s, &done, 0, 5000, 0), Some(0));
    }

    /// Unix time whose local minute of the day is `minute` (day `day`).
    fn unix_at(day: u32, minute: u32) -> u32 {
        (day * 86_400 + minute * 60).wrapping_add_signed(-(crate::tz_offset_secs() as i32))
    }

    #[test]
    fn a_catch_up_across_bedtime_arms_the_bed_at_the_right_minute() {
        use crate::game::engine::thresholds as t;
        const HOUR: u32 = 60;
        // Off from 23:00 to 06:00: the replay starts at night, tired pet. Bed
        // was due at 23:00, so the pet went to bed at once and the clock now
        // counts to getting up (07:00 plus the spread).
        let mut s = pet();
        s.demo.on = true;
        s.energy = (60 * crate::game::engine::needs::P) as u16;
        let done = AtomicBool::new(false);
        let (saved, boot) = (unix_at(100, 23 * HOUR), unix_at(101, 6 * HOUR));
        assert_eq!(boot_catch_up_minutes(&mut s, &done, saved, boot, 0), Some(7 * HOUR));
        assert!(s.is_sleeping, "the pet went to bed at 23:00");
        assert!(s.demo.clock_armed);
        let up = t::DEMO_UP_AT_MIN() as u32 - 6 * HOUR;
        assert!(
            (up..=up + t::DEMO_UP_SPREAD_MIN() as u32).contains(&(s.demo.clock_min as u32)),
            "{} minutes to getting up",
            s.demo.clock_min
        );
        // The pet's own minute is the time of day at the tick base again.
        assert_eq!(s.day_min, Some(super::minute_of_day(boot)));

        // Off from 07:00 to 08:00, rested: bed tonight, 14 h from 08:00.
        let mut s = pet();
        s.demo.on = true;
        s.energy = (99 * crate::game::engine::needs::P) as u16;
        let done = AtomicBool::new(false);
        let (saved, boot) = (unix_at(100, 7 * HOUR), unix_at(100, 8 * HOUR));
        boot_catch_up_minutes(&mut s, &done, saved, boot, 0);
        let to_bed = t::DEMO_BED_AT_MIN() as u32 - 8 * HOUR;
        let want = to_bed..=to_bed + t::DEMO_BED_SPREAD_MIN() as u32;
        assert!(want.contains(&(s.demo.clock_min as u32)), "{}", s.demo.clock_min);
    }

    #[test]
    fn catch_up_is_capped_and_safe() {
        assert_eq!(catch_up_minutes(0, 5000), 0);
        assert_eq!(catch_up_minutes(5000, 0), 0);
        assert_eq!(catch_up_minutes(1000, 900), 0);
        assert_eq!(catch_up_minutes(1000, 1000 + 3600), 60);
        assert_eq!(catch_up_minutes(1000, 1000 + 86400 * 3), 720);
    }

    #[test]
    fn catch_up_keeps_the_tick_base() {
        let mut s = pet();
        s.last_update_tick = 0;
        let age0 = s.age_ticks;
        s.catch_up(60);
        assert_eq!(s.last_update_tick, 0);
        assert_eq!(s.age_ticks, age0 + 360);
        // The next real tick is not counted as zero elapsed.
        s.update(6);
        assert_eq!(s.age_ticks, age0 + 366);
        assert_eq!(s.last_update_tick, 6);
    }

    #[test]
    fn catch_up_does_not_age_a_hibernating_pet() {
        let mut s = pet();
        s.hibernating = true;
        let age0 = s.age_ticks;
        let fullness = s.fullness;
        s.catch_up(600);
        assert_eq!(s.hibernate_ticks, 3600);
        assert_eq!(s.age_ticks, age0);
        assert_eq!(s.fullness, fullness);
        assert_eq!(s.last_update_tick, 0);
    }

    #[test]
    fn catch_up_carries_the_sub_minute_remainder() {
        let mut s = pet();
        let age0 = s.age_ticks;
        s.catch_up(1);
        assert_eq!(s.age_ticks, age0 + 6);
        assert_eq!(s.last_update_tick, 0);
        s.update(3); // half a minute of real time
        assert_eq!(s.tick_rem, 3);
        s.update(6);
        assert_eq!(s.tick_rem, 0);
    }

    #[test]
    fn clock_known_later_catches_up_the_off_time_only() {
        // Saved at 1000, booted at 1000 + 3600, clock set 600 s after boot.
        assert_eq!(deferred_catch_up_minutes(1000, 1000 + 3600 + 600, 600), 60);
        assert_eq!(deferred_catch_up_minutes(1000, 1000 + 3600, 0), 60);
    }

    #[test]
    fn clock_before_saved_is_ignored() {
        assert_eq!(deferred_catch_up_minutes(5000, 4000, 0), 0);
        assert_eq!(deferred_catch_up_minutes(5000, 5300, 600), 0);
        assert_eq!(deferred_catch_up_minutes(0, 9000, 10), 0);
        assert_eq!(deferred_catch_up_minutes(1000, 100, 600), 0); // now < uptime
    }

    #[test]
    fn catch_up_runs_once_per_boot() {
        let done = AtomicBool::new(false);
        let mut s = pet();
        s.saved_unix = 1000;
        assert!(apply_boot_catch_up(&mut s, &done, 1000, 1000 + 3600, 0));
        assert_eq!(s.age_ticks, 360);
        assert!(s.needs_save());
        let age = s.age_ticks;
        assert!(!apply_boot_catch_up(&mut s, &done, 1000, 1000 + 7200, 0));
        assert_eq!(s.age_ticks, age);
    }

    #[test]
    fn saved_unix_to_write_branches() {
        // Clock known: the clock.
        assert_eq!(saved_unix_to_write(Some(9000), 1000, 600, false), 9000);
        assert_eq!(saved_unix_to_write(Some(9000), 0, 0, true), 9000);
        // No clock, catch-up pending, boot value known: lower bound.
        assert_eq!(saved_unix_to_write(None, 1000, 600, false), 1600);
        assert_eq!(saved_unix_to_write(None, u32::MAX, 600, false), u32::MAX);
        // Otherwise unknown.
        assert_eq!(saved_unix_to_write(None, 0, 600, false), 0);
        assert_eq!(saved_unix_to_write(None, 1000, 600, true), 0);
    }

    #[test]
    fn save_before_clock_keeps_the_catch_up() {
        let done = AtomicBool::new(false);
        let mut s = pet();
        s.saved_unix = 1000; // loaded
        let boot_saved = s.saved_unix;
        // A save at uptime 600, no clock yet, rewrites the field.
        s.saved_unix = saved_unix_to_write(None, boot_saved, 600, false);
        assert_eq!(s.saved_unix, 1600);
        // Booted at 1000 + 3600, clock set 900 s after boot.
        assert!(apply_boot_catch_up(&mut s, &done, boot_saved, 1000 + 3600 + 900, 900));
        assert_eq!(s.age_ticks, 60 * 6);
    }

    #[test]
    fn reboot_before_clock_counts_both_off_gaps() {
        // Boot 1 happened 1800 s after the save at 1000, clock never set.
        // It ran 600 s, saved (lower bound 1600), then the badge was off
        // 3600 s. Boot 2 is at real 1000 + 1800 + 600 + 3600 = 7000.
        let written = saved_unix_to_write(None, 1000, 600, false);
        assert_eq!(written, 1600);
        let done = AtomicBool::new(false);
        let mut s = pet();
        s.saved_unix = written;
        assert!(apply_boot_catch_up(&mut s, &done, written, 7000, 0));
        // 90 min = 30 (gap 1) + 60 (gap 2); the 10 min session is not counted.
        assert_eq!(s.age_ticks, 90 * 6);
    }

    #[test]
    fn new_game_forgets_the_old_off_gap() {
        let boot = AtomicU32::new(1000);
        let done = AtomicBool::new(false);
        assert_eq!(saved_unix_to_write(None, boot.load(Ordering::Relaxed), 600, false), 1600);
        forget_boot_gap(&boot, &done);
        let b = boot.load(Ordering::Relaxed);
        let d = done.load(Ordering::Relaxed);
        assert_eq!(saved_unix_to_write(None, b, 600, d), 0);
        // The clock arriving later does not age the fresh egg.
        let mut egg = pet();
        let age = egg.age_ticks;
        assert!(!apply_boot_catch_up(&mut egg, &done, b, 1000 + 3600, 0));
        assert_eq!(egg.age_ticks, age);
    }
    use super::{demo_care_label, demo_menu_label, is_demo, set_state_for_test, toggle_demo, toggle_demo_care};
    use crate::game::engine::doses::{Reason, Refusal};

    #[test]
    fn the_toggle_says_why() {
        let _guard = crate::game::lock_test_state();
        let mut s = pet();
        s.health = (30 * P) as u16;
        set_state_for_test(Some(s));
        assert_eq!(toggle_demo(), Err(Reason::Refused(Refusal::FixMeFirst)));
        assert_eq!(demo_menu_label(), "Demo: fix me first");
        assert!(!is_demo(), "a refused toggle changes nothing");
        let mut s = pet();
        s.health = (60 * P) as u16;
        set_state_for_test(Some(s));
        assert_eq!(demo_menu_label(), "Demo mode: off");
        assert_eq!(toggle_demo(), Ok(true));
        assert_eq!(demo_menu_label(), "Demo mode: on");
        assert!(is_demo());
        assert_eq!(toggle_demo(), Ok(false));
        assert_eq!(demo_menu_label(), "Demo mode: off");
        // An egg, a pet that left and no pet at all have no demo to switch.
        set_state_for_test(Some(GameState::new_egg(7, PetKind(0))));
        assert_eq!(demo_menu_label(), "Demo: no pet");
        assert_eq!(toggle_demo(), Err(Reason::Egg));
        set_state_for_test(None);
        assert_eq!(demo_menu_label(), "Demo: no pet");
        assert!(toggle_demo().is_err());
    }

    /// The switch only ever answers with a reason the row has a word for: an
    /// unmapped one would read "Demo: no pet" and hide the real cause.
    #[test]
    fn the_demo_refusal_has_a_label() {
        use super::{
            DEMO_LABEL_FIX_ME_FIRST, DEMO_LABEL_FROZEN, DEMO_LABEL_NO_PET, demo_refusal_label,
        };
        let mut states = vec![GameState::new_egg(7, PetKind(0))];
        for (health, hibernating, phase, busy, leaving) in [
            (30, false, Phase::Active, false, false),
            (60, false, Phase::Active, false, false),
            (60, false, Phase::Active, true, false),
            (60, true, Phase::Active, false, false),
            (60, false, Phase::Gone, false, false),
            (60, false, Phase::Leaving, false, true),
            (0, false, Phase::Active, false, false),
        ] {
            let mut s = GameState::new_egg(7, PetKind(0));
            s.phase = phase;
            s.health = (health * P) as u16;
            s.hibernating = hibernating;
            if leaving {
                s.leaving_min = 10;
            }
            if busy {
                assert!(s.drink(crate::game::engine::DrinkKind::Water));
            }
            states.push(s);
        }
        let mut reasons = 0;
        for s in states {
            let Some(why) = s.demo_refusal() else { continue };
            reasons += 1;
            assert!(
                matches!(
                    why,
                    Reason::Refused(Refusal::FixMeFirst)
                        | Reason::Hibernating
                        | Reason::Egg
                        | Reason::Gone
                ),
                "demo_refusal answered {why:?}, which the row has no word for"
            );
        }
        assert!(reasons >= 5, "{reasons} refusals checked");
        assert_eq!(
            demo_refusal_label(Reason::Refused(Refusal::FixMeFirst)),
            DEMO_LABEL_FIX_ME_FIRST
        );
        assert_eq!(demo_refusal_label(Reason::Hibernating), DEMO_LABEL_FROZEN);
        assert_eq!(demo_refusal_label(Reason::Egg), DEMO_LABEL_NO_PET);
        assert_eq!(demo_refusal_label(Reason::Gone), DEMO_LABEL_NO_PET);
    }

    #[test]
    fn care_defaults_to_instant() {
        let _guard = crate::game::lock_test_state();
        set_state_for_test(Some(pet()));
        assert_eq!(demo_care_label(), "Demo care: instant");
        assert!(toggle_demo_care());
        assert_eq!(demo_care_label(), "Demo care: animated");
        let bytes = unsafe { (*super::GAME.get()).as_ref() }.unwrap().to_bytes();
        let back = GameState::from_bytes(&bytes).unwrap();
        assert!(back.demo.animated);
        set_state_for_test(Some(back));
        assert_eq!(demo_care_label(), "Demo care: animated");
        assert!(!toggle_demo_care());
        assert_eq!(demo_care_label(), "Demo care: instant");
        // The row is there without a pet, at its default, and does nothing.
        set_state_for_test(None);
        assert_eq!(demo_care_label(), "Demo care: instant");
        assert!(!toggle_demo_care());
    }

    #[test]
    fn a_clock_jump_redraws() {
        use super::clock_sync_needs_redraw as r;
        let m = |h: u16, mi: u16| Some(h * 60 + mi);
        assert!(r(None, m(12, 0)));
        assert!(!r(m(12, 0), m(12, 1)));
        assert!(r(m(19, 0), m(20, 50)));
        assert!(!r(m(20, 50), m(19, 0)));
        assert!(r(m(20, 59), m(21, 0)));
    }

    #[test]
    fn lock_test_state_pins_noon() {
        let _guard = crate::game::lock_test_state();
        assert_eq!(super::local_minute(), Some(720));
    }
}

#[cfg(test)]
mod retire_tests {
    use super::*;
    use crate::game::engine::{GameState, PetKind, Phase};
    use crate::game::{pet_registry, pet_select, sprite_loader};

    /// Roster 0, 1, 2 with kind 8 marked dropped; no pet, no pet selection.
    fn setup() -> crate::game::TestState {
        let guard = crate::game::lock_test_state();
        pet_registry::install_test_roster();
        pet_registry::set_dropped_for_test(1 << 8);
        sprite_loader::set_incomplete_for_test(false);
        pet_select::close();
        set_state_for_test(None);
        guard
    }

    fn teardown() {
        pet_registry::set_dropped_for_test(0);
        sprite_loader::set_incomplete_for_test(false);
        pet_select::close();
        set_state_for_test(None);
    }

    fn live(kind: u8) -> GameState {
        let mut s = GameState::new_egg(7, PetKind(kind));
        s.phase = Phase::Active;
        s.age_ticks = 600 * 6;
        // A fresh generation per pet: the dedupe must not see a record an
        // earlier test left in the process-wide Realm.
        static NEXT_GEN: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(100);
        s.generation = NEXT_GEN.fetch_add(1, Ordering::Relaxed);
        s
    }

    fn phase() -> Phase {
        unsafe { (*GAME.get()).as_ref().unwrap().phase }
    }

    #[test]
    fn a_hidden_pet_moves_to_the_realm() {
        let _g = setup();
        let pet = live(8);
        let generation = pet.generation;
        set_state_for_test(Some(pet));
        set_pet_name(b"Jelly");
        let n = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert_eq!(realm_count(), (n + 1).min(10));
        let r = realm_pet(0).unwrap();
        assert_eq!(r.pet_kind, PetKind(8));
        assert_eq!(r.generation, generation);
        assert_eq!(r.age_ticks, 600 * 6);
        assert_eq!(&r.name[..r.name_len as usize], b"Jelly");
        assert_eq!(display_anim(), DisplayAnim::Gone);
        assert!(!can_use_station());
        assert!(is_started(), "the Gone screen needs the game to draw");
        assert!(!take_naming_pending());
        teardown();
    }

    #[test]
    fn the_new_egg_records_it_only_once() {
        let _g = setup();
        set_state_for_test(Some(live(8)));
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        let n = realm_count();
        new_generation(PetKind(0), true, false);
        assert_eq!(realm_count(), n);
        let s = unsafe { (*GAME.get()).as_ref().unwrap() };
        assert_eq!(s.pet_kind, PetKind(0));
        assert_eq!(s.phase, Phase::Hatching);
        teardown();
    }

    #[test]
    fn retiring_twice_records_once() {
        let _g = setup();
        // A non-empty Realm, so the newest-first dedupe is exercised.
        unsafe { (*REALM.get()).push(PetRecord::from_game_state(&live(0), b"Old")) };
        set_state_for_test(Some(live(8)));
        let n = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert_eq!(realm_count(), (n + 1).min(10));
        assert_eq!(retire_hidden_pet(), Retired::Nothing);
        assert_eq!(realm_count(), (n + 1).min(10));
        // Power cut between the realm and state writes: the record is already
        // the newest, the pet is still alive.
        let pet = live(8);
        let rec = PetRecord::from_game_state(&pet, b"Jelly");
        set_state_for_test(Some(pet));
        set_pet_name(b"Jelly");
        unsafe { (*REALM.get()).push(rec) };
        let m = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert_eq!(realm_count(), m, "no second record");
        assert_eq!(phase(), Phase::Gone);
        teardown();
    }

    #[test]
    fn a_hidden_egg_goes_back_to_pet_selection() {
        let _g = setup();
        start_new_game(PetKind(8), true, false);
        assert_eq!(phase(), Phase::Hatching);
        let n = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::Egg);
        assert_eq!(realm_count(), n);
        assert!(pet_select::is_active());
        teardown();
    }

    #[test]
    fn hibernating_and_demo_pets_move_too() {
        let _g = setup();
        let mut s = live(8);
        s.hibernating = true;
        set_state_for_test(Some(s));
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert!(!unsafe { (*GAME.get()).as_ref().unwrap().hibernating });
        let mut s = live(8);
        s.demo.on = true;
        set_state_for_test(Some(s));
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert_eq!(phase(), Phase::Gone);
        teardown();
    }

    #[test]
    fn a_known_pet_is_left_alone() {
        let _g = setup();
        set_state_for_test(Some(live(0)));
        let n = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::Nothing);
        assert_eq!(phase(), Phase::Active);
        assert_eq!(realm_count(), n);
        teardown();
    }

    #[test]
    fn a_kind_the_cfg_omits_is_not_hidden() {
        let _g = setup();
        assert!(!is_hidden_kind(PetKind(5)), "not in the roster, not dropped");
        assert!(is_hidden_kind(PetKind(8)));
        set_state_for_test(Some(live(5)));
        assert_eq!(retire_hidden_pet(), Retired::Nothing);
        teardown();
    }

    #[test]
    fn an_incomplete_catalogue_retires_nothing() {
        let _g = setup();
        set_state_for_test(Some(live(8)));
        sprite_loader::set_incomplete_for_test(true);
        let n = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::Nothing);
        assert_eq!(phase(), Phase::Active);
        assert_eq!(realm_count(), n);
        teardown();
    }

    #[test]
    fn nothing_happens_without_a_roster() {
        // The global roster is install-once, so check the pure rule that
        // `is_hidden_kind` uses, on an empty slice.
        assert!(!pet_registry::hidden_in(&[], 1 << 8, PetKind(8)));
    }

    #[test]
    fn retiring_uses_only_state_v3_and_realm() {
        let _g = setup();
        assert_eq!(crate::game::engine::SAVE_KEY, "state_v3");
        set_state_for_test(Some(live(8)));
        let n = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert_eq!(realm_count(), (n + 1).min(10));
        assert!(REALM_DIRTY.load(Ordering::Relaxed));
        assert!(unsafe { (*GAME.get()).as_ref().unwrap().needs_save() });
        REALM_DIRTY.store(false, Ordering::Relaxed);
        teardown();
    }

    #[test]
    fn a_hidden_egg_stays_frozen_and_is_never_recorded() {
        let _g = setup();
        start_new_game(PetKind(8), true, false);
        let n = realm_count();
        assert_eq!(retire_hidden_pet(), Retired::Egg);
        advance_ticks(10_000);
        for _ in 0..3 {
            cycle();
        }
        assert_eq!(phase(), Phase::Hatching, "the egg did not hatch");
        assert!(!take_naming_pending(), "no naming prompt");
        new_generation(PetKind(0), true, false);
        assert_eq!(realm_count(), n);
        let s = unsafe { (*GAME.get()).as_ref().unwrap() };
        assert_eq!(s.pet_kind, PetKind(0));
        assert_eq!(s.phase, Phase::Hatching);
        assert!(!s.hibernating);
        teardown();
    }

    #[test]
    fn the_power_cut_dedupe_ignores_age_but_not_generation() {
        let _g = setup();
        let mut pet = live(8);
        set_pet_name(b"Jelly");
        let rec = PetRecord::from_game_state(&pet, b"Jelly");
        unsafe { (*REALM.get()).push(rec) };
        let n = realm_count();
        pet.age_ticks += 500; // catch-up changed the age between boots
        set_state_for_test(Some(pet.clone()));
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert_eq!(realm_count(), n, "same pet, no second record");
        let mut other = live(8);
        other.generation = pet.generation + 1000;
        set_state_for_test(Some(other));
        assert_eq!(retire_hidden_pet(), Retired::ToRealm);
        assert_eq!(realm_count(), (n + 1).min(10), "another generation records");
        teardown();
    }
}

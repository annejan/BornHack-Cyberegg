//! Game lifecycle — initialisation, save/restore, and the per-cycle
//! update that embassy.rs calls from the display loop.
//!
//! The game state lives in a static cell, accessed through the functions
//! in this module.  All functions are async (flash access goes through
//! the shared QSPI mutex).

use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};

use super::engine::{DisplayAnim, GameState, PET_NAME_MAX, PetRealm, PetRecord, PetStats};
#[cfg(feature = "embassy-base")]
use super::engine::{REALM_SAVE_SIZE, SAVE_SIZE};

// ---------------------------------------------------------------------------
// Static game state
// ---------------------------------------------------------------------------

/// Wrapper for static mutable access (single-task, sequential).
struct SyncCell<T>(core::cell::UnsafeCell<T>);
unsafe impl<T> Sync for SyncCell<T> {}
impl<T> SyncCell<T> {
    const fn new(v: T) -> Self {
        Self(core::cell::UnsafeCell::new(v))
    }
    fn get(&self) -> *mut T {
        self.0.get()
    }
}

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
    #[cfg(feature = "embassy-base")]
    {
        (embassy_time::Instant::now().as_secs() / 10) as u32
    }
    #[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
    {
        (sim_elapsed_ms() / 10_000) as u32
    }
    #[cfg(not(any(feature = "embassy-base", feature = "simulator")))]
    {
        UPTIME_TICKS.load(Ordering::Relaxed)
    }
}

/// Milliseconds elapsed since the simulator process started.  Pinned
/// to a `OnceLock<Instant>` so every call returns wall-clock-monotonic
/// values from the same epoch.  Used by `now_tick` (10 s per tick)
/// and by the in-game sprite-frame pacer (sub-tick resolution for
/// visible animation).
#[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
pub fn sim_elapsed_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_millis() as u64
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
#[cfg(feature = "embassy-base")]
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
        let mut name_buf = [0u8; PET_NAME_MAX];
        if let Ok(n) = ns.get("name", &mut name_buf).await {
            set_pet_name(&name_buf[..n]);
        }

        // If a saved game is active but has no name, prompt for one.
        if pet_name().is_empty() {
            if let Some(s) = unsafe { (*GAME.get()).as_mut() } {
                if s.phase == super::engine::Phase::Active {
                    s.naming_pending = true;
                }
            }
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
}

#[cfg(feature = "embassy-base")]
async fn try_load() -> Option<GameState> {
    use crate::fw::kv;
    let ns = kv::namespace("game");
    let mut buf = [0u8; SAVE_SIZE];
    if let Ok(n) = ns.get("state", &mut buf).await {
        if n == SAVE_SIZE {
            if let Some(mut s) = GameState::from_bytes(&buf) {
                s.last_update_tick = 0;
                defmt::info!(
                    "game: restored from flash (gen={} age={})",
                    s.generation,
                    s.age_ticks
                );
                return Some(s);
            }
            defmt::warn!("game: corrupt save data");
        }
    }
    None
}

#[cfg(feature = "embassy-base")]
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

#[cfg(not(feature = "embassy-base"))]
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

/// Create a new egg and begin the hatching countdown.
/// Called after the player selects a pet kind on the selection screen.
pub fn start_new_game(kind: super::engine::PetKind) {
    let mut egg = new_egg(kind);
    egg.last_update_tick = now_tick();
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
    let len = PET_NAME_LEN.load(core::sync::atomic::Ordering::Relaxed) as usize;
    let buf = unsafe { &*PET_NAME.get() };
    core::str::from_utf8(&buf[..len]).unwrap_or("")
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
    let stats = state.stats(tick);
    check_severity_transition(state);
    Some(stats)
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

fn current_severity(state: &GameState) -> Severity {
    use super::engine::Phase;
    use super::engine::thresholds::{
        SICK_TRIGGER_DRAINED, SICK_TRIGGER_HUNGER, SICK_TRIGGER_TIRED, WARNING_DRAINED,
        WARNING_HUNGER, WARNING_MISERABLE, WARNING_SICK, WARNING_TIRED,
    };

    if state.phase == Phase::Gone {
        return Severity::Gone;
    }
    if state.phase == Phase::Leaving {
        return Severity::Leaving;
    }
    // Severity is derived from the underlying stats (not the display
    // animation) so that active-action animations — Feeding/Healing/etc
    // — don't "mask" an existing warning and suppress the alert when
    // the action ends.
    let severe = state.sick > SICK_TRIGGER_TIRED
        || state.tired > SICK_TRIGGER_TIRED
        || state.hunger > SICK_TRIGGER_HUNGER
        || state.drained > SICK_TRIGGER_DRAINED;
    if severe {
        return Severity::Severe;
    }
    let warning = state.sick > WARNING_SICK
        || state.tired > WARNING_TIRED
        || state.hunger > WARNING_HUNGER
        || state.drained > WARNING_DRAINED
        || state.miserable > WARNING_MISERABLE;
    if warning {
        return Severity::Warning;
    }
    Severity::Neutral
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

    // Pet just left: play the "funny ending" melody.  Triggers on any
    // transition into Gone so e.g. a natural Leaving → Gone fires the
    // melody; `new_generation()` goes via Hatching, not Gone, so it
    // does not trigger this path.
    if now == Severity::Gone as u8 && prev != Severity::Gone as u8 {
        if !muted {
            #[cfg(feature = "embassy-base")]
            crate::fw::buzzer::play(crate::fw::buzzer::FUNNY_ENDING_INDEX);
        }
        return;
    }

    // Alert only on upward transitions between tracked levels:
    //   Neutral(1) → Warning(2)
    //   Warning(2) → Severe(3)
    //   Severe(3)  → Leaving(4)
    let upward = now > prev && now <= Severity::Leaving as u8;
    if upward && !muted {
        #[cfg(feature = "embassy-base")]
        crate::fw::buzzer::play(crate::fw::buzzer::PET_WARN_INDEX);
    }
}

/// Tick at which the sleep animation should stop being shown.  When
/// the player taps Sleep, the engine may auto-wake within a single
/// tick (10 s) if `tired` was already low — too short for the 4-frame
/// sleep animation to cycle visibly.  We pin a display floor of 4
/// ticks (40 s = full animation cycle) here so the user always sees
/// the sleep loop after invoking the action.
static SLEEP_ANIM_UNTIL_TICK: AtomicU32 = AtomicU32::new(0);

/// Get the current display animation (cheap, no update).
pub fn display_anim() -> DisplayAnim {
    let state = unsafe { (*GAME.get()).as_ref() };
    let raw = match state {
        Some(s) => s.display_anim(),
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
            | DisplayAnim::WarningDrained
            | DisplayAnim::WarningMiserable
    ) && now_tick() < SLEEP_ANIM_UNTIL_TICK.load(Ordering::Relaxed)
    {
        return DisplayAnim::Sleeping;
    }
    raw
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
pub fn feed() -> bool {
    with_state(|s| s.feed())
}

pub fn heal() -> bool {
    with_state(|s| s.heal())
}

pub fn relax() -> bool {
    with_state(|s| s.relax())
}

pub fn play() -> bool {
    with_state(|s| s.play())
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

/// Award inspiration for winning a mini-game.
pub fn award_inspiration() {
    let state = unsafe { (*GAME.get()).as_mut() };
    if let Some(s) = state {
        s.award_inspiration();
    }
}

/// Start a new generation (after pet has left or manual reset).
/// Records the current pet in the Unicorn Realm before replacing it.
pub fn new_generation(kind: super::engine::PetKind) {
    let state = unsafe { (*GAME.get()).as_mut() };
    if let Some(s) = state {
        // Record the departing pet in the realm (unless it never hatched).
        if s.phase != super::engine::Phase::Hatching {
            let record = PetRecord::from_game_state(s, pet_name_bytes_sync());
            let realm = unsafe { &mut *REALM.get() };
            realm.push(record);
            // Mark realm_pending so it gets persisted on the next save cycle.
            s.realm_pending = true;
        }

        let seed = now_tick() as u64 ^ 0xDEAD_BEEF;
        s.new_generation(seed, kind);
        s.last_update_tick = now_tick();
    }
}

/// Get the current pet's kind (defaults to Snail if no game).
pub fn pet_kind() -> super::engine::PetKind {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.pet_kind,
        None => super::engine::PetKind::Snail,
    }
}

/// Get the current pet's rolled traits (vitality, curiosity, resilience).
/// Returns `None` if no game is active.
pub fn pet_traits() -> Option<(u16, u16, u16)> {
    let state = unsafe { (*GAME.get()).as_ref() };
    state.map(|s| (s.vitality, s.curiosity, s.resilience))
}

/// Get the current generation (defaults to 0 if no game).
pub fn pet_generation() -> u16 {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.generation,
        None => 0,
    }
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
#[cfg(feature = "embassy-base")]
pub async fn save_if_needed() -> bool {
    let state = unsafe { (*GAME.get()).as_mut() };
    let Some(state) = state else {
        return false;
    };

    // If the pet just left, record it in the Unicorn Realm.
    if state.realm_pending {
        state.realm_pending = false;
        let record = PetRecord::from_game_state(state, pet_name_bytes_sync());
        let realm = unsafe { &mut *REALM.get() };
        realm.push(record);
        let buf = realm.to_bytes();
        let ns = crate::fw::kv::namespace("game");
        if ns.set("realm", &buf, true).await.is_err() {
            defmt::warn!("game: realm save failed");
        } else {
            defmt::info!(
                "game: pet recorded in Unicorn Realm (gen={})",
                record.generation
            );
        }
    }

    if !state.needs_save() {
        return false;
    }

    let buf = state.to_bytes();
    let ns = crate::fw::kv::namespace("game");
    match ns.set("state", &buf, true).await {
        Ok(()) => {
            state.mark_saved();
            // Also persist pet name alongside state.
            let name = pet_name_bytes_sync();
            let _ = ns.set("name", name, true).await;
            true
        }
        Err(_) => {
            defmt::warn!("game: save failed");
            false
        }
    }
}

/// No-op when ekv is not available (simulator build).
#[cfg(not(feature = "embassy-base"))]
pub async fn save_if_needed() -> bool {
    false
}

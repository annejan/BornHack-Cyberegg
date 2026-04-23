//! Game lifecycle — initialisation, save/restore, and the per-cycle
//! update that embassy.rs calls from the display loop.
//!
//! The game state lives in a static cell, accessed through the functions
//! in this module.  All functions are async (flash access goes through
//! the shared QSPI mutex).

use super::engine::{GameState, PetStats, DisplayAnim, PetRealm, PetRecord, PET_NAME_MAX};
#[cfg(feature = "mesh")]
use super::engine::{SAVE_SIZE, REALM_SAVE_SIZE};

use core::sync::atomic::{AtomicU32, Ordering};

// ---------------------------------------------------------------------------
// Static game state
// ---------------------------------------------------------------------------

/// Wrapper for static mutable access (single-task, sequential).
struct SyncCell<T>(core::cell::UnsafeCell<T>);
unsafe impl<T> Sync for SyncCell<T> {}
impl<T> SyncCell<T> {
    const fn new(v: T) -> Self { Self(core::cell::UnsafeCell::new(v)) }
    fn get(&self) -> *mut T { self.0.get() }
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
/// 1 tick = 10 seconds.  Uses embassy uptime when available.
pub fn now_tick() -> u32 {
    #[cfg(feature = "embassy-base")]
    {
        (embassy_time::Instant::now().as_secs() / 10) as u32
    }
    #[cfg(not(feature = "embassy-base"))]
    {
        UPTIME_TICKS.load(Ordering::Relaxed)
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
#[cfg(feature = "embassy-base")]
pub async fn init() {
    let state = try_load().await;
    if state.is_some() {
        defmt::info!("game: resumed from save");
    } else {
        defmt::info!("game: no save — waiting for player to start");
    }
    unsafe { *GAME.get() = state; }

    // Load pet name and Unicorn Realm.
    #[cfg(feature = "mesh")]
    {
        use crate::fw::mesh::kv;
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
            unsafe { *REALM.get() = realm; }
        }
    }
}

#[cfg(feature = "embassy-base")]
async fn try_load() -> Option<GameState> {
    #[cfg(feature = "mesh")]
    {
        use crate::fw::mesh::kv;
        let ns = kv::namespace("game");
        let mut buf = [0u8; SAVE_SIZE];
        if let Ok(n) = ns.get("state", &mut buf).await {
            if n == SAVE_SIZE {
                if let Some(mut s) = GameState::from_bytes(&buf) {
                    s.last_update_tick = 0;
                    defmt::info!("game: restored from flash (gen={} age={})",
                        s.generation, s.age_ticks);
                    return Some(s);
                }
                defmt::warn!("game: corrupt save data");
            }
        }
    }
    None
}

#[cfg(feature = "embassy-base")]
fn new_egg(kind: super::engine::PetKind) -> GameState {
    let id = crate::fw::device_id::get_bytes();
    let seed = u64::from_le_bytes([
        id[0], id[1], id[2], id[3],
        id[0] ^ 0xAA, id[1] ^ 0x55, id[2] ^ 0xCC, id[3] ^ 0x33,
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

/// Create a new egg and begin the hatching countdown.
/// Called after the player selects a pet kind on the selection screen.
pub fn start_new_game(kind: super::engine::PetKind) {
    let mut egg = new_egg(kind);
    egg.last_update_tick = now_tick();
    unsafe { *GAME.get() = Some(egg); }
}

// ---------------------------------------------------------------------------
// Pet naming
// ---------------------------------------------------------------------------

/// Short Danish and Dutch names used as random defaults.
const DEFAULT_NAMES: &[&str] = &[
    "Arie", "Bert", "Bjorn", "Bob", "Bram",
    "Daan", "Femke", "Freja", "Ida", "Jens",
    "Kees", "Koen", "Lars", "Lotte", "Mette",
    "Niels", "Rupert", "Stijn", "Sven", "Anja",
];

/// Set the pet name from raw bytes (called by text entry callback).
pub fn set_pet_name(name: &[u8]) {
    let len = name.len().min(PET_NAME_MAX);
    let buf = unsafe { &mut *PET_NAME.get() };
    buf[..len].copy_from_slice(&name[..len]);
    buf[len..].fill(0);
    PET_NAME_LEN.store(len as u8, core::sync::atomic::Ordering::Relaxed);
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
    Some(state.stats(tick))
}

/// Get the current display animation (cheap, no update).
pub fn display_anim() -> DisplayAnim {
    let state = unsafe { (*GAME.get()).as_ref() };
    match state {
        Some(s) => s.display_anim(),
        None => DisplayAnim::Gone, // no game state loaded
    }
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
    with_state(|s| s.sleep())
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

fn with_state(f: impl FnOnce(&mut GameState) -> bool) -> bool {
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
/// Only available when the mesh feature provides ekv access.
#[cfg(feature = "mesh")]
pub async fn save_if_needed() -> bool {
    let state = unsafe { (*GAME.get()).as_mut() };
    let Some(state) = state else { return false; };

    // If the pet just left, record it in the Unicorn Realm.
    if state.realm_pending {
        state.realm_pending = false;
        let record = PetRecord::from_game_state(state, pet_name_bytes_sync());
        let realm = unsafe { &mut *REALM.get() };
        realm.push(record);
        let buf = realm.to_bytes();
        let ns = crate::fw::mesh::kv::namespace("game");
        if ns.set("realm", &buf, true).await.is_err() {
            defmt::warn!("game: realm save failed");
        } else {
            defmt::info!("game: pet recorded in Unicorn Realm (gen={})", record.generation);
        }
    }

    if !state.needs_save() { return false; }

    let buf = state.to_bytes();
    let ns = crate::fw::mesh::kv::namespace("game");
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

/// No-op when ekv is not available (game-only build).
#[cfg(not(feature = "mesh"))]
pub async fn save_if_needed() -> bool { false }

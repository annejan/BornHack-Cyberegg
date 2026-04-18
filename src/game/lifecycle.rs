//! Game lifecycle — initialisation, save/restore, and the per-cycle
//! update that embassy.rs calls from the display loop.
//!
//! The game state lives in a static cell, accessed through the functions
//! in this module.  All functions are async (flash access goes through
//! the shared QSPI mutex).

use super::engine::{GameState, PetStats, DisplayAnim};
#[cfg(feature = "embassy-base")]
use super::engine::SAVE_SIZE;

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
fn new_egg() -> GameState {
    let id = crate::fw::device_id::get_bytes();
    let seed = u64::from_le_bytes([
        id[0], id[1], id[2], id[3],
        id[0] ^ 0xAA, id[1] ^ 0x55, id[2] ^ 0xCC, id[3] ^ 0x33,
    ]);
    GameState::new_egg(seed)
}

#[cfg(not(feature = "embassy-base"))]
fn new_egg() -> GameState {
    GameState::new_egg(42)
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
/// Called when the player presses Fire on the "start game" screen.
pub fn start_new_game() {
    unsafe { *GAME.get() = Some(new_egg()); }
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

/// Start a new generation (after pet has left).
pub fn new_generation() {
    let state = unsafe { (*GAME.get()).as_mut() };
    if let Some(s) = state {
        let seed = now_tick() as u64 ^ 0xDEAD_BEEF;
        s.new_generation(seed);
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
// Save to flash
// ---------------------------------------------------------------------------

/// Save the game state to ekv if enough time has passed.
/// Returns true if a save was performed.
/// Only available when the mesh feature provides ekv access.
#[cfg(feature = "mesh")]
pub async fn save_if_needed() -> bool {
    let state = unsafe { (*GAME.get()).as_mut() };
    let Some(state) = state else { return false; };
    if !state.needs_save() { return false; }

    let buf = state.to_bytes();
    let ns = crate::fw::mesh::kv::namespace("game");
    match ns.set("state", &buf, true).await {
        Ok(()) => {
            state.mark_saved();
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

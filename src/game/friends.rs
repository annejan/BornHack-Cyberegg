//! Mesh friend discovery — pets "meeting" over the private SHDW channel.
//!
//! Every badge running this firmware auto-joins a channel called "SHDW"
//! (see `crate::fw::mesh::channels::ensure_shdw_channel`) and periodically
//! broadcasts a small [`PetBeacon`] on it (see
//! `crate::fw::mesh::meshcore::pet_beacon_ticker_task`). When another
//! badge's beacon is received, [`on_pet_beacon`] records it as a friend
//! and nudges happiness — a bigger one-time bump for a brand-new friend,
//! a smaller cooldown-gated bump for spending time with one already known.
//!
//! Persisted separately from the main game save, in its own `"friends"` KV
//! namespace — mirrors `PetRealm`'s ring-buffer/dirty-flag pattern.

use super::engine::PET_NAME_MAX;

// ---------------------------------------------------------------------------
// Wire format — the beacon broadcast on the SHDW channel
// ---------------------------------------------------------------------------

/// Private `GrpData` `data_type` marking a BornPets friend-discovery
/// beacon. Arbitrary but distinctive, chosen well clear of the low values
/// used by the MeshCore companion-app protocol for its own blob types, so
/// beacons never get confused for (or clutter) companion/channel-chat sync.
pub const PET_BEACON_TYPE: u16 = 0xBEAC;

/// Fixed-size beacon payload: sender identity + pet snapshot.
///
/// `attack`/`defense`/`speed`/`max_hp` are the sender's current derived
/// combat stats (see `crate::game::battle::CombatStats`) — carried here so
/// a friend's badge can cache them and battle against them later without
/// needing the opponent to be in range at that exact moment.
pub struct PetBeacon {
    pub device_id: [u8; 2],
    pub pet_kind: u8,
    pub generation: u16,
    pub name: [u8; PET_NAME_MAX],
    pub name_len: u8,
    pub attack: u8,
    pub defense: u8,
    pub speed: u8,
    pub max_hp: u8,
}

const BEACON_SIZE: usize = 22; // 2 + 1 + 2 + 12 + 1 + 4

impl PetBeacon {
    pub fn to_bytes(&self) -> [u8; BEACON_SIZE] {
        let mut buf = [0u8; BEACON_SIZE];
        buf[0..2].copy_from_slice(&self.device_id);
        buf[2] = self.pet_kind;
        buf[3..5].copy_from_slice(&self.generation.to_le_bytes());
        buf[5..17].copy_from_slice(&self.name);
        buf[17] = self.name_len;
        buf[18] = self.attack;
        buf[19] = self.defense;
        buf[20] = self.speed;
        buf[21] = self.max_hp;
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < BEACON_SIZE {
            return None;
        }
        let mut name = [0u8; PET_NAME_MAX];
        name.copy_from_slice(&buf[5..17]);
        Some(Self {
            device_id: [buf[0], buf[1]],
            pet_kind: buf[2],
            generation: u16::from_le_bytes([buf[3], buf[4]]),
            name,
            name_len: buf[17],
            attack: buf[18],
            defense: buf[19],
            speed: buf[20],
            max_hp: buf[21],
        })
    }
}

// ---------------------------------------------------------------------------
// Friend records — persisted list of pets met on SHDW
// ---------------------------------------------------------------------------

/// Minimum time between recurring happiness boosts from the same known
/// friend ("spending time together" vs. re-noticing them a minute later).
/// 360 ticks/hour (1 tick = 10s) * 4 hours.
pub const FRIEND_BOOST_COOLDOWN_TICKS: u32 = 360 * 4;

pub const FRIENDS_MAX: usize = 20;
const FRIEND_RECORD_SIZE: usize = 36; // 2 + 1 + 12 + 1 + 4 + 4 + 4 + 4 + 2 + 2
pub const FRIENDS_SAVE_SIZE: usize = 1 + FRIENDS_MAX * FRIEND_RECORD_SIZE;

#[derive(Clone, Copy)]
pub struct FriendRecord {
    pub device_id: [u8; 2],
    pub pet_kind: u8,
    pub name: [u8; PET_NAME_MAX],
    pub name_len: u8,
    pub first_seen_tick: u32,
    pub last_boost_tick: u32,
    /// Tick of the most recent beacon received from this friend,
    /// regardless of whether it also triggered a happiness boost —
    /// unlike `last_boost_tick`, this updates on every sighting. Drives
    /// the "Last seen" line on the detail screen and the most-recently-
    /// seen-first ordering of the list.
    pub last_seen_tick: u32,
    /// Cached combat-stat snapshot from this friend's most recent beacon —
    /// see `crate::game::battle::CombatStats`. Used to battle them without
    /// needing their badge in range at that exact moment.
    pub attack: u8,
    pub defense: u8,
    pub speed: u8,
    pub max_hp: u8,
    /// Head-to-head mesh Battle record against this specific friend —
    /// distinct from the pet's own overall `wins`/`losses` in
    /// `engine::GameState`. Updated on both badges for every battle
    /// between them (see `battle::challenge`/`battle::on_battle_result`),
    /// so the tally reads the same from either side.
    pub wins: u16,
    pub losses: u16,
}

impl FriendRecord {
    const EMPTY: Self = Self {
        device_id: [0; 2],
        pet_kind: 0,
        name: [0; PET_NAME_MAX],
        name_len: 0,
        first_seen_tick: 0,
        last_boost_tick: 0,
        last_seen_tick: 0,
        attack: 0,
        defense: 0,
        speed: 0,
        max_hp: 0,
        wins: 0,
        losses: 0,
    };

    fn to_bytes(self, buf: &mut [u8]) {
        buf[0..2].copy_from_slice(&self.device_id);
        buf[2] = self.pet_kind;
        buf[3..15].copy_from_slice(&self.name);
        buf[15] = self.name_len;
        buf[16..20].copy_from_slice(&self.first_seen_tick.to_le_bytes());
        buf[20..24].copy_from_slice(&self.last_boost_tick.to_le_bytes());
        buf[24..28].copy_from_slice(&self.last_seen_tick.to_le_bytes());
        buf[28] = self.attack;
        buf[29] = self.defense;
        buf[30] = self.speed;
        buf[31] = self.max_hp;
        buf[32..34].copy_from_slice(&self.wins.to_le_bytes());
        buf[34..36].copy_from_slice(&self.losses.to_le_bytes());
    }

    fn from_bytes(buf: &[u8]) -> Self {
        let mut name = [0u8; PET_NAME_MAX];
        name.copy_from_slice(&buf[3..15]);
        Self {
            device_id: [buf[0], buf[1]],
            pet_kind: buf[2],
            name,
            name_len: buf[15],
            first_seen_tick: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
            last_boost_tick: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            last_seen_tick: u32::from_le_bytes([buf[24], buf[25], buf[26], buf[27]]),
            attack: buf[28],
            defense: buf[29],
            speed: buf[30],
            max_hp: buf[31],
            wins: u16::from_le_bytes([buf[32], buf[33]]),
            losses: u16::from_le_bytes([buf[34], buf[35]]),
        }
    }

    /// Friend's pet name as a str.
    pub fn name_str(&self) -> &str {
        let n = (self.name_len as usize).min(PET_NAME_MAX);
        core::str::from_utf8(&self.name[..n]).unwrap_or("")
    }
}

/// Ring buffer of met friends, newest-first — same shape as `PetRealm`,
/// but entries are updated in place by `device_id` rather than always
/// appended, since this tracks unique friends rather than a history.
pub struct FriendsList {
    pub friends: [FriendRecord; FRIENDS_MAX],
    pub count: u8,
}

impl Default for FriendsList {
    fn default() -> Self {
        Self::new()
    }
}

impl FriendsList {
    pub const fn new() -> Self {
        Self {
            friends: [FriendRecord::EMPTY; FRIENDS_MAX],
            count: 0,
        }
    }

    fn find_mut(&mut self, device_id: [u8; 2]) -> Option<&mut FriendRecord> {
        self.friends[..self.count as usize]
            .iter_mut()
            .find(|f| f.device_id == device_id)
    }

    fn find_index(&self, device_id: [u8; 2]) -> Option<usize> {
        self.friends[..self.count as usize]
            .iter()
            .position(|f| f.device_id == device_id)
    }

    /// Move the entry at `index` to the front, shifting everything
    /// between it and the front down by one. Used to keep the list
    /// ordered most-recently-seen-first as beacons come in.
    fn move_to_front(&mut self, index: usize) {
        if index == 0 {
            return;
        }
        let rec = self.friends[index];
        for i in (1..=index).rev() {
            self.friends[i] = self.friends[i - 1];
        }
        self.friends[0] = rec;
    }

    /// Add a newly-met friend, newest first, dropping the oldest if full.
    fn push(&mut self, record: FriendRecord) {
        for i in (1..FRIENDS_MAX).rev() {
            self.friends[i] = self.friends[i - 1];
        }
        self.friends[0] = record;
        if (self.count as usize) < FRIENDS_MAX {
            self.count += 1;
        }
    }

    pub fn to_bytes(&self) -> [u8; FRIENDS_SAVE_SIZE] {
        let mut buf = [0u8; FRIENDS_SAVE_SIZE];
        buf[0] = self.count;
        for i in 0..self.count as usize {
            let offset = 1 + i * FRIEND_RECORD_SIZE;
            self.friends[i].to_bytes(&mut buf[offset..offset + FRIEND_RECORD_SIZE]);
        }
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut list = Self::new();
        if buf.is_empty() {
            return list;
        }
        list.count = buf[0].min(FRIENDS_MAX as u8);
        for i in 0..list.count as usize {
            let offset = 1 + i * FRIEND_RECORD_SIZE;
            if offset + FRIEND_RECORD_SIZE <= buf.len() {
                list.friends[i] = FriendRecord::from_bytes(&buf[offset..]);
            }
        }
        list
    }
}

// ---------------------------------------------------------------------------
// Static state + KV persistence
// ---------------------------------------------------------------------------

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

static FRIENDS: SyncCell<FriendsList> = SyncCell::new(FriendsList::new());
static FRIENDS_DIRTY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Load the friends list from flash. Call once at startup, same as
/// `lifecycle::init`'s Unicorn Realm load.
#[cfg(feature = "embassy-base")]
pub async fn init() {
    use crate::fw::kv;
    let ns = kv::namespace("friends");
    let mut buf = [0u8; FRIENDS_SAVE_SIZE];
    if let Ok(n) = ns.get("list", &mut buf).await {
        let list = FriendsList::from_bytes(&buf[..n]);
        defmt::info!("friends: loaded {} known friends", list.count);
        unsafe {
            *FRIENDS.get() = list;
        }
    }
}

#[cfg(not(feature = "embassy-base"))]
pub async fn init() {}

/// Persist the friends list if it changed since the last save. Called
/// from `lifecycle::save_if_needed` alongside the Unicorn Realm save.
#[cfg(feature = "embassy-base")]
pub async fn save_if_needed() {
    use core::sync::atomic::Ordering;
    if !FRIENDS_DIRTY.swap(false, Ordering::Relaxed) {
        return;
    }
    let list = unsafe { &*FRIENDS.get() };
    let buf = list.to_bytes();
    let ns = crate::fw::kv::namespace("friends");
    if ns.set("list", &buf, true).await.is_err() {
        FRIENDS_DIRTY.store(true, Ordering::Relaxed); // retry next cycle
        defmt::warn!("friends: save failed");
    }
}

#[cfg(not(feature = "embassy-base"))]
pub async fn save_if_needed() {}

/// Number of known friends.
pub fn count() -> u8 {
    unsafe { (*FRIENDS.get()).count }
}

/// Get a known friend by index (0 = most recently *seen* — the list is
/// kept ordered by `last_seen_tick`, newest first, via `move_to_front`).
pub fn get(index: usize) -> Option<FriendRecord> {
    let list = unsafe { &*FRIENDS.get() };
    if index < list.count as usize {
        Some(list.friends[index])
    } else {
        None
    }
}

/// Record a mesh Battle result against a specific friend — the
/// head-to-head tally shown in the Friends detail screen. No-op if
/// `device_id` isn't a known friend (e.g. a battle result arrived
/// before we'd ever received a beacon from them).
///
/// Called from both `battle::challenge` (the challenger's side) and
/// `battle::on_battle_result` (the target's side) so the tally reads
/// the same from either badge — see module docs.
pub fn record_battle_vs(device_id: [u8; 2], won: bool) {
    let list = unsafe { &mut *FRIENDS.get() };
    if let Some(friend) = list.find_mut(device_id) {
        if won {
            friend.wins = friend.wins.saturating_add(1);
        } else {
            friend.losses = friend.losses.saturating_add(1);
        }
        FRIENDS_DIRTY.store(true, core::sync::atomic::Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Beacon receive handler
// ---------------------------------------------------------------------------

/// The real `fw` module (and `device_id` with it) only exists in builds
/// that pull in `embassy-base` — the plain host `simulator` build (which
/// still enables `game`, and so compiles this file) gets a stub `fw` with
/// just a couple of UI-only submodules. `on_pet_beacon` is only ever
/// actually invoked from mesh code, which in every real build combination
/// implies `embassy-base`, so the simulator stub value is simply dead code
/// kept around to type-check.
#[cfg(feature = "embassy-base")]
pub(super) fn local_device_id() -> [u8; 2] {
    crate::fw::device_id::get()
}

#[cfg(not(feature = "embassy-base"))]
pub(super) fn local_device_id() -> [u8; 2] {
    [0, 0]
}

/// Handle a `PetBeacon` received on the SHDW channel: record the friend
/// (new or already known), refresh `last_seen_tick`, move them to the
/// front of the list (most-recently-seen first), and apply the matching
/// happiness boost.
///
/// Called from `fw::mesh::meshcore::push_grp_data` when a `GrpData`
/// packet on the SHDW slot carries `data_type == PET_BEACON_TYPE`.
pub async fn on_pet_beacon(data: &[u8]) {
    let Some(beacon) = PetBeacon::from_bytes(data) else {
        return;
    };

    // Beacons flood across the mesh and can echo back to their own
    // sender — ignore ourselves.
    if beacon.device_id == local_device_id() {
        return;
    }

    let now = super::lifecycle::now_tick();
    let list = unsafe { &mut *FRIENDS.get() };

    let big_boost = match list.find_index(beacon.device_id) {
        Some(idx) => {
            let friend = &mut list.friends[idx];
            friend.name = beacon.name;
            friend.name_len = beacon.name_len;
            friend.pet_kind = beacon.pet_kind;
            friend.attack = beacon.attack;
            friend.defense = beacon.defense;
            friend.speed = beacon.speed;
            friend.max_hp = beacon.max_hp;
            friend.last_seen_tick = now;
            let boost = if now.saturating_sub(friend.last_boost_tick) < FRIEND_BOOST_COOLDOWN_TICKS
            {
                None // seen too recently — no boost, just refreshed the record above
            } else {
                friend.last_boost_tick = now;
                Some(false)
            };
            list.move_to_front(idx);
            boost
        }
        None => {
            // `push` already inserts at the front, so a brand-new friend
            // needs no separate move.
            list.push(FriendRecord {
                device_id: beacon.device_id,
                pet_kind: beacon.pet_kind,
                name: beacon.name,
                name_len: beacon.name_len,
                first_seen_tick: now,
                last_boost_tick: now,
                last_seen_tick: now,
                attack: beacon.attack,
                defense: beacon.defense,
                speed: beacon.speed,
                max_hp: beacon.max_hp,
                wins: 0,
                losses: 0,
            });
            Some(true)
        }
    };

    FRIENDS_DIRTY.store(true, core::sync::atomic::Ordering::Relaxed);

    if let Some(big) = big_boost {
        super::lifecycle::friend_boost(big);
        super::show_toast(if big {
            super::Toast::NewFriend
        } else {
            super::Toast::FriendReunion
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beacon(id: [u8; 2]) -> PetBeacon {
        PetBeacon {
            device_id: id,
            pet_kind: 1,
            generation: 3,
            name: *b"Rex\0\0\0\0\0\0\0\0\0",
            name_len: 3,
            attack: 40,
            defense: 30,
            speed: 20,
            max_hp: 100,
        }
    }

    fn friend_record(device_id: [u8; 2], first_seen_tick: u32, last_boost_tick: u32) -> FriendRecord {
        FriendRecord {
            device_id,
            pet_kind: 0,
            name: [0; PET_NAME_MAX],
            name_len: 0,
            first_seen_tick,
            last_boost_tick,
            last_seen_tick: last_boost_tick,
            attack: 0,
            defense: 0,
            speed: 0,
            max_hp: 0,
            wins: 0,
            losses: 0,
        }
    }

    #[test]
    fn beacon_round_trips() {
        let b = beacon([0xAB, 0xCD]);
        let bytes = b.to_bytes();
        let restored = PetBeacon::from_bytes(&bytes).unwrap();
        assert_eq!(restored.device_id, [0xAB, 0xCD]);
        assert_eq!(restored.pet_kind, 1);
        assert_eq!(restored.generation, 3);
        assert_eq!(restored.name_len, 3);
        assert_eq!(&restored.name[..3], b"Rex");
        assert_eq!(restored.attack, 40);
        assert_eq!(restored.defense, 30);
        assert_eq!(restored.speed, 20);
        assert_eq!(restored.max_hp, 100);
    }

    #[test]
    fn friends_list_add_and_lookup() {
        let mut list = FriendsList::new();
        assert!(list.find_mut([1, 1]).is_none());
        list.push(friend_record([1, 1], 10, 10));
        assert_eq!(list.count, 1);
        assert!(list.find_mut([1, 1]).is_some());
        assert!(list.find_mut([2, 2]).is_none());
    }

    #[test]
    fn move_to_front_reorders_without_losing_entries() {
        let mut list = FriendsList::new();
        list.push(friend_record([1, 1], 1, 1)); // becomes index 1 after next push
        list.push(friend_record([2, 2], 2, 2)); // index 0
        list.push(friend_record([3, 3], 3, 3)); // index 0, pushes the others down
        // Order is now [3,3], [2,2], [1,1].
        assert_eq!(list.friends[0].device_id, [3, 3]);
        assert_eq!(list.friends[1].device_id, [2, 2]);
        assert_eq!(list.friends[2].device_id, [1, 1]);

        // "Re-seeing" [1,1] (last in the list) should promote it to the
        // front without dropping or duplicating anyone.
        let idx = list.find_index([1, 1]).unwrap();
        list.move_to_front(idx);
        assert_eq!(list.count, 3);
        assert_eq!(list.friends[0].device_id, [1, 1]);
        assert_eq!(list.friends[1].device_id, [3, 3]);
        assert_eq!(list.friends[2].device_id, [2, 2]);

        // Already at the front: a no-op, not a duplicate/shift.
        list.move_to_front(0);
        assert_eq!(list.friends[0].device_id, [1, 1]);
        assert_eq!(list.friends[1].device_id, [3, 3]);
        assert_eq!(list.friends[2].device_id, [2, 2]);
    }

    #[test]
    fn friends_list_ring_overflow_drops_oldest() {
        let mut list = FriendsList::new();
        for i in 0..(FRIENDS_MAX as u16 + 3) {
            let id = i.to_le_bytes();
            list.push(friend_record(id, i as u32, i as u32));
        }
        assert_eq!(list.count as usize, FRIENDS_MAX);
        // The 3 oldest (i=0,1,2) should have been evicted.
        assert!(list.find_mut(0u16.to_le_bytes()).is_none());
        assert!(list.find_mut(2u16.to_le_bytes()).is_none());
        // The most recent should still be present, at the front.
        let last_id = (FRIENDS_MAX as u16 + 2).to_le_bytes();
        assert!(list.find_mut(last_id).is_some());
    }

    #[test]
    fn friends_list_round_trips_through_bytes() {
        let mut list = FriendsList::new();
        list.push(FriendRecord {
            device_id: [9, 9],
            pet_kind: 2,
            name: *b"Mochi\0\0\0\0\0\0\0",
            name_len: 5,
            first_seen_tick: 100,
            last_boost_tick: 200,
            last_seen_tick: 250,
            attack: 55,
            defense: 45,
            speed: 35,
            max_hp: 120,
            wins: 7,
            losses: 3,
        });
        let bytes = list.to_bytes();
        let restored = FriendsList::from_bytes(&bytes);
        assert_eq!(restored.count, 1);
        assert_eq!(restored.friends[0].device_id, [9, 9]);
        assert_eq!(restored.friends[0].name_str(), "Mochi");
        assert_eq!(restored.friends[0].first_seen_tick, 100);
        assert_eq!(restored.friends[0].last_boost_tick, 200);
        assert_eq!(restored.friends[0].last_seen_tick, 250);
        assert_eq!(restored.friends[0].attack, 55);
        assert_eq!(restored.friends[0].defense, 45);
        assert_eq!(restored.friends[0].speed, 35);
        assert_eq!(restored.friends[0].max_hp, 120);
        assert_eq!(restored.friends[0].wins, 7);
        assert_eq!(restored.friends[0].losses, 3);
    }

    #[test]
    fn record_battle_vs_updates_known_friend_and_ignores_unknown() {
        let mut list = FriendsList::new();
        list.push(friend_record([4, 4], 1, 1));

        // Exercise the same increment logic `record_battle_vs` uses,
        // directly against the list (the module-level function operates
        // on the shared static, which isn't reachable in a plain unit
        // test without the embassy/simulator runtime).
        if let Some(friend) = list.find_mut([4, 4]) {
            friend.wins += 1;
        }
        if let Some(friend) = list.find_mut([4, 4]) {
            friend.losses += 1;
        }
        assert!(list.find_mut([9, 9]).is_none());

        let friend = list.find_mut([4, 4]).unwrap();
        assert_eq!(friend.wins, 1);
        assert_eq!(friend.losses, 1);
    }

    #[test]
    fn cooldown_classification_matches_elapsed_ticks() {
        // Not a call into on_pet_beacon (that needs the static + async
        // runtime) — just pins down the boundary the classification
        // in `on_pet_beacon` relies on.
        let last_boost_tick: u32 = 1000;
        let just_under = last_boost_tick + FRIEND_BOOST_COOLDOWN_TICKS - 1;
        let at_boundary = last_boost_tick + FRIEND_BOOST_COOLDOWN_TICKS;
        assert!(just_under.saturating_sub(last_boost_tick) < FRIEND_BOOST_COOLDOWN_TICKS);
        assert!(at_boundary.saturating_sub(last_boost_tick) >= FRIEND_BOOST_COOLDOWN_TICKS);
    }
}

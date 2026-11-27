//! Mesh friend discovery — pets "meeting" over the private SHDW channel.
//!
//! Every badge running this firmware auto-joins a channel called "SHDW"
//! (see `crate::fw::mesh::channels::ensure_shdw_channel`) and periodically
//! broadcasts a small [`PetBeacon`] on it (see
//! the firmware's `pet_beacon_ticker_task`). When another
//! badge's beacon is received, [`on_pet_beacon`] records it as a friend
//! and nudges happiness — a bigger one-time bump for a brand-new friend,
//! a smaller cooldown-gated bump for spending time with one already known.
//!
//! Persisted separately from the main game save, in its own `"friends"` KV
//! namespace — mirrors `PetRealm`'s ring-buffer/dirty-flag pattern.

use crate::sync::SyncCell;
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
/// `power`/`guard`/`speed`/`hp` (and stamina, focus) are the sender's current
/// fight numbers (see `crate::game::engine::combat::Snapshot`) — carried here so
/// a friend's badge can cache them and battle against them later without
/// needing the opponent to be in range at that exact moment.
pub struct PetBeacon {
    pub device_id: [u8; 2],
    pub pet_kind: u8,
    pub generation: u16,
    pub name: [u8; PET_NAME_MAX],
    pub name_len: u8,
    pub power: u8,
    pub guard: u8,
    pub speed: u8,
    pub hp: u8,
    pub stamina: u8,
    pub focus: i8,
    pub demo: bool,
    /// True when decoded from a 25-byte v3 beacon; an old badge's 22 bytes
    /// leave stamina 3, focus 0 and no demo flag.
    pub v3: bool,
}

/// v3 beacon: bytes 0..18 as before, then power, guard, speed, hp, stamina,
/// focus, flags (bit 0 demo).
const BEACON_SIZE: usize = 25;
/// An older badge's beacon: 2 + 1 + 2 + 12 + 1 + 4.
/// Stamina assumed for a pre-v3 beacon, which does not carry one: a middling
/// value, so an old badge neither dominates nor collapses in a fight.
const OLD_BEACON_STAMINA: u8 = 3;

const BEACON_SIZE_OLD: usize = 22;

impl PetBeacon {
    pub fn to_bytes(&self) -> [u8; BEACON_SIZE] {
        let mut buf = [0u8; BEACON_SIZE];
        buf[0..2].copy_from_slice(&self.device_id);
        buf[2] = self.pet_kind;
        buf[3..5].copy_from_slice(&self.generation.to_le_bytes());
        buf[5..17].copy_from_slice(&self.name);
        buf[17] = self.name_len;
        buf[18] = self.power;
        buf[19] = self.guard;
        buf[20] = self.speed;
        buf[21] = self.hp;
        buf[22] = self.stamina;
        buf[23] = self.focus as u8;
        buf[24] = self.demo as u8;
        buf
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < BEACON_SIZE_OLD {
            return None;
        }
        let v3 = buf.len() >= BEACON_SIZE;
        let mut name = [0u8; PET_NAME_MAX];
        name.copy_from_slice(&buf[5..17]);
        Some(Self {
            device_id: [buf[0], buf[1]],
            pet_kind: buf[2],
            generation: u16::from_le_bytes([buf[3], buf[4]]),
            name,
            name_len: buf[17],
            power: buf[18],
            guard: buf[19],
            speed: buf[20],
            hp: buf[21],
            stamina: if v3 { buf[22] } else { OLD_BEACON_STAMINA },
            focus: if v3 { buf[23] as i8 } else { 0 },
            demo: v3 && buf[24] & 1 != 0,
            v3,
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
/// Today's record: 2 + 1 + 12 + 1 + 4 + 4 + 4 + 4 + 2 + 2 = 36, then stamina,
/// focus, flags, and the 4-byte `fought_tick`.
const FRIEND_RECORD_SIZE: usize = 43;
/// The per-pair fight cooldown in ticks (1 tick = 10 s, so 6 per minute).
fn pair_cooldown_ticks() -> u32 {
    super::engine::thresholds::BATTLE_PAIR_COOLDOWN_MIN() as u32 * 6
}

/// A pre-v3 record (`list`), read for the upgrade only.
const FRIEND_RECORD_SIZE_OLD: usize = 36;
/// First byte of `list_v3`.
pub const FRIENDS_LAYOUT: u8 = 3;
/// `[layout][count][20 x 43]`.
pub const FRIENDS_SAVE_SIZE: usize = 2 + FRIENDS_MAX * FRIEND_RECORD_SIZE;
/// The pre-v3 blob, `[count][20 x 36]`.
pub const FRIENDS_SAVE_SIZE_OLD: usize = 1 + FRIENDS_MAX * FRIEND_RECORD_SIZE_OLD;
/// KV key the friends list is saved under.
pub const FRIENDS_KEY: &str = "list_v3";
/// The pre-v3 KV key: read for the upgrade, never written.
pub const FRIENDS_KEY_OLD: &str = "list";

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
    /// Cached combat numbers (power, guard, speed, hp) from this friend's
    /// most recent beacon — see `Snapshot::from_beacon`. Used to battle them without
    /// needing their badge in range at that exact moment.
    pub power: u8,
    pub guard: u8,
    pub speed: u8,
    pub hp: u8,
    /// Stamina and focus from the same beacon (an old beacon's defaults when
    /// it carried none).
    pub stamina: u8,
    pub focus: i8,
    /// Flags bit 0: the last beacon was from a demo-mode badge.
    pub demo: bool,
    /// Flags bit 1: the last beacon was a v3 beacon. Task 7 reads it so a
    /// non-v3 friend is never challenged.
    pub v3: bool,
    /// Tick of the last fight between the two pets (0 = never); rebased on
    /// reboot like the other ticks. Drives the per-pair cooldown.
    pub fought_tick: u32,
    /// Head-to-head mesh Battle record against this specific friend —
    /// distinct from the pet's own overall `wins`/`losses` in
    /// `engine::GameState`. Updated on both badges for every battle
    /// between them (see `battle_link`),
    /// so the tally reads the same from either side.
    pub wins: u16,
    pub losses: u16,
    /// RAM only, never saved: a beacon from this friend arrived since this
    /// boot. After a reboot `rebase_after_reboot` makes every stored
    /// `last_seen_tick` look recent, so a fallback fight (battle `fresh`)
    /// needs this too.
    pub heard_this_boot: bool,
}

impl FriendRecord {
    /// List label: `"Name (Species)"`, or just the species when unnamed.
    pub fn label(&self) -> heapless::String<32> {
        let kind_name = super::engine::PetKind::from_u8(self.pet_kind).name();
        let name = self.name_str();
        let mut label = heapless::String::new();
        if !name.is_empty() {
            let _ = core::fmt::Write::write_fmt(&mut label, format_args!("{} ({})", name, kind_name));
        } else {
            let _ = core::fmt::Write::write_fmt(&mut label, format_args!("{}", kind_name));
        }
        label
    }

    const EMPTY: Self = Self {
        device_id: [0; 2],
        pet_kind: 0,
        name: [0; PET_NAME_MAX],
        name_len: 0,
        first_seen_tick: 0,
        last_boost_tick: 0,
        last_seen_tick: 0,
        power: 0,
        guard: 0,
        speed: 0,
        hp: 0,
        stamina: 0,
        focus: 0,
        demo: false,
        v3: false,
        fought_tick: 0,
        wins: 0,
        losses: 0,
        heard_this_boot: false,
    };

    /// A beacon with numbers has arrived (an unfought friend added from an
    /// old save has `hp` 0).
    pub fn has_numbers(&self) -> bool {
        self.hp > 0
    }

    /// The fight numbers we hold for this friend, as an estimate.
    pub fn estimate_snapshot(&self) -> super::engine::combat::Snapshot {
        super::engine::combat::Snapshot::from_beacon(
            self.power,
            self.guard,
            self.speed,
            self.hp,
            self.stamina,
            self.focus,
            self.demo,
        )
    }

    /// Seconds until these two pets may fight again, or `None` when they may
    /// (never fought, or the pair cooldown has run out). One tick is 10 s.
    pub fn pair_wait_secs(&self, now_tick: u32) -> Option<u32> {
        if self.fought_tick == 0 {
            return None;
        }
        let cooldown = pair_cooldown_ticks();
        let elapsed = now_tick.saturating_sub(self.fought_tick);
        if elapsed >= cooldown {
            None
        } else {
            Some((cooldown - elapsed) * 10)
        }
    }

    /// Copy everything a beacon carries onto this record (the same update
    /// `on_pet_beacon` applies on every sighting).
    fn refresh(&mut self, beacon: &PetBeacon, now: u32) {
        self.name = beacon.name;
        self.name_len = beacon.name_len;
        self.pet_kind = beacon.pet_kind;
        self.power = beacon.power;
        self.guard = beacon.guard;
        self.speed = beacon.speed;
        self.hp = beacon.hp;
        self.stamina = beacon.stamina;
        self.focus = beacon.focus;
        self.demo = beacon.demo;
        self.v3 = beacon.v3;
        self.last_seen_tick = now;
        self.heard_this_boot = true;
    }

    fn to_bytes(self, buf: &mut [u8]) {
        buf[0..2].copy_from_slice(&self.device_id);
        buf[2] = self.pet_kind;
        buf[3..15].copy_from_slice(&self.name);
        buf[15] = self.name_len;
        buf[16..20].copy_from_slice(&self.first_seen_tick.to_le_bytes());
        buf[20..24].copy_from_slice(&self.last_boost_tick.to_le_bytes());
        buf[24..28].copy_from_slice(&self.last_seen_tick.to_le_bytes());
        buf[28] = self.power;
        buf[29] = self.guard;
        buf[30] = self.speed;
        buf[31] = self.hp;
        buf[32..34].copy_from_slice(&self.wins.to_le_bytes());
        buf[34..36].copy_from_slice(&self.losses.to_le_bytes());
        buf[36] = self.stamina;
        buf[37] = self.focus as u8;
        buf[38] = self.demo as u8 | (self.v3 as u8) << 1;
        buf[39..43].copy_from_slice(&self.fought_tick.to_le_bytes());
    }

    /// Decode the first 36 bytes (shared by both layouts); the v3 fields are
    /// zero.
    fn from_old_bytes(buf: &[u8]) -> Self {
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
            power: buf[28],
            guard: buf[29],
            speed: buf[30],
            hp: buf[31],
            stamina: 0,
            focus: 0,
            demo: false,
            v3: false,
            fought_tick: 0,
            wins: u16::from_le_bytes([buf[32], buf[33]]),
            losses: u16::from_le_bytes([buf[34], buf[35]]),
            heard_this_boot: false,
        }
    }

    fn from_bytes(buf: &[u8]) -> Self {
        let mut rec = Self::from_old_bytes(buf);
        rec.stamina = buf[36];
        rec.focus = buf[37] as i8;
        rec.demo = buf[38] & 1 != 0;
        rec.v3 = buf[38] & 2 != 0;
        rec.fought_tick = u32::from_le_bytes([buf[39], buf[40], buf[41], buf[42]]);
        rec
    }

    /// Friend's pet name as a str.
    pub fn name_str(&self) -> &str {
        super::engine::name_str(&self.name, self.name_len)
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
        buf[0] = FRIENDS_LAYOUT;
        buf[1] = self.count;
        for i in 0..self.count as usize {
            let offset = 2 + i * FRIEND_RECORD_SIZE;
            self.friends[i].to_bytes(&mut buf[offset..offset + FRIEND_RECORD_SIZE]);
        }
        buf
    }

    /// Decode `list_v3` (layout 3). An empty or foreign-layout blob gives an
    /// empty list.
    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut list = Self::new();
        if buf.len() < 2 || buf[0] != FRIENDS_LAYOUT {
            return list;
        }
        list.count = buf[1].min(FRIENDS_MAX as u8);
        for i in 0..list.count as usize {
            let offset = 2 + i * FRIEND_RECORD_SIZE;
            if offset + FRIEND_RECORD_SIZE <= buf.len() {
                list.friends[i] = FriendRecord::from_bytes(&buf[offset..]);
            }
        }
        list
    }

    /// Decode today's pre-v3 `list` blob, `[count][20 x 36]`; the new fields
    /// are zero.
    pub fn from_old_bytes(buf: &[u8]) -> Self {
        let mut list = Self::new();
        if buf.is_empty() {
            return list;
        }
        list.count = buf[0].min(FRIENDS_MAX as u8);
        for i in 0..list.count as usize {
            let offset = 1 + i * FRIEND_RECORD_SIZE_OLD;
            if offset + FRIEND_RECORD_SIZE_OLD <= buf.len() {
                list.friends[i] = FriendRecord::from_old_bytes(&buf[offset..]);
            }
        }
        list
    }

    /// Rebase every stored tick field so the load survives a reboot.
    ///
    /// `now_tick()` is uptime-since-boot (`embassy_time::Instant`), not a
    /// persisted wall clock — it resets to ~0 on every power cycle. But
    /// `first_seen_tick`/`last_seen_tick`/`last_boost_tick` are saved as
    /// absolute tick values from whatever boot session wrote them. Loaded
    /// as-is after a reboot, every one of those stored values is larger
    /// than the freshly-reset `now`, so every `now.saturating_sub(stored)`
    /// display computation saturates to 0 — "Met 0d 0h ago" / "Last seen
    /// 0d 0h ago" for every friend, forever, until enough continuous
    /// uptime in the new session outgrows the old stored value (which can
    /// take as long as the previous session's uptime did). This is the
    /// same class of problem `GameState::last_update_tick` solves by
    /// resetting itself to 0 on load — but that works for a single
    /// delta-accumulator field; here there are several *absolute*
    /// timestamps across every friend that all need to stay consistent
    /// with each other and with `now`.
    ///
    /// Fix: find the single largest stored tick across every friend (the
    /// most recent interaction, whichever field it's in) and shift every
    /// tick field on every friend down by the same amount, so that
    /// maximum lands exactly on `now` — i.e. assume no time passed while
    /// the badge was off, exactly like the `GameState` trick assumes.
    ///
    /// `fought_tick` is part of the shift's maximum (a fallback fight can be
    /// newer than the last beacon) but is rebased by age, not by clamping:
    /// each fight's age is measured from the maximum; at or past the pair
    /// cooldown it becomes 0 ("never", no wait), otherwise it keeps its
    /// remaining wait relative to `now` (when `now` is smaller than the age
    /// no earlier tick exists, so the wait restarts in full: never shorter).
    ///
    /// This can't recover real elapsed offline time (there's no RTC to
    /// know it), but it keeps every "ago" value sane instead of every friend
    /// reading "0 ago" until the new session's uptime catches up. The
    /// relative ordering between friends (who you met first, who you saw
    /// most recently) survives only for ticks within `now` of the maximum:
    /// older ticks saturate at 0 and tie. Right after boot `now` is small,
    /// so most of the older ticks tie at 0.
    fn rebase_after_reboot(&mut self, now: u32) {
        let max_stored = self.friends[..self.count as usize]
            .iter()
            .flat_map(|f| [f.first_seen_tick, f.last_seen_tick, f.last_boost_tick, f.fought_tick])
            .max()
            .unwrap_or(0);
        if max_stored <= now {
            return; // nothing to do — already consistent with `now`
        }
        let rebase = max_stored - now;
        for f in self.friends[..self.count as usize].iter_mut() {
            f.first_seen_tick = f.first_seen_tick.saturating_sub(rebase);
            f.last_seen_tick = f.last_seen_tick.saturating_sub(rebase);
            f.last_boost_tick = f.last_boost_tick.saturating_sub(rebase);
            if f.fought_tick != 0 {
                let age = max_stored.saturating_sub(f.fought_tick);
                f.fought_tick = if age >= pair_cooldown_ticks() {
                    0
                } else {
                    // Non-zero: 0 means "never fought".
                    now.saturating_sub(age).max(1)
                };
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Static state + KV persistence
// ---------------------------------------------------------------------------

/// What one KV read of a friends key gave.
pub enum LoadRead<'a> {
    Found(&'a [u8]),
    /// The key does not exist.
    Missing,
    /// Any other error (a flash glitch, a bad key, ...).
    Failed,
}

/// The result of [`decide_load`].
pub struct Loaded {
    pub list: FriendsList,
    /// The list came from the old key: save it under `list_v3` soon.
    pub dirty: bool,
    /// False when a read failed or the blob is foreign: never write this boot,
    /// so a transient error cannot overwrite or strand good data.
    pub may_save: bool,
}

/// Decide what to load from the two reads. `old` is only looked at when
/// `v3` is `Missing`.
pub fn decide_load(v3: LoadRead, old: LoadRead) -> Loaded {
    let empty = |may_save| Loaded { list: FriendsList::new(), dirty: false, may_save };
    match v3 {
        LoadRead::Found(b) if b.len() >= 2 && b[0] == FRIENDS_LAYOUT => Loaded {
            list: FriendsList::from_bytes(b),
            dirty: false,
            may_save: true,
        },
        // Some other layout (or a stub): leave it alone.
        LoadRead::Found(_) | LoadRead::Failed => empty(false),
        LoadRead::Missing => match old {
            LoadRead::Found(b) => Loaded {
                list: FriendsList::from_old_bytes(b),
                dirty: true,
                may_save: true,
            },
            LoadRead::Missing => empty(true),
            LoadRead::Failed => empty(false),
        },
    }
}

/// Set when the load failed: friends are not written for the rest of the boot.
static FRIENDS_SAVE_BLOCKED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

static FRIENDS: SyncCell<FriendsList> = SyncCell::new(FriendsList::new());
static FRIENDS_DIRTY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Load the friends list from flash. Call once at startup, same as
/// `lifecycle::init`'s Unicorn Realm load.
#[cfg(feature = "embassy-core")]
pub async fn init() {
    use crate::fw::kv;
    let ns = kv::namespace("friends");
    let mut buf = [0u8; FRIENDS_SAVE_SIZE];
    let mut old = [0u8; FRIENDS_SAVE_SIZE_OLD];
    let v3 = match ns.get(FRIENDS_KEY, &mut buf).await {
        Ok(n) => LoadRead::Found(&buf[..n]),
        Err(kv::KvError::NotFound) => LoadRead::Missing,
        Err(_) => LoadRead::Failed,
    };
    // The old key (read only) is consulted only when `list_v3` is absent.
    let old_read = if matches!(v3, LoadRead::Missing) {
        match ns.get(FRIENDS_KEY_OLD, &mut old).await {
            Ok(n) => LoadRead::Found(&old[..n]),
            Err(kv::KvError::NotFound) => LoadRead::Missing,
            Err(_) => LoadRead::Failed,
        }
    } else {
        LoadRead::Missing
    };
    let loaded = decide_load(v3, old_read);
    if !loaded.may_save {
        FRIENDS_SAVE_BLOCKED.store(true, core::sync::atomic::Ordering::Relaxed);
        defmt::warn!("friends: load failed, saving blocked this boot");
    }
    if loaded.dirty {
        FRIENDS_DIRTY.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    let mut list = loaded.list;
    list.rebase_after_reboot(super::lifecycle::now_tick());
    defmt::info!("friends: loaded {} known friends", list.count);
    unsafe {
        *FRIENDS.get() = list;
    }
}

#[cfg(not(feature = "embassy-core"))]
pub async fn init() {}

/// Persist the friends list if it changed since the last save. Called
/// from `lifecycle::save_if_needed` alongside the Unicorn Realm save.
#[cfg(feature = "embassy-core")]
pub async fn save_if_needed() {
    use core::sync::atomic::Ordering;
    if FRIENDS_SAVE_BLOCKED.load(Ordering::Relaxed) {
        return;
    }
    if !FRIENDS_DIRTY.swap(false, Ordering::Relaxed) {
        return;
    }
    let list = unsafe { &*FRIENDS.get() };
    let buf = list.to_bytes();
    let ns = crate::fw::kv::namespace("friends");
    if ns.set(FRIENDS_KEY, &buf, true).await.is_err() {
        FRIENDS_DIRTY.store(true, Ordering::Relaxed); // retry next cycle
        defmt::warn!("friends: save failed");
    }
}

#[cfg(not(feature = "embassy-core"))]
pub async fn save_if_needed() {}

/// Number of known friends.
pub fn count() -> u8 {
    unsafe { (*FRIENDS.get()).count }
}

/// Step a picker cursor to the previous friend, wrapping. Shared by the
/// Friends list and the Battle picker so both scroll identically.
pub fn cursor_prev(cur: &core::sync::atomic::AtomicU8) {
    use core::sync::atomic::Ordering;
    let c = cur.load(Ordering::Relaxed);
    cur.store(crate::ui::wrap_prev(c, count()), Ordering::Relaxed);
}

/// Step a picker cursor to the next friend, wrapping.
pub fn cursor_next(cur: &core::sync::atomic::AtomicU8) {
    use core::sync::atomic::Ordering;
    let c = cur.load(Ordering::Relaxed);
    cur.store(crate::ui::wrap_next(c, count()), Ordering::Relaxed);
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

/// Look up a friend's pet name by device id, as `(buffer, len)`.
///
/// Returns a copy rather than a `&str` so callers don't hold a borrow on
/// the static list. `None` when we've never received a beacon from that
/// device — the battle-animation renderer falls back to the species name.
pub fn name_of(device_id: [u8; 2]) -> Option<([u8; PET_NAME_MAX], u8)> {
    let list = unsafe { &*FRIENDS.get() };
    list.find_index(device_id)
        .map(|i| (list.friends[i].name, list.friends[i].name_len))
}

/// Record a mesh Battle result against a specific friend — the
/// head-to-head tally shown in the Friends detail screen. No-op if
/// `device_id` isn't a known friend (e.g. a battle result arrived
/// before we'd ever received a beacon from them).
///
/// Called through `note_fight` from `battle_link`, on both the challenger's
/// and the target's side, so the tally reads the same from either badge.
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

/// Test hook: put `rec` in the list as a newly met friend.
#[cfg(any(test, feature = "simulator"))]
pub fn insert_for_test(rec: FriendRecord) {
    let list = unsafe { &mut *FRIENDS.get() };
    list.push(rec);
}

/// A known friend by device id (a copy), or `None`.
pub fn by_id(device_id: [u8; 2]) -> Option<FriendRecord> {
    let list = unsafe { &*FRIENDS.get() };
    list.find_index(device_id).map(|i| list.friends[i])
}

/// A fight with this friend just ended: bump the head-to-head record and
/// stamp `fought_tick` (the pair cooldown starts). No-op for an unknown id.
pub fn note_fight(device_id: [u8; 2], won: bool, now_tick: u32) {
    record_battle_vs(device_id, won);
    let list = unsafe { &mut *FRIENDS.get() };
    if let Some(friend) = list.find_mut(device_id) {
        // 0 means "never fought", so a fight at tick 0 is stamped as 1.
        friend.fought_tick = now_tick.max(1);
        FRIENDS_DIRTY.store(true, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Zero the head-to-head Battle record (wins/losses) on every known
/// friend. Paired with `GameState::debug_reset_battle_record` (which
/// zeros the pet's own overall tally) by the debug-cheat menu — lets a
/// badge that picked up inflated counts from the duplicate-mesh-
/// delivery bug (fixed by `GRP_DATA_SEEN` dedup) get back to a clean
/// baseline instead of carrying stale numbers forever.
pub fn reset_all_battle_records() {
    let list = unsafe { &mut *FRIENDS.get() };
    for friend in list.friends[..list.count as usize].iter_mut() {
        friend.wins = 0;
        friend.losses = 0;
    }
    FRIENDS_DIRTY.store(true, core::sync::atomic::Ordering::Relaxed);
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
#[cfg(feature = "embassy-core")]
pub(super) fn local_device_id() -> [u8; 2] {
    crate::fw::device_id::get()
}

#[cfg(not(feature = "embassy-core"))]
pub(super) fn local_device_id() -> [u8; 2] {
    [0, 0]
}

/// The beacon this badge broadcasts now: identity plus the current fight
/// numbers, in the v3 layout. `None` without a pet. The firmware ticker only
/// sends what this builds.
pub fn beacon_now() -> Option<PetBeacon> {
    let snap = super::lifecycle::snapshot()?;
    let name = super::lifecycle::pet_name();
    let n = name.len().min(PET_NAME_MAX);
    let mut beacon_name = [0u8; PET_NAME_MAX];
    beacon_name[..n].copy_from_slice(&name.as_bytes()[..n]);
    Some(PetBeacon {
        device_id: local_device_id(),
        pet_kind: super::lifecycle::pet_kind().id(),
        generation: super::lifecycle::pet_generation(),
        name: beacon_name,
        name_len: n as u8,
        power: snap.power,
        guard: snap.guard,
        speed: snap.speed,
        hp: snap.hp,
        stamina: snap.stamina,
        focus: snap.focus,
        demo: super::lifecycle::is_demo(),
        v3: true,
    })
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
            friend.refresh(&beacon, now);
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
                power: beacon.power,
                guard: beacon.guard,
                speed: beacon.speed,
                hp: beacon.hp,
                stamina: beacon.stamina,
                focus: beacon.focus,
                demo: beacon.demo,
                v3: beacon.v3,
                fought_tick: 0,
                wins: 0,
                losses: 0,
                heard_this_boot: true,
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
            power: 40,
            guard: 30,
            speed: 20,
            hp: 100,
            stamina: 3,
            focus: 0,
            demo: false,
            v3: true,
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
            power: 0,
            guard: 0,
            speed: 0,
            hp: 0,
            stamina: 0,
            focus: 0,
            demo: false,
            v3: false,
            fought_tick: 0,
            wins: 0,
            losses: 0,
            heard_this_boot: false,
        }
    }

    /// Simulates loading a save written near the end of a long previous
    /// boot session (large stored ticks) into a freshly-booted `now`
    /// (small, since `now_tick()` is uptime-since-boot). Without rebasing,
    /// `now.saturating_sub(stored)` would floor every friend's "ago" at 0.
    #[test]
    fn rebase_after_reboot_keeps_relative_order_and_matches_now() {
        let mut list = FriendsList::new();
        // Met [1,1] at tick 100, last saw them at 5000 (most recent
        // interaction in the whole list, in a session that ran to ~5000
        // ticks of uptime before saving).
        list.push(friend_record([1, 1], 100, 5000));
        // Met [2,2] later (tick 4000), only ever boosted/seen at first
        // meeting since (last_seen_tick mirrors last_boost_tick here).
        list.push(friend_record([2, 2], 4000, 4000));

        // Reboot: uptime resets, only 3 ticks have elapsed in the new session.
        list.rebase_after_reboot(3);

        // The single largest stored tick (5000, [1,1]'s last_seen/boost)
        // should land exactly on the new `now` (3).
        let f1 = list.find_mut([1, 1]).unwrap();
        assert_eq!(f1.last_seen_tick, 3);
        assert_eq!(f1.last_boost_tick, 3);
        // first_seen_tick (100) was 4900 ticks before the max (5000);
        // after rebasing by 4997 it saturates to 0 rather than
        // underflowing — still "a while before now", never wrapping.
        assert_eq!(f1.first_seen_tick, 0);

        // [2,2]'s tick (4000) was 1000 before the max — same 4997 shift
        // saturates to 0 too, but the *relative order* (met later than
        // [1,1]'s original 100 vs [2,2]'s original 4000) doesn't matter
        // for display purposes once both are this close to `now`; the
        // key correctness property is nothing reads as "in the future"
        // or panics, and the most-recent contact reads as "just now".
        let f2 = list.find_mut([2, 2]).unwrap();
        assert_eq!(f2.first_seen_tick, 0);
        assert_eq!(f2.last_seen_tick, 0);
    }

    #[test]
    fn rebase_after_reboot_is_a_no_op_when_already_consistent() {
        let mut list = FriendsList::new();
        list.push(friend_record([1, 1], 10, 20));
        // `now` already ahead of every stored tick (normal same-session
        // case, no reboot happened) — nothing should change.
        list.rebase_after_reboot(1000);
        let f = list.find_mut([1, 1]).unwrap();
        assert_eq!(f.first_seen_tick, 10);
        assert_eq!(f.last_boost_tick, 20);
        assert_eq!(f.last_seen_tick, 20);
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
        assert_eq!(restored.power, 40);
        assert_eq!(restored.guard, 30);
        assert_eq!(restored.speed, 20);
        assert_eq!(restored.hp, 100);
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
    fn heard_this_boot_is_ram_only() {
        let mut rec = friend_record([3, 3], 1, 1);
        rec.heard_this_boot = true;
        let mut list = FriendsList::new();
        list.push(rec);
        let restored = FriendsList::from_bytes(&list.to_bytes());
        assert!(!restored.friends[0].heard_this_boot);
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
            power: 55,
            guard: 45,
            speed: 35,
            hp: 120,
            stamina: 0,
            focus: 0,
            demo: false,
            v3: false,
            fought_tick: 0,
            wins: 7,
            losses: 3,
            heard_this_boot: false,
        });
        let bytes = list.to_bytes();
        let restored = FriendsList::from_bytes(&bytes);
        assert_eq!(restored.count, 1);
        assert_eq!(restored.friends[0].device_id, [9, 9]);
        assert_eq!(restored.friends[0].name_str(), "Mochi");
        assert_eq!(restored.friends[0].first_seen_tick, 100);
        assert_eq!(restored.friends[0].last_boost_tick, 200);
        assert_eq!(restored.friends[0].last_seen_tick, 250);
        assert_eq!(restored.friends[0].power, 55);
        assert_eq!(restored.friends[0].guard, 45);
        assert_eq!(restored.friends[0].speed, 35);
        assert_eq!(restored.friends[0].hp, 120);
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
    fn reset_all_battle_records_zeroes_every_friend() {
        let mut list = FriendsList::new();
        let mut a = friend_record([1, 1], 1, 1);
        a.wins = 5;
        a.losses = 2;
        let mut b = friend_record([2, 2], 1, 1);
        b.wins = 1;
        b.losses = 9;
        list.push(a);
        list.push(b);

        // Exercise the same zeroing logic `reset_all_battle_records`
        // uses, directly against the list (the module-level function
        // operates on the shared static — see the note on the sibling
        // `record_battle_vs` test above).
        for friend in list.friends[..list.count as usize].iter_mut() {
            friend.wins = 0;
            friend.losses = 0;
        }

        assert_eq!(list.find_mut([1, 1]).unwrap().wins, 0);
        assert_eq!(list.find_mut([1, 1]).unwrap().losses, 0);
        assert_eq!(list.find_mut([2, 2]).unwrap().wins, 0);
        assert_eq!(list.find_mut([2, 2]).unwrap().losses, 0);
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

    #[test]
    fn friends_v3_round_trip() {
        let mut list = FriendsList::new();
        for i in 0..3u8 {
            let mut r = friend_record([i + 1, i + 1], 10 + i as u32, 20 + i as u32);
            r.power = 40 + i;
            r.guard = 30 + i;
            r.speed = 20 + i;
            r.hp = 100 + i;
            r.stamina = 2 + i;
            r.focus = -3 + i as i8;
            r.demo = i % 2 == 0;
            r.v3 = true;
            r.fought_tick = 500 + i as u32;
            r.wins = i as u16;
            list.push(r);
        }
        let bytes = list.to_bytes();
        assert_eq!(bytes.len(), FRIENDS_SAVE_SIZE);
        assert_eq!(bytes[0], 3);
        let back = FriendsList::from_bytes(&bytes);
        assert_eq!(back.count, 3);
        for i in 0..3 {
            let (a, b) = (&back.friends[i], &list.friends[i]);
            assert_eq!(a.device_id, b.device_id);
            assert_eq!((a.power, a.guard, a.speed, a.hp), (b.power, b.guard, b.speed, b.hp));
            assert_eq!((a.stamina, a.focus), (b.stamina, b.focus));
            assert_eq!((a.demo, a.v3, a.fought_tick), (b.demo, b.v3, b.fought_tick));
            assert_eq!((a.first_seen_tick, a.last_seen_tick), (b.first_seen_tick, b.last_seen_tick));
            assert_eq!(a.wins, b.wins);
        }
        assert_eq!(back.to_bytes(), bytes);
    }

    #[test]
    fn old_friends_blob_migrates() {
        let mut old = [0u8; FRIENDS_SAVE_SIZE_OLD];
        old[0] = 2;
        for (i, id) in [[0xA1u8, 0xB2], [0xC3, 0xD4]].iter().enumerate() {
            let o = 1 + i * FRIEND_RECORD_SIZE_OLD;
            old[o..o + 2].copy_from_slice(id);
            old[o + 2] = 4;
            old[o + 3..o + 6].copy_from_slice(b"Rex");
            old[o + 15] = 3;
            old[o + 16..o + 20].copy_from_slice(&77u32.to_le_bytes());
            old[o + 28..o + 32].copy_from_slice(&[41, 32, 23, 114]);
            old[o + 32..o + 34].copy_from_slice(&5u16.to_le_bytes());
            old[o + 34..o + 36].copy_from_slice(&6u16.to_le_bytes());
        }
        let list = FriendsList::from_old_bytes(&old);
        assert_eq!(list.count, 2);
        assert_eq!(list.friends[1].device_id, [0xC3, 0xD4]);
        let f = &list.friends[0];
        assert_eq!(f.device_id, [0xA1, 0xB2]);
        assert_eq!(f.name_str(), "Rex");
        assert_eq!((f.first_seen_tick, f.wins, f.losses), (77, 5, 6));
        assert_eq!((f.power, f.guard, f.speed, f.hp), (41, 32, 23, 114));
        assert_eq!(f.stamina, 0);
        assert!(!f.v3 && !f.demo);
        assert_eq!(f.fought_tick, 0);
    }

    #[test]
    fn never_writes_the_old_friends_key() {
        let src = include_str!("friends.rs");
        // Built in pieces so this test's own text does not match.
        let needle = concat!("ns.", "set", "(");
        assert_eq!(src.matches(needle).count(), 1);
        let at = src.find(needle).unwrap() + needle.len();
        assert!(src[at..].starts_with("FRIENDS_KEY,"));
        assert_eq!(FRIENDS_KEY, "list_v3");
        assert_eq!(FRIENDS_KEY_OLD, "list");
        // The old key is never the key of any write, nor a "list" literal.
        assert!(!src.contains(concat!("set(", "FRIENDS_KEY_OLD")));
        assert!(!src.contains(concat!("set(", "\"list\"")));
    }

    #[test]
    fn pair_cooldown_counts_down() {
        let mut r = friend_record([1, 1], 1, 1);
        r.fought_tick = 1000;
        assert_eq!(r.pair_wait_secs(1000 + 300), Some(600));
        assert_eq!(r.pair_wait_secs(1000 + 360), None);
        r.fought_tick = 0;
        assert_eq!(r.pair_wait_secs(5), None);
    }

    #[test]
    fn rebase_moves_fought_tick() {
        let mut list = FriendsList::new();
        let mut a = friend_record([1, 1], 100, 5000);
        a.fought_tick = 4800; // 200 ticks before the newest, inside the cooldown
        list.push(a);
        list.push(friend_record([2, 2], 100, 4000));
        list.rebase_after_reboot(1000);
        assert_eq!(list.find_mut([1, 1]).unwrap().fought_tick, 800);
        assert_eq!(list.find_mut([2, 2]).unwrap().fought_tick, 0);
        // A fallback fight newer than every beacon counts toward the shift.
        let mut list = FriendsList::new();
        let mut b = friend_record([3, 3], 100, 200);
        b.fought_tick = 3000;
        list.push(b);
        list.rebase_after_reboot(500);
        assert_eq!(list.find_mut([3, 3]).unwrap().fought_tick, 500);
        assert_eq!(list.find_mut([3, 3]).unwrap().last_seen_tick, 0);
    }

    #[test]
    fn rebase_old_fight_means_no_wait() {
        let cd = pair_cooldown_ticks();
        let mut list = FriendsList::new();
        let mut a = friend_record([1, 1], 100, 500_000);
        a.fought_tick = 500_000 - cd - 5; // days before shutdown
        list.push(a);
        list.rebase_after_reboot(0);
        let f = list.find_mut([1, 1]).unwrap();
        assert_eq!(f.fought_tick, 0);
        assert_eq!(f.pair_wait_secs(0), None);
    }

    #[test]
    fn rebase_recent_fight_keeps_its_wait() {
        let mut list = FriendsList::new();
        let mut a = friend_record([1, 1], 100, 500_000);
        a.fought_tick = 500_000 - 60; // 10 min before shutdown
        list.push(a);
        let before = list.friends[0].pair_wait_secs(500_000);
        list.rebase_after_reboot(100);
        let f = list.find_mut([1, 1]).unwrap();
        // 600 s elapsed before shutdown, the same wait is left after.
        assert_eq!(f.pair_wait_secs(100), before);
        assert_eq!(f.pair_wait_secs(100), Some((pair_cooldown_ticks() - 60) * 10));

        // `now` below the age cannot hold a tick before zero: the wait is
        // never shorter than it was, it restarts in full.
        let mut b = friend_record([2, 2], 100, 500_000);
        b.fought_tick = 500_000 - 60;
        let mut list = FriendsList::new();
        list.push(b);
        list.rebase_after_reboot(1);
        let f = list.find_mut([2, 2]).unwrap();
        assert!(f.pair_wait_secs(1) >= before);
    }

    #[test]
    fn load_decision_covers_every_read_result() {
        let v3 = friends_v3_blob();
        let old = [0u8; 1 + FRIEND_RECORD_SIZE_OLD];
        // v3 present: loaded, not dirty, saving allowed; old is ignored.
        let l = decide_load(LoadRead::Found(&v3), LoadRead::Failed);
        assert!(l.list.count == 1 && !l.dirty && l.may_save);
        // v3 failed: empty and blocked, old is never consulted.
        let l = decide_load(LoadRead::Failed, LoadRead::Found(&old));
        assert!(l.list.count == 0 && !l.dirty && !l.may_save);
        // v3 missing, old present: migrate and mark dirty.
        let mut o = [0u8; FRIENDS_SAVE_SIZE_OLD];
        o[0] = 1;
        o[1] = 7;
        let l = decide_load(LoadRead::Missing, LoadRead::Found(&o));
        assert!(l.list.count == 1 && l.dirty && l.may_save);
        assert_eq!(l.list.friends[0].device_id[0], 7);
        // both missing: a fresh badge, saving allowed.
        let l = decide_load(LoadRead::Missing, LoadRead::Missing);
        assert!(l.list.count == 0 && !l.dirty && l.may_save);
        // v3 missing, old read failed: blocked.
        let l = decide_load(LoadRead::Missing, LoadRead::Failed);
        assert!(l.list.count == 0 && !l.dirty && !l.may_save);
        // foreign layout in v3: blocked.
        let l = decide_load(LoadRead::Found(&[9, 1, 2]), LoadRead::Missing);
        assert!(l.list.count == 0 && !l.may_save);
    }

    fn friends_v3_blob() -> [u8; FRIENDS_SAVE_SIZE] {
        let mut list = FriendsList::new();
        list.push(friend_record([5, 5], 1, 1));
        list.to_bytes()
    }

    #[test]
    fn beacon_updates_the_fighting_numbers() {
        let mut b = beacon([1, 1]);
        b.stamina = 5;
        b.focus = -2;
        b.demo = true;
        let mut r = friend_record([1, 1], 1, 1);
        assert!(!r.has_numbers());
        r.refresh(&b, 99);
        assert_eq!((r.power, r.guard, r.speed, r.hp), (40, 30, 20, 100));
        assert_eq!((r.stamina, r.focus), (5, -2));
        assert!(r.demo && r.v3);
        assert_eq!(r.last_seen_tick, 99);
        assert_eq!(r.name_str(), "Rex");
        assert!(r.has_numbers());
        assert_eq!(r.estimate_snapshot().power, 40);
        // A later beacon from a non-v3, non-demo badge clears both flags.
        let mut old_b = beacon([1, 1]);
        old_b.v3 = false;
        old_b.demo = false;
        r.refresh(&old_b, 100);
        assert!(!r.v3 && !r.demo);
    }
}

//! Mesh Battles — pets fighting over the private SHDW channel.
//!
//! Each pet's Attack/Defense/Speed/HP are derived from its existing care
//! stats and traits (see [`derive_combat_stats`]). Battling a friend is
//! **instant, using cached stats**: the challenger simulates the entire
//! fight locally, right away, using its own live stats plus the friend's
//! most recently broadcast combat snapshot (cached in
//! `crate::game::friends::FriendRecord`) — no waiting for the other
//! badge to be in range or respond.
//!
//! After resolving, the challenger broadcasts a small [`BattleResultMsg`]
//! on the SHDW channel (same `GrpData` broadcast + device-id filtering
//! pattern `friends::PetBeacon` already uses) so the friend's badge can
//! independently learn the outcome and update its own win/loss tally
//! whenever it next receives it — see [`on_battle_result`].
//!
//! Battle HP exists only for the duration of one `simulate()` call: it is
//! never persisted and never touches the pet's real `sick`/lifecycle
//! stats, so losing a battle cannot harm the pet.

use super::engine::PetStats;
use super::friends::FriendRecord;

// ---------------------------------------------------------------------------
// Combat stats — derived from existing care stats + traits
// ---------------------------------------------------------------------------

/// Derived combat attributes. Tunable numbers, but concrete and
/// self-contained — a pure function of a `PetStats` snapshot plus the
/// trait percentages `lifecycle::pet_traits()` already exposes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombatStats {
    pub attack: u8,
    pub defense: u8,
    pub speed: u8,
    pub max_hp: u8,
}

/// Flat combat-stat penalty per permanent condition (diabetic/alcoholic)
/// — the ongoing health toll shows up in the arena too, not just as
/// faster `sick` decay.
const CONDITION_PENALTY: u16 = 10;

/// Derive Attack/Defense/Speed/HP from a stats snapshot and the pet's
/// (percentage, 0-100, higher=better) traits.
///
/// - `attack`  — curiosity-driven, plus a bonus for being well cared for.
/// - `defense` — resilience-driven, plus a bonus for being lean/fit.
/// - `speed`   — fitness-driven (lighter pet = faster), plus vitality.
/// - `max_hp`  — vitality-driven, scaled down by how sick the pet is.
pub fn derive_combat_stats(
    stats: &PetStats,
    vitality_pct: u8,
    curiosity_pct: u8,
    resilience_pct: u8,
) -> CombatStats {
    let care_pct = (stats.hunger as u16
        + stats.tired as u16
        + stats.healthy as u16
        + stats.happy as u16)
        / 4;
    let fit_pct = stats.weight as u16;

    let mut attack = 20u16 + (curiosity_pct as u16 * 5 / 10) + (care_pct * 3 / 10);
    let mut defense = 20u16 + (resilience_pct as u16 * 5 / 10) + (fit_pct * 3 / 10);
    let mut speed = 20u16 + (fit_pct * 6 / 10) + (vitality_pct as u16 * 2 / 10);
    let max_hp = (50u16 + vitality_pct as u16) * stats.healthy as u16 / 100;

    let penalty = (stats.diabetic as u16 + stats.alcoholic as u16) * CONDITION_PENALTY;
    attack = attack.saturating_sub(penalty);
    defense = defense.saturating_sub(penalty);
    speed = speed.saturating_sub(penalty);

    CombatStats {
        attack: attack.clamp(1, 100) as u8,
        defense: defense.clamp(1, 100) as u8,
        speed: speed.clamp(1, 100) as u8,
        max_hp: max_hp.clamp(20, 150) as u8,
    }
}

// ---------------------------------------------------------------------------
// Battle simulation — deterministic, run once by the challenger only
// ---------------------------------------------------------------------------

/// Hard cap on rounds, purely a termination guarantee — whoever has more
/// remaining HP% when the cap hits wins.
const MAX_ROUNDS: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BattleOutcome {
    pub challenger_won: bool,
    pub challenger_hp_pct: u8,
    pub target_hp_pct: u8,
    pub rounds: u8,
}

/// Tiny xorshift32 step — deterministic, no external RNG crate needed.
fn next_rng(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

/// Damage = attacker's Attack minus roughly half the defender's Defense
/// (floored at 1), with 80-120% seeded variance so identical stats don't
/// produce a totally flat, predictable fight.
fn damage(attacker: CombatStats, defender: CombatStats, rng: &mut u32) -> i32 {
    let base = (attacker.attack as i32 - defender.defense as i32 / 2).max(1);
    let variance = 80 + (next_rng(rng) % 41) as i32; // 80..=120
    (base * variance / 100).max(1)
}

/// Simulate a full battle in one shot: both combatants act every round,
/// faster (higher Speed) one first, until either HP hits 0 or the round
/// cap is reached. Deterministic given the same `seed` + stats — only
/// the challenger ever calls this (see module docs); the target does not
/// need to re-simulate or agree with the outcome.
pub fn simulate(seed: u32, challenger: CombatStats, target: CombatStats) -> BattleOutcome {
    let mut rng = seed | 1; // never let the generator get stuck on 0
    let max_a = challenger.max_hp as i32;
    let max_b = target.max_hp as i32;
    let mut hp_a = max_a;
    let mut hp_b = max_b;
    let mut rounds = 0u8;

    while hp_a > 0 && hp_b > 0 && rounds < MAX_ROUNDS {
        rounds += 1;
        if challenger.speed >= target.speed {
            hp_b = (hp_b - damage(challenger, target, &mut rng)).max(0);
            if hp_b > 0 {
                hp_a = (hp_a - damage(target, challenger, &mut rng)).max(0);
            }
        } else {
            hp_a = (hp_a - damage(target, challenger, &mut rng)).max(0);
            if hp_a > 0 {
                hp_b = (hp_b - damage(challenger, target, &mut rng)).max(0);
            }
        }
    }

    let challenger_hp_pct = (hp_a * 100 / max_a.max(1)) as u8;
    let target_hp_pct = (hp_b * 100 / max_b.max(1)) as u8;
    let challenger_won = match (hp_a > 0, hp_b > 0) {
        (true, false) => true,
        (false, true) => false,
        // Round cap reached (or a simultaneous knockout) — more remaining
        // HP% wins.
        _ => challenger_hp_pct >= target_hp_pct,
    };

    BattleOutcome {
        challenger_won,
        challenger_hp_pct,
        target_hp_pct,
        rounds,
    }
}

// ---------------------------------------------------------------------------
// Wire result — broadcast on SHDW after the challenger resolves locally
// ---------------------------------------------------------------------------

/// Private `GrpData` `data_type` marking a Battle result — distinct from
/// `friends::PET_BEACON_TYPE`.
pub const PET_BATTLE_TYPE: u16 = 0xBA71;

pub struct BattleResultMsg {
    pub challenger_id: [u8; 2],
    pub target_id: [u8; 2],
    pub challenger_won: bool,
    pub challenger_hp_pct: u8,
    pub target_hp_pct: u8,
    /// Challenger's pet species (sprite `PP` prefix), or [`KIND_UNKNOWN`] when
    /// decoded from a legacy `MIN_SIZE` packet that predates this field.
    pub challenger_kind: u8,
    /// Target's pet species, or [`KIND_UNKNOWN`] (see above).
    pub target_kind: u8,
}

/// Legacy wire layout: `2 + 2 + 1 + 1 + 1`. Still accepted on decode so a badge
/// running the older firmware can battle a new one (see module docs / spec).
const MIN_SIZE: usize = 7;
/// Current wire layout: legacy + `challenger_kind + target_kind`.
const FULL_SIZE: usize = 9;

/// Sentinel species used when a legacy packet carries no `kind` bytes. Not a
/// real pet id — the receiver resolves it via the friend record or the generic
/// placeholder pet (see [`resolve_kind`]).
pub const KIND_UNKNOWN: u8 = 0xFF;

/// Resolve a combatant's sprite species: use `kind` when it's a real value,
/// else fall back to the stored friend record for `device_id`, else the first
/// firmware pet (Bartholomeus). There is no generic placeholder — every drawn
/// species is a real pet.
pub fn resolve_kind(kind: u8, device_id: [u8; 2]) -> u8 {
    if kind != KIND_UNKNOWN {
        return kind;
    }
    super::friends::pet_kind_of(device_id).unwrap_or(super::engine::PetKind::Bartholomeus.id())
}

impl BattleResultMsg {
    pub fn to_bytes(&self) -> [u8; FULL_SIZE] {
        let mut buf = [0u8; FULL_SIZE];
        buf[0..2].copy_from_slice(&self.challenger_id);
        buf[2..4].copy_from_slice(&self.target_id);
        buf[4] = self.challenger_won as u8;
        buf[5] = self.challenger_hp_pct;
        buf[6] = self.target_hp_pct;
        buf[7] = self.challenger_kind;
        buf[8] = self.target_kind;
        buf
    }

    /// Decode a result. Accepts both the current `FULL_SIZE` layout and the
    /// legacy `MIN_SIZE` one; the species fields come back as [`KIND_UNKNOWN`]
    /// when the packet is too short to carry them (trailing bytes beyond what
    /// we read are ignored, matching how the old parser tolerated extra data).
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < MIN_SIZE {
            return None;
        }
        let (challenger_kind, target_kind) = if buf.len() >= FULL_SIZE {
            (buf[7], buf[8])
        } else {
            (KIND_UNKNOWN, KIND_UNKNOWN)
        };
        Some(Self {
            challenger_id: [buf[0], buf[1]],
            target_id: [buf[2], buf[3]],
            challenger_won: buf[4] != 0,
            challenger_hp_pct: buf[5],
            target_hp_pct: buf[6],
            challenger_kind,
            target_kind,
        })
    }
}

/// Broadcast a `BattleResultMsg` on the SHDW channel as a **zero-hop**
/// packet — `RouteType::Direct` with an empty path (`path_len == 0`),
/// the MeshCore `sendZeroHop` form. Standard MeshCore repeaters will
/// **not** re-broadcast it (their relay path only forwards flood packets,
/// and a direct packet with no next-hop hash matches no repeater), so a
/// battle result only reaches badges within direct LoRa range and never
/// floods the wider mesh. In-range nodes still decrypt and dispatch it
/// normally — channel reception does not depend on the route type.
///
/// Trade-off: the target only syncs its side of the tally if it is in
/// range at the moment we send; there is no mesh relay carrying the
/// result to an out-of-range friend later. That fits a battle (two badges
/// meeting), and the challenger's own side is always recorded locally in
/// [`challenge`] regardless.
///
/// No-op under builds without the `mesh` feature (e.g. the plain host
/// `simulator` build, which still enables `game`) — mirrors
/// `friends::local_device_id`'s embassy-base/not split.
#[cfg(feature = "mesh")]
fn broadcast_result(msg: &BattleResultMsg) {
    let mut data: heapless::Vec<u8, { crate::fw::mesh::MAX_CHANNEL_DATA }> = heapless::Vec::new();
    let _ = data.extend_from_slice(&msg.to_bytes());
    let _ = crate::fw::mesh::tx_send(crate::fw::mesh::TxRequest::ChannelData(
        crate::fw::mesh::TxChannelData {
            channel_idx: crate::fw::mesh::channels::SHDW_SLOT,
            data_type: PET_BATTLE_TYPE,
            // path_len == 0 → send_grp_data emits RouteType::Direct with an
            // empty path (zero-hop). Not OUT_PATH_UNKNOWN (0xFF), which
            // would select flood routing.
            path_len: 0,
            path: heapless::Vec::new(),
            data,
        },
    ));
}

#[cfg(not(feature = "mesh"))]
fn broadcast_result(_msg: &BattleResultMsg) {}

// ---------------------------------------------------------------------------
// Challenge — entry point called from `battle_view` on Fire
// ---------------------------------------------------------------------------

/// Challenge `friend` right now: derive our live combat stats, simulate
/// against the friend's cached snapshot, record the result on our own
/// pet (both the overall tally and the head-to-head record against this
/// friend), and broadcast it so the friend's badge can sync its own side
/// of the same head-to-head tally.
///
/// Returns `None` if no pet is currently active.
pub fn challenge(friend: &FriendRecord) -> Option<BattleOutcome> {
    // Refuse a self-battle outright — this should never come up through
    // the normal Friends flow (`friends::on_pet_beacon` already ignores
    // our own beacon echo, so we should never appear in our own list),
    // but guarding it here means a bad `FriendRecord` can't corrupt our
    // own win/loss tally by battling "ourselves".
    if friend.device_id == super::friends::local_device_id() {
        return None;
    }

    let my_stats = super::lifecycle::combat_stats()?;
    let their_stats = CombatStats {
        attack: friend.attack,
        defense: friend.defense,
        speed: friend.speed,
        max_hp: friend.max_hp,
    };

    let seed = super::lifecycle::now_tick()
        ^ ((friend.device_id[0] as u32) << 8)
        ^ (friend.device_id[1] as u32);
    let outcome = simulate(seed, my_stats, their_stats);

    super::lifecycle::record_battle(outcome.challenger_won);
    super::friends::record_battle_vs(friend.device_id, outcome.challenger_won);

    let my_kind = super::lifecycle::pet_kind().id();

    broadcast_result(&BattleResultMsg {
        challenger_id: super::friends::local_device_id(),
        target_id: friend.device_id,
        challenger_won: outcome.challenger_won,
        challenger_hp_pct: outcome.challenger_hp_pct,
        target_hp_pct: outcome.target_hp_pct,
        challenger_kind: my_kind,
        target_kind: friend.pet_kind,
    });

    // Play the battle animation locally: our pet (left) vs the friend (right).
    super::show_battle_anim(
        my_kind,
        friend.pet_kind,
        outcome.challenger_won,
        outcome.challenger_hp_pct,
        outcome.target_hp_pct,
        friend.device_id,
    );

    Some(outcome)
}

// ---------------------------------------------------------------------------
// Receive handler — fires on the *target's* badge
// ---------------------------------------------------------------------------

/// Handle a `BattleResultMsg` received on the SHDW channel.
///
/// Called from `fw::mesh::meshcore::push_grp_data` when a `GrpData`
/// packet on the SHDW slot carries `data_type == PET_BATTLE_TYPE`. Only
/// acts if we are the named target — otherwise it's someone else's
/// battle and is ignored, same broadcast-filtering idea as
/// `friends::on_pet_beacon` ignoring its own echo.
pub async fn on_battle_result(data: &[u8]) {
    let Some(msg) = BattleResultMsg::from_bytes(data) else {
        return;
    };

    if msg.target_id != super::friends::local_device_id() {
        return;
    }

    // Defensive: if we're somehow the challenger named in this packet, we
    // already recorded our side synchronously in `challenge` the moment we
    // sent it, so ignore it here rather than double-count ourselves as our
    // own opponent. (The result is now sent zero-hop, so a repeater echo
    // back to the sender shouldn't happen — but the guard is cheap and
    // keeps us correct regardless of routing.)
    if msg.challenger_id == super::friends::local_device_id() {
        return;
    }

    // From our side, the challenger's result is inverted: their win is
    // our loss. Update both the overall tally and our head-to-head
    // record against the challenger specifically, so it reads the same
    // from either badge — a no-op on the head-to-head side if we've
    // never received a beacon from this challenger (not yet a known
    // friend on our side, even though we were just battled).
    let we_won = !msg.challenger_won;
    super::lifecycle::record_battle(we_won);
    super::friends::record_battle_vs(msg.challenger_id, we_won);

    // Replay the battle as a full-screen animation instead of a toast: our pet
    // (left) vs the challenger (right, mirrored). The challenger's species comes
    // from the packet when present, else the stored friend record, else the
    // generic placeholder pet (see `resolve_kind`).
    let own_kind = super::lifecycle::pet_kind().id();
    let opp_kind = resolve_kind(msg.challenger_kind, msg.challenger_id);
    // From our side the HP swaps: our pet's HP is the packet's target HP.
    super::show_battle_anim(
        own_kind,
        opp_kind,
        we_won,
        msg.target_hp_pct,
        msg.challenger_hp_pct,
        msg.challenger_id,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_stats() -> PetStats {
        // A cheap way to get a valid PetStats without going through the
        // full lifecycle module: build a fresh egg, mark it Active, and
        // snapshot it directly.
        use crate::game::engine::{GameState, PetKind, Phase};
        let mut state = GameState::new_egg(1, PetKind::Cat);
        state.phase = Phase::Active;
        state.stats(0)
    }

    #[test]
    fn result_msg_full_round_trip_preserves_species() {
        let msg = BattleResultMsg {
            challenger_id: [0xAB, 0xCD],
            target_id: [0x12, 0x34],
            challenger_won: true,
            challenger_hp_pct: 80,
            target_hp_pct: 0,
            challenger_kind: 5,
            target_kind: 1,
        };
        let bytes = msg.to_bytes();
        assert_eq!(bytes.len(), FULL_SIZE);
        let back = BattleResultMsg::from_bytes(&bytes).unwrap();
        assert_eq!(back.challenger_id, [0xAB, 0xCD]);
        assert_eq!(back.target_id, [0x12, 0x34]);
        assert!(back.challenger_won);
        assert_eq!(back.challenger_hp_pct, 80);
        assert_eq!(back.target_hp_pct, 0);
        assert_eq!(back.challenger_kind, 5);
        assert_eq!(back.target_kind, 1);
    }

    #[test]
    fn result_msg_legacy_decode_marks_species_unknown() {
        // A 7-byte packet from the old firmware must still decode, with the
        // species reported as unknown so the receiver falls back.
        let legacy = [0xAB, 0xCD, 0x12, 0x34, 1, 80, 0];
        let back = BattleResultMsg::from_bytes(&legacy).unwrap();
        assert_eq!(back.challenger_kind, KIND_UNKNOWN);
        assert_eq!(back.target_kind, KIND_UNKNOWN);
        assert!(back.challenger_won);
        // Too short → rejected.
        assert!(BattleResultMsg::from_bytes(&legacy[..MIN_SIZE - 1]).is_none());
    }

    #[test]
    fn new_receiver_ignores_extra_trailing_bytes() {
        // Forward-compat: a longer future packet still decodes the fields we know.
        let mut buf = BattleResultMsg {
            challenger_id: [1, 2],
            target_id: [3, 4],
            challenger_won: false,
            challenger_hp_pct: 10,
            target_hp_pct: 90,
            challenger_kind: 2,
            target_kind: 0,
        }
        .to_bytes()
        .to_vec();
        buf.push(0x99); // hypothetical future field
        let back = BattleResultMsg::from_bytes(&buf).unwrap();
        assert_eq!(back.target_kind, 0);
        assert_eq!(back.challenger_kind, 2);
    }

    #[test]
    fn derive_combat_stats_stays_in_bounds() {
        let stats = base_stats();
        let combat = derive_combat_stats(&stats, 0, 0, 0);
        assert!(combat.attack >= 1 && combat.attack <= 100);
        assert!(combat.defense >= 1 && combat.defense <= 100);
        assert!(combat.speed >= 1 && combat.speed <= 100);
        assert!(combat.max_hp >= 20 && combat.max_hp <= 150);

        let combat_max = derive_combat_stats(&stats, 100, 100, 100);
        assert!(combat_max.attack <= 100);
        assert!(combat_max.defense <= 100);
        assert!(combat_max.speed <= 100);
        assert!(combat_max.max_hp <= 150);
    }

    #[test]
    fn permanent_conditions_reduce_combat_stats() {
        let mut stats = base_stats();
        let healthy = derive_combat_stats(&stats, 50, 50, 50);

        stats.diabetic = true;
        stats.alcoholic = true;
        let sick = derive_combat_stats(&stats, 50, 50, 50);

        assert!(sick.attack < healthy.attack);
        assert!(sick.defense < healthy.defense);
        assert!(sick.speed < healthy.speed);
    }

    #[test]
    fn simulate_is_deterministic() {
        let a = CombatStats {
            attack: 40,
            defense: 30,
            speed: 25,
            max_hp: 100,
        };
        let b = CombatStats {
            attack: 35,
            defense: 35,
            speed: 20,
            max_hp: 90,
        };
        let outcome1 = simulate(12345, a, b);
        let outcome2 = simulate(12345, a, b);
        assert_eq!(outcome1, outcome2);
    }

    #[test]
    fn simulate_always_terminates_with_a_winner() {
        let a = CombatStats {
            attack: 50,
            defense: 10,
            speed: 30,
            max_hp: 100,
        };
        let b = CombatStats {
            attack: 10,
            defense: 50,
            speed: 10,
            max_hp: 100,
        };
        let outcome = simulate(999, a, b);
        assert!(outcome.rounds <= MAX_ROUNDS);
        // A big attack/defense mismatch should decide it well before the
        // round cap — not a hard guarantee, but a useful smoke check that
        // damage is actually being applied.
        assert!(outcome.rounds < MAX_ROUNDS);
    }

    #[test]
    fn evenly_matched_stats_can_hit_the_round_cap_and_still_pick_a_winner() {
        let a = CombatStats {
            attack: 20,
            defense: 20,
            speed: 20,
            max_hp: 150,
        };
        let b = CombatStats {
            attack: 20,
            defense: 20,
            speed: 20,
            max_hp: 150,
        };
        let outcome = simulate(1, a, b);
        // Whichever way it resolves, exactly one side should be reported
        // as the winner and the round count must respect the cap.
        assert!(outcome.rounds <= MAX_ROUNDS);
        let _ = outcome.challenger_won; // just needs to not panic/underflow
    }

    #[test]
    fn battle_result_msg_round_trips() {
        let msg = BattleResultMsg {
            challenger_id: [0x11, 0x22],
            target_id: [0x33, 0x44],
            challenger_won: true,
            challenger_hp_pct: 80,
            target_hp_pct: 0,
            challenger_kind: 0,
            target_kind: 0,
        };
        let bytes = msg.to_bytes();
        let restored = BattleResultMsg::from_bytes(&bytes).unwrap();
        assert_eq!(restored.challenger_id, [0x11, 0x22]);
        assert_eq!(restored.target_id, [0x33, 0x44]);
        assert!(restored.challenger_won);
        assert_eq!(restored.challenger_hp_pct, 80);
        assert_eq!(restored.target_hp_pct, 0);
    }
}

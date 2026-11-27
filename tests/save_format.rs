//! Golden-bytes tests for the persisted / on-air record encodings.
//!
//! These pin the exact byte layout of everything the firmware writes to flash
//! (game state, Unicorn Realm, friends list) or sends over the mesh (pet
//! beacon, battle result). The literals were captured from the encoders; they
//! must never change, or saves written by an older firmware stop loading.
//! `GAME_GOLDEN` is the format-2 save (97 bytes, kv key `state`); the v3
//! save (177 bytes, key `state_v3`) is pinned by `V3_GOLDEN`.

use bornhack_aegg::game::battle::{
    AcceptMsg, BattleResultMsg, ChallengeMsg, DeclineMsg, KIND_UNKNOWN,
};
use bornhack_aegg::game::engine::combat::{Flags, Snapshot};
use bornhack_aegg::game::engine::doses::Reason;
use bornhack_aegg::game::engine::advance::SLOW_SCALE;
use bornhack_aegg::game::engine::thresholds;
use bornhack_aegg::game::engine::{
    Action, DrinkKind, FoodKind, GameState, PET_RECORD_SIZE, PetKind, PetRealm, PetRecord, Phase,
    REALM_MAX_PETS, REALM_SAVE_SIZE, SAVE_FORMAT_VERSION, SAVE_SIZE, SAVE_SIZE_V2, pick_save,
};
use bornhack_aegg::game::friends::{
    FRIENDS_MAX, FRIENDS_SAVE_SIZE, FRIENDS_SAVE_SIZE_OLD, FriendRecord, FriendsList, PetBeacon,
};

// ---------------------------------------------------------------------------
// GameState
// ---------------------------------------------------------------------------

/// Format-2 save, read only by the upgrade path.
const GAME_GOLDEN: [u8; 97] = [
    0x02, 0x70, 0x11, 0x73, 0x12, 0x76, 0x13, 0x79, 0x14, 0x7C, 0x15, 0x7F, 0x16, 0x82, 0x17, 0x85,
    0x18, 0x88, 0x19, 0x8C, 0x6A, 0x49, 0x2A, 0x8D, 0x6B, 0x4A, 0x2B, 0x01, 0x94, 0x1D, 0x90, 0x6E,
    0x4D, 0x2E, 0x9A, 0x1F, 0x07, 0x41, 0xA3, 0x22, 0xA6, 0x23, 0xA9, 0x24, 0xAC, 0x25, 0xAF, 0x26,
    0xB2, 0x27, 0xB5, 0x28, 0xB8, 0x29, 0xBB, 0x2A, 0x9D, 0x7B, 0x5A, 0x3B, 0x9E, 0x7C, 0x5B, 0x3C,
    0x9F, 0x7D, 0x5C, 0x3D, 0xA0, 0x7E, 0x5D, 0x3E, 0x01, 0x00, 0x01, 0x00, 0xA5, 0x83, 0x62, 0x43,
    0xA6, 0x84, 0x63, 0x44, 0x05, 0xDF, 0x36, 0xE2, 0x37, 0xE5, 0x38, 0xAB, 0x89, 0x68, 0x49, 0x01,
    0x01,
];

/// Format-3 save of `v3_state()`, captured from the encoder once the
/// offsets in `v3_golden_bytes_round_trip` were checked by hand.
const V3_GOLDEN: [u8; 177] = [
    0x03, 0x92, 0xC7, 0x8C, 0xED, 0x89, 0xEC, 0x86, 0xEB, 0xAC, 0x8A, 0x7F, 0x16, 0x82, 0x17, 0x85,
    0x18, 0x88, 0x19, 0x8C, 0x6A, 0x49, 0x2A, 0x8D, 0x6B, 0x4A, 0x2B, 0x01, 0x94, 0x1D, 0x15, 0x14,
    0x13, 0x12, 0x9A, 0x1F, 0x0A, 0x09, 0x01, 0x51, 0x01, 0x41, 0x02, 0x52, 0x23, 0x01, 0x33, 0x00,
    0x02, 0x42, 0x03, 0x43, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x0D, 0x0C, 0x0B, 0x0A, 0x11, 0x10, 0x0F, 0x0E, 0x01, 0x00, 0x01, 0x01, 0xA5, 0x83, 0x62, 0x43,
    0xA6, 0x84, 0x63, 0x44, 0x05, 0xDF, 0x36, 0xE2, 0x37, 0xE5, 0x38, 0xAB, 0x89, 0x68, 0x49, 0x01,
    0x01, 0x01, 0x21, 0x02, 0x22, 0x03, 0x23, 0x04, 0x24, 0x05, 0x25, 0x01, 0x30, 0x02, 0x31, 0x03,
    0x32, 0x04, 0x33, 0x05, 0x34, 0x06, 0x35, 0x07, 0x36, 0x08, 0x37, 0x09, 0x38, 0x0A, 0x39, 0x0B,
    0x3A, 0x0C, 0x3B, 0x0F, 0x03, 0x04, 0x05, 0x04, 0x03, 0x02, 0x01, 0x08, 0x07, 0x06, 0x05, 0x03,
    0x04, 0x24, 0x23, 0x22, 0x21, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00,
];

// Offsets into the 97-byte head shared by formats 2 and 3.
const PHASE_OFS: usize = 1 + 18 + 8;
const ACTION_OFS: usize = PHASE_OFS + 1 + 2 + 4 + 2;
const OVERWEIGHT_TICKS_OFS: usize = 64;
const DRUNK_TICKS_OFS: usize = 68;
const FLAGS_OFS: usize = 72;
// Offsets of the format-3 extension.
const V3_FLAGS_OFS: usize = 131;
const V3_FOOD_OFS: usize = 143;
const V3_DRINK_OFS: usize = 144;
const V3_SAVED_UNIX_OFS: usize = 145;

fn v2_decoded() -> GameState {
    GameState::from_bytes_v2(&GAME_GOLDEN).expect("golden v2 save must upgrade")
}

/// A state with a distinct value in every persisted field, built from
/// the upgraded golden save so private counters are covered too.
fn v3_state() -> GameState {
    let mut s = v2_decoded();
    s.hydration = 0x2101;
    s.fun = 0x2202;
    s.strain = 0x2303;
    s.dirt = 0x2404;
    s.fitness = 0x2505;
    s.sleep_min = 0x3001;
    s.oversleep_min = 0x3102;
    s.good_sleep_min = 0x3203;
    s.rested_min = 0x3304;
    s.injured_min = 0x3405;
    s.groggy_min = 0x3506;
    s.hangover_min = 0x3607;
    s.rehab_min = 0x3708;
    s.recovering_min = 0x3809;
    s.junk_min = 0x390A;
    s.ozempic_min = 0x3A0B;
    s.reversal_min = 0x3B0C;
    s.med_cd_min = 0x4101;
    s.insulin_min = 0x4202;
    s.ozempic_cd_min = 0x4303;
    s.booze_peak = true;
    s.deprived = true;
    s.night = true;
    s.no_groggy = true;
    s.short_nights = 3;
    s.good_nights = 4;
    s.tick_rem = 5;
    s.care_good_min = 0x0102_0304;
    s.care_awake_min = 0x0506_0708;
    s.diabetes_min = 0x0A0B_0C0D;
    s.dependence_min = 0x0E0F_1011;
    s.leaving_min = 0x1213_1415;
    s.dependent = true;
    s.active_action = Some(Action::Wash);
    s.action_ticks_remaining = 9;
    s.saved_unix = 0x2122_2324;
    s.active_food = Some(FoodKind::Pizza);
    s.active_drink = Some(DrinkKind::Whiskey);
    s.demo_wins = 0x5101;
    s.demo_losses = 0x5202;
    s.prize_min = 0x0123;
    s.decline_min = 0x0033;
    s
}

#[test]
fn v3_golden_bytes_round_trip() {
    assert_eq!(SAVE_SIZE, 177);
    assert_eq!(SAVE_FORMAT_VERSION, 3);
    let s = v3_state();
    let bytes = s.to_bytes();
    assert_eq!(bytes.len(), 177);
    assert_eq!(bytes[0], 3);

    // The head keeps the v2 offsets; spot-check the renamed slots.
    assert_eq!(&bytes[1..3], &s.fullness.to_le_bytes());
    assert_eq!(&bytes[3..5], &s.energy.to_le_bytes());
    assert_eq!(&bytes[5..7], &s.health.to_le_bytes());
    assert_eq!(&bytes[7..9], &s.mood.to_le_bytes());
    assert_eq!(&bytes[9..11], &s.weight.to_le_bytes());
    assert_eq!(&bytes[11..13], &s.booze.to_le_bytes());
    assert_eq!(&bytes[30..34], &s.leaving_min.to_le_bytes());
    assert_eq!(bytes[ACTION_OFS], 10); // Wash
    assert_eq!(bytes[ACTION_OFS + 1], 9);
    assert_eq!(&bytes[40..42], &s.med_cd_min.to_le_bytes()); // old cooldown_heal
    assert_eq!(&bytes[48..50], &s.insulin_min.to_le_bytes()); // old cooldown_medicate
    assert_eq!(&bytes[50..52], &s.ozempic_cd_min.to_le_bytes()); // old cooldown_ozempic
    // The battle fields sit in the old feed, relax, play and exercise slots.
    assert_eq!(&bytes[38..40], &0x5101u16.to_le_bytes()); // demo_wins
    assert_eq!(&bytes[42..44], &0x5202u16.to_le_bytes()); // demo_losses
    assert_eq!(&bytes[44..46], &0x0123u16.to_le_bytes()); // prize_min
    assert_eq!(&bytes[46..48], &0x0033u16.to_le_bytes()); // decline_min
    for ofs in [52usize, 54] {
        assert_eq!(&bytes[ofs..ofs + 2], &[0, 0], "removed cooldown slot {ofs}");
    }
    assert_eq!(&bytes[56..64], &[0u8; 8]); // removed interval counters
    assert_eq!(
        &bytes[OVERWEIGHT_TICKS_OFS..OVERWEIGHT_TICKS_OFS + 4],
        &s.diabetes_min.to_le_bytes()
    );
    assert_eq!(
        &bytes[DRUNK_TICKS_OFS..DRUNK_TICKS_OFS + 4],
        &s.dependence_min.to_le_bytes()
    );
    assert_eq!(bytes[FLAGS_OFS], 1);
    assert_eq!(bytes[FLAGS_OFS + 3], 1); // dependent

    // The extension.
    assert_eq!(&bytes[97..99], &0x2101u16.to_le_bytes()); // hydration
    assert_eq!(&bytes[107..109], &0x3001u16.to_le_bytes()); // sleep_min
    assert_eq!(&bytes[129..131], &0x3B0Cu16.to_le_bytes()); // reversal_min
    assert_eq!(bytes[V3_FLAGS_OFS], 0b1111); // booze_peak, deprived, night, no_groggy
    assert_eq!(&bytes[132..135], &[3, 4, 5]);
    assert_eq!(&bytes[135..139], &0x0102_0304u32.to_le_bytes());
    assert_eq!(&bytes[139..143], &0x0506_0708u32.to_le_bytes());
    assert_eq!(bytes[V3_FOOD_OFS], 3); // Pizza
    assert_eq!(bytes[V3_DRINK_OFS], 4); // Whiskey
    assert_eq!(
        &bytes[V3_SAVED_UNIX_OFS..V3_SAVED_UNIX_OFS + 4],
        &0x2122_2324u32.to_le_bytes()
    );
    assert_eq!(&bytes[149..177], &[0u8; 28]);

    // Every field comes back, including the private counters.
    let back = GameState::from_bytes(&bytes).expect("v3 save must decode");
    assert_eq!(back, s);
    assert_eq!(back.to_bytes(), bytes);

    // The captured literal pins the layout forever.
    assert_eq!(bytes, V3_GOLDEN);
}

#[test]
fn v3_keeps_running_action_food_and_drink() {
    let mut s = v3_state();
    for (a, f, d) in [
        (Some(Action::Feed), Some(FoodKind::Salad), None),
        (Some(Action::Drink), None, Some(DrinkKind::Water)),
        (None, None, None),
    ] {
        s.active_action = a;
        s.active_food = f;
        s.active_drink = d;
        let back = GameState::from_bytes(&s.to_bytes()).unwrap();
        assert_eq!(back.active_action, a);
        assert_eq!(back.active_food, f);
        assert_eq!(back.active_drink, d);
        assert_eq!(back.action_ticks_remaining, 9);
    }
}

/// Cola became coffee in the same slot: a save made mid-Cola (drink byte 1)
/// loads as a coffee, and a coffee saves as byte 1.
#[test]
fn old_cola_drink_byte_loads_as_coffee() {
    let mut s = v3_state();
    s.active_action = Some(Action::Drink);
    s.active_food = None;
    s.active_drink = Some(DrinkKind::Coffee);
    let mut bytes = s.to_bytes();
    assert_eq!(bytes[V3_DRINK_OFS], 1);
    bytes[V3_DRINK_OFS] = 1;
    let back = GameState::from_bytes(&bytes).unwrap();
    assert_eq!(back.active_drink, Some(DrinkKind::Coffee));
    assert_eq!(DrinkKind::ALL[1], DrinkKind::Coffee);
}

/// Caffeine sits in the reserved tail right after the demo block (bytes
/// 160..162): it comes back from a save, and an older save (zeros there)
/// reads as no caffeine.
#[test]
fn caffeine_round_trips_in_the_reserved_tail() {
    let mut s = v3_state();
    s.caffeine_min = 0x1234;
    let bytes = s.to_bytes();
    assert_eq!(&bytes[160..162], &0x1234u16.to_le_bytes());
    assert_eq!(&bytes[162..177], &[0u8; 15], "the rest stays reserved");
    let back = GameState::from_bytes(&bytes).unwrap();
    assert_eq!(back.caffeine_min, 0x1234);
    assert_eq!(back, s);

    let old = GameState::from_bytes(&V3_GOLDEN).unwrap();
    assert_eq!(old.caffeine_min, 0);
}

#[test]
fn action_bytes_round_trip_and_2_is_unused() {
    let all = [
        (Action::Feed, 0u8),
        (Action::Heal, 1),
        (Action::Play, 3),
        (Action::Exercise, 4),
        (Action::Medicate, 5),
        (Action::Ozempic, 6),
        (Action::Drink, 7),
        (Action::Rehab, 8),
        (Action::OnlyPets, 9),
        (Action::Wash, 10),
        (Action::Relax, 11),
    ];
    for (a, byte) in all {
        assert_eq!(a.to_u8(), byte, "{a:?}");
        assert_eq!(Action::from_u8(byte), Some(a));
    }
    assert_eq!(Action::from_u8(2), None);
    assert_eq!(Action::from_u8(12), None);
    assert_eq!(Action::from_u8(0xFF), None);
}

#[test]
fn v2_golden_upgrades_to_expected_v3_levels() {
    let s = v2_decoded();
    let max = 65535u32;
    assert_eq!(s.phase, Phase::Active);
    assert_eq!(s.health as u32, max - 0x1376);
    assert_eq!(s.energy as u32, max - 0x1273);
    assert_eq!(s.mood as u32, max - 0x1479);
    assert_eq!(s.booze, 0x167F);
    // fullness = clamp(MAX - 0x1170, 25 %, 78 %)
    assert!(
        s.fullness >= 25 * 655 && s.fullness <= 78 * 655,
        "{}",
        s.fullness
    );
    assert_eq!(s.fullness, 78 * 655);
    assert_eq!(s.weight, 50 * 655 + 0x157C / 2);
    assert!(s.weight >= 50 * 655);
    assert_eq!(s.fitness, 60 * 655);
    assert_eq!(s.hydration, 60 * 655);
    assert_eq!(s.fun, 50 * 655);
    assert_eq!(s.dirt, 20 * 655);
    assert_eq!(s.strain, 0);
    // Copied over.
    assert_eq!(s.money, 0x4968_89AB);
    assert_eq!(s.generation, 0x1F9A);
    assert_eq!(s.wins, 0x36DF);
    assert_eq!(s.losses, 0x37E2);
    assert!(s.diabetic);
    assert!(!s.dependent);
    assert_eq!(s.pet_kind, PetKind::from_u8(5));
    assert_eq!(s.age_ticks, 0x2B4A_6B8D);
    assert_eq!(s.vitality, 0x1782);
    assert_eq!(s.hibernate_ticks, 0x4362_83A5);
    assert!(s.money_enabled);
    assert!(s.hard_mode);
    // cooldown_medicate (0x27B2 ticks = 1693 min) is capped at 480 min.
    assert_eq!(s.insulin_min, 480);
    let mut b = GAME_GOLDEN;
    b[48..50].copy_from_slice(&60u16.to_le_bytes());
    assert_eq!(GameState::from_bytes_v2(&b).unwrap().insulin_min, 10);
    // A running action is dropped; sleep is kept and marked night.
    assert_eq!(s.active_action, None);
    assert_eq!(s.action_ticks_remaining, 0);
    assert!(s.is_sleeping);
    assert!(s.night);
}

#[test]
fn v2_upgrade_scales_slow_counters() {
    let onset_dep = thresholds::DEPENDENCE_ONSET_MIN() as u32 * SLOW_SCALE - 1;
    let onset_dia = thresholds::DIABETES_ONSET_MIN() as u32 * SLOW_SCALE - 1;

    // Huge counters cap one quarter-minute under onset and do not flip a flag.
    let mut b = GAME_GOLDEN;
    b[DRUNK_TICKS_OFS..DRUNK_TICKS_OFS + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    b[OVERWEIGHT_TICKS_OFS..OVERWEIGHT_TICKS_OFS + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let s = GameState::from_bytes_v2(&b).unwrap();
    assert_eq!(s.dependence_min, onset_dep);
    assert_eq!(s.diabetes_min, onset_dia);
    assert!(!s.dependent);

    // Small counters: ticks / 6 minutes, 4 quarter-minutes each.
    let mut b = GAME_GOLDEN;
    b[DRUNK_TICKS_OFS..DRUNK_TICKS_OFS + 4].copy_from_slice(&600u32.to_le_bytes());
    b[OVERWEIGHT_TICKS_OFS..OVERWEIGHT_TICKS_OFS + 4].copy_from_slice(&66u32.to_le_bytes());
    let s = GameState::from_bytes_v2(&b).unwrap();
    assert_eq!(s.dependence_min, 100 * 4);
    assert_eq!(s.diabetes_min, 11 * 4);
}

#[test]
fn v2_upgrade_leaves_battle_slots_zero() {
    // A format-2 save has old cooldowns in the slots format 3 reuses.
    let mut b = GAME_GOLDEN;
    for (i, byte) in b[38..48].iter_mut().enumerate() {
        *byte = 0x10 + i as u8;
    }
    let s = GameState::from_bytes_v2(&b).unwrap();
    assert_eq!((s.demo_wins, s.demo_losses, s.prize_min, s.decline_min), (0, 0, 0, 0));
}

#[test]
fn v2_upgrade_maps_alcoholic_to_dependent() {
    let mut b = GAME_GOLDEN;
    b[FLAGS_OFS + 3] = 1;
    assert!(GameState::from_bytes_v2(&b).unwrap().dependent);
}

#[test]
fn v3_rejects_v2_length_and_v2_decoder_rejects_v3() {
    assert!(GameState::from_bytes(&GAME_GOLDEN).is_none());
    let v3 = v3_state().to_bytes();
    let mut as_v2 = [0u8; SAVE_SIZE_V2];
    as_v2.copy_from_slice(&v3[..SAVE_SIZE_V2]);
    assert!(GameState::from_bytes_v2(&as_v2).is_none()); // version byte 3
    let mut v2_with_v3_version = GAME_GOLDEN;
    v2_with_v3_version[0] = 3;
    assert!(GameState::from_bytes_v2(&v2_with_v3_version).is_none());
}

#[test]
fn game_state_rejects_wrong_version_and_length() {
    let good = v3_state().to_bytes();
    let mut bad = good;
    bad[0] = SAVE_FORMAT_VERSION + 1;
    assert!(GameState::from_bytes(&bad).is_none());
    bad[0] = 2;
    assert!(GameState::from_bytes(&bad).is_none());
    bad[0] = 0;
    assert!(GameState::from_bytes(&bad).is_none());

    assert!(GameState::from_bytes(&[]).is_none());
    assert!(GameState::from_bytes(&good[..SAVE_SIZE - 1]).is_none());
    let mut long = good.to_vec();
    long.push(0);
    assert!(GameState::from_bytes(&long).is_none());
}

#[test]
fn game_state_decode_normalises_out_of_range_bytes() {
    let good = v3_state().to_bytes();
    let mut b = good;
    b[PHASE_OFS] = 0x77; // unknown phase -> Gone
    b[ACTION_OFS] = 2; // unused byte -> no action
    b[FLAGS_OFS] = 0x7F; // is_sleeping: any non-zero is true
    b[FLAGS_OFS + 1] = 0x02; // hibernating
    b[V3_FOOD_OFS] = 0x55; // unknown food -> None
    b[V3_DRINK_OFS] = 0x55;
    b[44..48].fill(0xFF); // prize_min, decline_min: held to their limits
    b[38..40].fill(0xFF); // the record counters are not limited
    let s = GameState::from_bytes(&b).unwrap();
    assert_eq!(s.prize_min, bornhack_aegg::game::engine::demo::MINUTES_PER_DAY);
    assert_eq!(s.decline_min, thresholds::BATTLE_DECLINE_MIN());
    assert_eq!(s.demo_wins, u16::MAX);
    assert_eq!(s.phase, Phase::Gone);
    assert_eq!(s.active_action, None);
    assert!(s.is_sleeping);
    assert!(s.hibernating);
    assert_eq!(s.active_food, None);
    assert_eq!(s.active_drink, None);

    // None is written back as 0xFF.
    let mut s = v3_state();
    s.active_action = None;
    s.active_food = None;
    s.active_drink = None;
    let out = s.to_bytes();
    assert_eq!(out[ACTION_OFS], 0xFF);
    assert_eq!(out[V3_FOOD_OFS], 0xFF);
    assert_eq!(out[V3_DRINK_OFS], 0xFF);

    // Phases 0 and 2 decode to Hatching / Leaving and re-encode identically.
    for (byte, phase) in [(0u8, Phase::Hatching), (2, Phase::Leaving)] {
        let mut b = good;
        b[PHASE_OFS] = byte;
        let s = GameState::from_bytes(&b).unwrap();
        assert_eq!(s.phase, phase);
        assert_eq!(s.to_bytes()[PHASE_OFS], byte);
    }
}

#[test]
fn pick_save_prefers_v3() {
    let v3 = v3_state().to_bytes();
    let (s, upgraded) = pick_save(Some(&v3), Some(&GAME_GOLDEN)).expect("v3 present");
    assert!(!upgraded);
    assert_eq!(s, v3_state());
}

#[test]
fn pick_save_upgrades_v2_when_no_v3() {
    let (s, upgraded) = pick_save(None, Some(&GAME_GOLDEN)).expect("v2 present");
    assert!(upgraded);
    assert_eq!(s.phase, Phase::Active);
    assert_eq!(s.money, 0x4968_89AB);
    assert!(s.needs_save(), "an upgraded state is written at once");
}

#[test]
fn pick_save_falls_through_corrupt_v3() {
    let mut bad = v3_state().to_bytes();
    bad[0] = 9;
    let (_, upgraded) = pick_save(Some(&bad), Some(&GAME_GOLDEN)).expect("falls to v2");
    assert!(upgraded);
}

#[test]
fn pick_save_none_when_both_bad() {
    assert!(pick_save(None, None).is_none());
    let short = [3u8; 10];
    assert!(pick_save(Some(&short), Some(&short)).is_none());
    let mut v3 = v3_state().to_bytes();
    v3[0] = 2;
    let mut v2 = GAME_GOLDEN;
    v2[0] = 3;
    assert!(pick_save(Some(&v3), Some(&v2)).is_none());
}

// ---------------------------------------------------------------------------

fn padded<const N: usize>(prefix: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out[..prefix.len()].copy_from_slice(prefix);
    out
}

fn name12(s: &str) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..s.len()].copy_from_slice(s.as_bytes());
    n
}

// ---------------------------------------------------------------------------
// Unicorn Realm
// ---------------------------------------------------------------------------

// Three records; the remaining seven slots are all-zero.
const REALM_PREFIX: [u8; 79] = [
    0x03, 0x02, 0x01, 0x0D, 0x0C, 0x0B, 0x0A, 0x12, 0x11, 0x22, 0x21, 0x32, 0x31, 0x07, 0x5A, 0x6F,
    0x72, 0x70, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x03, 0x1D, 0x1C, 0x1B,
    0x1A, 0x42, 0x41, 0x52, 0x51, 0x62, 0x61, 0x09, 0x42, 0x6F, 0x62, 0x20, 0x74, 0x68, 0x65, 0x20,
    0x50, 0x65, 0x74, 0x00, 0x0B, 0x06, 0x05, 0x2D, 0x2C, 0x2B, 0x2A, 0x72, 0x71, 0x82, 0x81, 0x92,
    0x91, 0x03, 0x51, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
];

fn pet_record(
    generation: u16,
    age_ticks: u32,
    (vitality, curiosity, resilience): (u16, u16, u16),
    kind: u8,
    name: &str,
) -> PetRecord {
    PetRecord {
        generation,
        age_ticks,
        vitality,
        curiosity,
        resilience,
        pet_kind: PetKind::from_u8(kind),
        name: name12(name),
        name_len: name.len() as u8,
    }
}

fn realm_value() -> PetRealm {
    let mut r = PetRealm::new();
    r.pets[0] = pet_record(0x0102, 0x0A0B_0C0D, (0x1112, 0x2122, 0x3132), 7, "Zorp");
    r.pets[1] = pet_record(
        0x0304,
        0x1A1B_1C1D,
        (0x4142, 0x5152, 0x6162),
        9,
        "Bob the Pet",
    );
    r.pets[2] = pet_record(0x0506, 0x2A2B_2C2D, (0x7172, 0x8182, 0x9192), 3, "Q");
    r.count = 3;
    r
}

#[test]
fn realm_golden_bytes_round_trip() {
    assert_eq!(PET_RECORD_SIZE, 26);
    assert_eq!(REALM_SAVE_SIZE, 261);
    let golden: [u8; REALM_SAVE_SIZE] = padded(&REALM_PREFIX);
    assert_eq!(realm_value().to_bytes(), golden);

    let r = PetRealm::from_bytes(&golden);
    assert_eq!(r.count, 3);
    let want = realm_value();
    for i in 0..3 {
        let (a, b) = (&r.pets[i], &want.pets[i]);
        assert_eq!(a.generation, b.generation);
        assert_eq!(a.age_ticks, b.age_ticks);
        assert_eq!(a.vitality, b.vitality);
        assert_eq!(a.curiosity, b.curiosity);
        assert_eq!(a.resilience, b.resilience);
        assert_eq!(a.pet_kind, b.pet_kind);
        assert_eq!(a.name, b.name);
        assert_eq!(a.name_len, b.name_len);
    }
    assert_eq!(r.to_bytes(), golden);
}

#[test]
fn realm_short_and_oversized_input() {
    // Empty -> fresh realm.
    assert_eq!(PetRealm::from_bytes(&[]).count, 0);
    // Count is clamped to REALM_MAX_PETS.
    let mut b: [u8; REALM_SAVE_SIZE] = padded(&REALM_PREFIX);
    b[0] = 200;
    assert_eq!(PetRealm::from_bytes(&b).count as usize, REALM_MAX_PETS);
    // A truncated buffer keeps the count, but a record that does not fit
    // completely stays at its default (zero) value; complete ones still decode.
    let cut = &REALM_PREFIX[..1 + 2 * PET_RECORD_SIZE + 5];
    let r = PetRealm::from_bytes(cut);
    assert_eq!(r.count, 3);
    assert_eq!(r.pets[1].generation, 0x0304);
    assert_eq!(r.pets[2].generation, 0);
    assert_eq!(r.pets[2].name_len, 0);
}

// ---------------------------------------------------------------------------
// Friends
// ---------------------------------------------------------------------------

// Two records; the remaining eighteen slots are all-zero.
const FRIENDS_PREFIX: [u8; 73] = [
    0x02, 0xA1, 0xB2, 0x04, 0x52, 0x65, 0x78, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x03, 0x04, 0x03, 0x02, 0x01, 0x08, 0x07, 0x06, 0x05, 0x0C, 0x0B, 0x0A, 0x09, 0x29, 0x20, 0x17,
    0x72, 0x02, 0x01, 0x04, 0x03, 0xC3, 0xD4, 0x08, 0x46, 0x6C, 0x75, 0x66, 0x66, 0x79, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x06, 0x14, 0x13, 0x12, 0x11, 0x18, 0x17, 0x16, 0x15, 0x1C, 0x1B, 0x1A,
    0x19, 0x37, 0x42, 0x4D, 0x58, 0x12, 0x11, 0x14, 0x13,
];

fn friend(
    device_id: [u8; 2],
    pet_kind: u8,
    name: &str,
    ticks: [u32; 3],
    combat: [u8; 4],
    wl: [u16; 2],
) -> FriendRecord {
    FriendRecord {
        device_id,
        pet_kind,
        name: name12(name),
        name_len: name.len() as u8,
        first_seen_tick: ticks[0],
        last_boost_tick: ticks[1],
        last_seen_tick: ticks[2],
        power: combat[0],
        guard: combat[1],
        speed: combat[2],
        hp: combat[3],
        stamina: 0,
        focus: 0,
        demo: false,
        v3: false,
        fought_tick: 0,
        wins: wl[0],
        losses: wl[1],
        heard_this_boot: false,
    }
}

fn friends_value() -> FriendsList {
    let mut f = FriendsList::new();
    f.friends[0] = friend(
        [0xA1, 0xB2],
        4,
        "Rex",
        [0x0102_0304, 0x0506_0708, 0x090A_0B0C],
        [41, 32, 23, 114],
        [0x0102, 0x0304],
    );
    f.friends[1] = friend(
        [0xC3, 0xD4],
        8,
        "Fluffy",
        [0x1112_1314, 0x1516_1718, 0x191A_1B1C],
        [55, 66, 77, 88],
        [0x1112, 0x1314],
    );
    f.count = 2;
    f
}

/// The old `list` blob decodes through `from_old_bytes`, new fields zero.
#[test]
fn friends_golden_old_blob_migrates() {
    assert_eq!(FRIENDS_SAVE_SIZE_OLD, 721);
    let golden: [u8; FRIENDS_SAVE_SIZE_OLD] = padded(&FRIENDS_PREFIX);
    let f = FriendsList::from_old_bytes(&golden);
    assert_eq!(f.count, 2);
    let want = friends_value();
    for i in 0..2 {
        let (a, b) = (&f.friends[i], &want.friends[i]);
        assert_eq!(a.device_id, b.device_id);
        assert_eq!((a.pet_kind, a.name_len), (b.pet_kind, b.name_len));
        assert_eq!(a.name, b.name);
        assert_eq!(a.first_seen_tick, b.first_seen_tick);
        assert_eq!(a.last_boost_tick, b.last_boost_tick);
        assert_eq!(a.last_seen_tick, b.last_seen_tick);
        assert_eq!((a.power, a.guard, a.speed, a.hp), (b.power, b.guard, b.speed, b.hp));
        assert_eq!((a.wins, a.losses), (b.wins, b.losses));
        assert_eq!((a.stamina, a.focus, a.demo, a.v3, a.fought_tick), (0, 0, false, false, 0));
    }
}

// `list_v3`: layout 3, count 2, two 43-byte records (the old 36 bytes, then
// stamina, focus, flags, fought_tick); the other eighteen slots are zero.
const FRIENDS_V3_PREFIX: [u8; 89] = [
    0x03, 0x02, 0xA1, 0xB2, 0x04, 0x52, 0x65, 0x78, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x03, 0x04, 0x03, 0x02, 0x01, 0x08, 0x07, 0x06, 0x05, 0x0C, 0x0B, 0x0A, 0x09, 0x29, 0x20,
    0x17, 0x72, 0x02, 0x01, 0x04, 0x03, 0x05, 0xFE, 0x01, 0x78, 0x56, 0x34, 0x12, 0xC3, 0xD4, 0x08,
    0x46, 0x6C, 0x75, 0x66, 0x66, 0x79, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x14, 0x13, 0x12,
    0x11, 0x18, 0x17, 0x16, 0x15, 0x1C, 0x1B, 0x1A, 0x19, 0x37, 0x42, 0x4D, 0x58, 0x12, 0x11, 0x14,
    0x13, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00,
];

fn friends_v3_value() -> FriendsList {
    let mut f = friends_value();
    f.friends[0].stamina = 5;
    f.friends[0].focus = -2;
    f.friends[0].demo = true;
    f.friends[0].fought_tick = 0x1234_5678;
    // v3 alone on record 1 (flags 0x02), so the two bits are pinned apart.
    f.friends[1].v3 = true;
    f
}

#[test]
fn friends_golden_bytes_round_trip() {
    assert_eq!(FRIENDS_SAVE_SIZE, 862);
    let golden: [u8; FRIENDS_SAVE_SIZE] = padded(&FRIENDS_V3_PREFIX);
    assert_eq!(friends_v3_value().to_bytes(), golden);

    let f = FriendsList::from_bytes(&golden);
    assert_eq!(f.count, 2);
    let want = friends_v3_value();
    for i in 0..2 {
        let (a, b) = (&f.friends[i], &want.friends[i]);
        assert_eq!(a.device_id, b.device_id);
        assert_eq!(a.pet_kind, b.pet_kind);
        assert_eq!(a.name, b.name);
        assert_eq!(a.name_len, b.name_len);
        assert_eq!(a.first_seen_tick, b.first_seen_tick);
        assert_eq!(a.last_boost_tick, b.last_boost_tick);
        assert_eq!(a.last_seen_tick, b.last_seen_tick);
        assert_eq!(a.power, b.power);
        assert_eq!(a.guard, b.guard);
        assert_eq!(a.speed, b.speed);
        assert_eq!(a.hp, b.hp);
        assert_eq!((a.stamina, a.focus), (b.stamina, b.focus));
        assert_eq!((a.demo, a.v3, a.fought_tick), (b.demo, b.v3, b.fought_tick));
        assert_eq!(a.wins, b.wins);
        assert_eq!(a.losses, b.losses);
    }
    assert_eq!(f.to_bytes(), golden);
}

#[test]
fn friends_short_and_oversized_input() {
    assert_eq!(FriendsList::from_bytes(&[]).count, 0);
    // An old blob (first byte is a count, not layout 3) is not a v3 blob.
    assert_eq!(FriendsList::from_bytes(&FRIENDS_PREFIX).count, 0);
    let mut b: [u8; FRIENDS_SAVE_SIZE] = padded(&FRIENDS_V3_PREFIX);
    b[1] = 250;
    assert_eq!(FriendsList::from_bytes(&b).count as usize, FRIENDS_MAX);
    // Truncated mid second record: first decodes, second stays default.
    let f = FriendsList::from_bytes(&FRIENDS_V3_PREFIX[..2 + 43 + 10]);
    assert_eq!(f.count, 2);
    assert_eq!(f.friends[0].device_id, [0xA1, 0xB2]);
    assert_eq!(f.friends[1].device_id, [0, 0]);
    assert_eq!(f.friends[1].wins, 0);
}

const BEACON_GOLDEN: [u8; 22] = [
    0xE5, 0xF6, 0x06, 0x0B, 0x0A, 0x50, 0x69, 0x78, 0x65, 0x6C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x05, 0x31, 0x42, 0x53, 0x64,
];

const BEACON_V3_GOLDEN: [u8; 25] = [
    0xE5, 0xF6, 0x06, 0x0B, 0x0A, 0x50, 0x69, 0x78, 0x65, 0x6C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x05, 0x31, 0x42, 0x53, 0x64, 0x07, 0xFB, 0x01,
];

#[test]
fn pet_beacon_v3_golden_bytes() {
    let beacon = PetBeacon {
        device_id: [0xE5, 0xF6],
        pet_kind: 6,
        generation: 0x0A0B,
        name: name12("Pixel"),
        name_len: 5,
        power: 0x31,
        guard: 0x42,
        speed: 0x53,
        hp: 0x64,
        stamina: 7,
        focus: -5,
        demo: true,
        v3: true,
    };
    assert_eq!(beacon.to_bytes(), BEACON_V3_GOLDEN);

    let b = PetBeacon::from_bytes(&BEACON_V3_GOLDEN).unwrap();
    assert_eq!(b.device_id, [0xE5, 0xF6]);
    assert_eq!(b.pet_kind, 6);
    assert_eq!(b.generation, 0x0A0B);
    assert_eq!(b.name, name12("Pixel"));
    assert_eq!(b.name_len, 5);
    assert_eq!(
        (b.power, b.guard, b.speed, b.hp),
        (0x31, 0x42, 0x53, 0x64)
    );
    assert_eq!((b.stamina, b.focus, b.demo, b.v3), (7, -5, true, true));
    assert_eq!(b.to_bytes(), BEACON_V3_GOLDEN);

    // Too short -> rejected; longer is tolerated (trailing bytes ignored).
    assert!(PetBeacon::from_bytes(&BEACON_V3_GOLDEN[..21]).is_none());
    assert!(PetBeacon::from_bytes(&[]).is_none());
    let mut long = BEACON_V3_GOLDEN.to_vec();
    long.extend_from_slice(&[0xEE, 0xEE]);
    assert_eq!(PetBeacon::from_bytes(&long).unwrap().to_bytes(), BEACON_V3_GOLDEN);
}

#[test]
fn beacon_v3_starts_with_the_old_beacon() {
    // Old firmware reads the first 22 bytes of a v3 beacon.
    assert_eq!(BEACON_V3_GOLDEN[..22], BEACON_GOLDEN);
}

#[test]
fn pet_beacon_old_still_decodes() {
    let b = PetBeacon::from_bytes(&BEACON_GOLDEN).unwrap();
    assert_eq!(b.device_id, [0xE5, 0xF6]);
    assert_eq!(b.pet_kind, 6);
    assert_eq!(b.generation, 0x0A0B);
    assert_eq!(b.name, name12("Pixel"));
    assert_eq!(
        (b.power, b.guard, b.speed, b.hp),
        (0x31, 0x42, 0x53, 0x64)
    );
    assert_eq!((b.stamina, b.focus, b.demo, b.v3), (3, 0, false, false));
    // 23 and 24 bytes are also an old beacon (the extra bytes are not a v3 tail).
    assert!(!PetBeacon::from_bytes(&BEACON_V3_GOLDEN[..24]).unwrap().v3);
    assert!(PetBeacon::from_bytes(&BEACON_GOLDEN[..21]).is_none());
}

// ---------------------------------------------------------------------------
// Battle result (on-air)
// ---------------------------------------------------------------------------

const BATTLE_GOLDEN: [u8; 9] = [0xAB, 0xCD, 0x12, 0x34, 0x01, 0x57, 0x09, 0x05, 0x0C];

#[test]
fn battle_result_golden_bytes_round_trip() {
    let msg = BattleResultMsg {
        challenger_id: [0xAB, 0xCD],
        target_id: [0x12, 0x34],
        challenger_won: true,
        challenger_hp_pct: 0x57,
        target_hp_pct: 0x09,
        challenger_kind: 5,
        target_kind: 0x0C,
    };
    assert_eq!(msg.to_bytes(), BATTLE_GOLDEN);

    let back = BattleResultMsg::from_bytes(&BATTLE_GOLDEN).unwrap();
    assert_eq!(back.challenger_id, [0xAB, 0xCD]);
    assert_eq!(back.target_id, [0x12, 0x34]);
    assert!(back.challenger_won);
    assert_eq!(back.challenger_hp_pct, 0x57);
    assert_eq!(back.target_hp_pct, 0x09);
    assert_eq!(back.challenger_kind, 5);
    assert_eq!(back.target_kind, 0x0C);
    assert_eq!(back.to_bytes(), BATTLE_GOLDEN);
}

#[test]
fn battle_result_short_packet_tolerance() {
    // Legacy 7-byte packet: species unknown. 8 bytes is still "short".
    for len in [7usize, 8] {
        let back = BattleResultMsg::from_bytes(&BATTLE_GOLDEN[..len]).unwrap();
        assert_eq!(back.challenger_id, [0xAB, 0xCD]);
        assert_eq!(back.target_hp_pct, 0x09);
        assert_eq!(back.challenger_kind, KIND_UNKNOWN);
        assert_eq!(back.target_kind, KIND_UNKNOWN);
    }
    for len in [0usize, 1, 6] {
        assert!(BattleResultMsg::from_bytes(&BATTLE_GOLDEN[..len]).is_none());
    }
    // Trailing bytes ignored; any non-zero "won" byte means true.
    let mut long = BATTLE_GOLDEN.to_vec();
    long[4] = 0x02;
    long.push(0x99);
    let back = BattleResultMsg::from_bytes(&long).unwrap();
    assert!(back.challenger_won);
    assert_eq!(back.target_kind, 0x0C);
    long[4] = 0;
    assert!(!BattleResultMsg::from_bytes(&long).unwrap().challenger_won);
}

#[test]
fn saved_unix_is_zero_for_upgrades_and_old_v3_saves() {
    assert_eq!(v2_decoded().saved_unix, 0);
    let mut bytes = v3_state().to_bytes();
    bytes[V3_SAVED_UNIX_OFS..V3_SAVED_UNIX_OFS + 4].fill(0);
    let back = GameState::from_bytes(&bytes).expect("v3 save must decode");
    assert_eq!(back.saved_unix, 0);
}

#[test]
fn pet_name_prefers_name_v3_and_falls_back_to_the_old_key() {
    use bornhack_aegg::game::engine::{NAME_KEY, NAME_KEY_V2, pick_name};
    assert_eq!(NAME_KEY, "name_v3");
    assert_eq!(NAME_KEY_V2, "name"); // the old firmware's key, read only
    // Both present: v3 wins.
    assert_eq!(pick_name(Some(b"New"), Some(b"Old")), Some(&b"New"[..]));
    // Upgraded pet, no v3 name yet: the old name carries over.
    assert_eq!(pick_name(None, Some(b"Old")), Some(&b"Old"[..]));
    // A present but empty v3 name is a real answer, not "missing".
    assert_eq!(pick_name(Some(b""), Some(b"Old")), Some(&b""[..]));
    assert_eq!(pick_name(None, None), None);
}

// ---------------------------------------------------------------------------
// v3 fight packets (on-air): snapshot, CHALLENGE, ACCEPT, DECLINE
// ---------------------------------------------------------------------------

/// power 40, guard 20, speed 50, hp 80, stamina 6, focus -10, flags 0x800005
/// (bits 0 and 2, demo).
const SNAPSHOT_GOLDEN: [u8; 9] = [0x28, 0x14, 0x32, 0x50, 0x06, 0xF6, 0x05, 0x00, 0x80];
/// Same numbers, flags 0x880201 (bits 0, 9, 19 and demo): all three flag bytes.
const SNAPSHOT_FLAGS_GOLDEN: [u8; 9] = [0x28, 0x14, 0x32, 0x50, 0x06, 0xF6, 0x01, 0x02, 0x88];

fn snap(flags: u32) -> Snapshot {
    Snapshot {
        power: 40,
        guard: 20,
        speed: 50,
        hp: 80,
        stamina: 6,
        focus: -10,
        flags: Flags(flags),
    }
}

#[test]
fn snapshot_golden_bytes() {
    for (g, flags) in [(SNAPSHOT_GOLDEN, 0x80_0005u32), (SNAPSHOT_FLAGS_GOLDEN, 0x88_0201)] {
        assert_eq!(snap(flags).to_wire(), g);
        assert_eq!(Snapshot::from_wire(&g), Some(snap(flags)));
        assert_eq!(Snapshot::from_wire(&g[..g.len() - 1]), None);
    }
}

const CHALLENGE_GOLDEN: [u8; 29] = [
    0x01, 0x02, 0x03, 0x04, 0x28, 0x14, 0x32, 0x50, 0x06, 0xF6, 0x05, 0x00, 0x80, 0xEF, 0xBE, 0x05,
    0x42, 0x72, 0x61, 0x6D, 0, 0, 0, 0, 0, 0, 0, 0, 0x04,
];
const ACCEPT_GOLDEN: [u8; 15] = [
    0x01, 0x02, 0x03, 0x04, 0x28, 0x14, 0x32, 0x50, 0x06, 0xF6, 0x05, 0x00, 0x80, 0x34, 0x12,
];
const DECLINE_GOLDEN: [u8; 7] = [0x01, 0x02, 0x03, 0x04, 0x06, 0x02, 0x01];

#[test]
fn challenge_golden_bytes() {
    let msg = ChallengeMsg {
        challenger_id: [1, 2],
        target_id: [3, 4],
        snap: snap(0x80_0005),
        nonce: 0xBEEF,
        kind: 5,
        name: name12("Bram"),
        name_len: 4,
    };
    assert_eq!(msg.to_bytes(), CHALLENGE_GOLDEN);
    assert_eq!(ChallengeMsg::from_bytes(&CHALLENGE_GOLDEN), Some(msg));
    assert_eq!(ChallengeMsg::from_bytes(&CHALLENGE_GOLDEN[..28]), None);
}

#[test]
fn accept_golden_bytes() {
    let msg = AcceptMsg {
        challenger_id: [1, 2],
        target_id: [3, 4],
        snap: snap(0x80_0005),
        nonce: 0x1234,
    };
    assert_eq!(msg.to_bytes(), ACCEPT_GOLDEN);
    assert_eq!(AcceptMsg::from_bytes(&ACCEPT_GOLDEN), Some(msg));
    assert_eq!(AcceptMsg::from_bytes(&ACCEPT_GOLDEN[..14]), None);
}

#[test]
fn decline_golden_bytes() {
    let msg = DeclineMsg {
        challenger_id: [1, 2],
        target_id: [3, 4],
        why: 6,
        secs: 0x0102,
    };
    assert_eq!(msg.to_bytes(), DECLINE_GOLDEN);
    assert_eq!(DeclineMsg::from_bytes(&DECLINE_GOLDEN), Some(msg));
    assert_eq!(DeclineMsg::from_bytes(&DECLINE_GOLDEN[..6]), None);
    assert_eq!(msg.reason(), Some(Reason::Wait(0x0102)));
}

#[test]
fn decline_new_saturates_a_long_wait() {
    let msg = DeclineMsg::new([1, 2], [3, 4], Reason::Wait(70_000));
    assert_eq!(msg.secs, u16::MAX);
    assert_eq!(msg.why, 6);
    assert_eq!(msg.reason(), Some(Reason::Wait(u16::MAX as u32)));
    let short = DeclineMsg::new([1, 2], [3, 4], Reason::Wait(90));
    assert_eq!(short.secs, 90);
    assert_eq!(short.to_bytes()[5..7], [90, 0]);
    let busy = DeclineMsg::new([1, 2], [3, 4], Reason::Busy);
    assert_eq!((busy.why, busy.secs), (8, 0));
    assert_eq!(busy.reason(), Some(Reason::Busy));
}

#[test]
fn player_decline_has_no_reason() {
    let mut b = DECLINE_GOLDEN;
    b[4] = bornhack_aegg::game::battle::WHY_DECLINED;
    assert_eq!(DeclineMsg::from_bytes(&b).unwrap().reason(), None);
}

#[test]
fn longer_packets_are_tolerated() {
    let mut c = CHALLENGE_GOLDEN.to_vec();
    c.extend_from_slice(&[0xEE, 0xEE]);
    assert_eq!(ChallengeMsg::from_bytes(&c).unwrap().to_bytes(), CHALLENGE_GOLDEN);
    let mut a = ACCEPT_GOLDEN.to_vec();
    a.extend_from_slice(&[0xEE, 0xEE]);
    assert_eq!(AcceptMsg::from_bytes(&a).unwrap().to_bytes(), ACCEPT_GOLDEN);
    let mut d = DECLINE_GOLDEN.to_vec();
    d.extend_from_slice(&[0xEE, 0xEE]);
    assert_eq!(DeclineMsg::from_bytes(&d).unwrap().to_bytes(), DECLINE_GOLDEN);
}

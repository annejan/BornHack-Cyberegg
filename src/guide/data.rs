//! The numbers dump: every tuning value the player's guide cites, for one
//! preset, as JSON (`guide data`). Needs the preset installed first (`dump`
//! does it), because the engine reads its bands from the installed table.

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::json::J;
use crate::game::diary::DIARY_LEN;
use crate::game::engine::debug::DebugPoke;
use crate::game::engine::doses::{self, Reason};
use crate::game::engine::needs::{self, P};
use crate::game::engine::sim;
use crate::game::engine::status::SECOND_WORD_UNDER;
use crate::game::engine::thresholds::{BORNPETS_CFG_KEYS, Mode, Thresholds, badge_preset};
use crate::game::engine::{
    ASPIRINE_HAX_COST, Action, DRUG_HAX_COST, DrinkKind, FoodKind, GameState,
    MEDICATION_HARD_MULT, MINIGAME_HAX_DRAW, MINIGAME_HAX_LOSS, MINIGAME_HAX_REWARD, ONLYPETS_HOBBY_REWARD, PET_NAME_MAX, PLAY_HAX_COST,
    PetKind, Phase, REALM_MAX_PETS, REHAB_HARD_MULT, TICKS_PER_MINUTE,
};
use crate::game::lifecycle::CATCH_UP_MAX_MIN;
use crate::game::pacing::IDLE_FRAME_SECS;

/// How the guide writes a number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Pct,
    Min,
    Ticks,
    Secs,
    Count,
    Hax,
    Raw,
}

impl Unit {
    pub fn name(self) -> &'static str {
        match self {
            Unit::Pct => "pct",
            Unit::Min => "min",
            Unit::Ticks => "ticks",
            Unit::Secs => "secs",
            Unit::Count => "count",
            Unit::Hax => "hax",
            Unit::Raw => "raw",
        }
    }
}

/// Fields whose name has no unit suffix, or whose suffix says the wrong
/// thing. These win over the suffix rules in [`unit_of`]. Each comment says
/// where the unit comes from.
const UNIT_OVERRIDES: &[(&str, Unit)] = &[
    // `game_refusal` shows `Wait(cooldown * 10)` seconds: engine ticks.
    ("MINIGAME_COOLDOWN", Unit::Ticks),
    ("BATTLE_COOLDOWN", Unit::Ticks),
    // advance.rs: `MOOD_STEP * dt` is a level (u16 scale) per minute: no unit.
    ("MOOD_STEP", Unit::Raw),
    // advance.rs: subtracted from `health` on the u16 scale: no unit.
    ("COLLAPSE_DAMAGE", Unit::Raw),
    // engine/mod.rs `new_egg`: trait scores drawn from MIN..MAX, no unit.
    ("MIN_TRAIT", Unit::Raw),
    ("MAX_TRAIT", Unit::Raw),
    // advance.rs compares them with `short_nights` / `good_nights` counters.
    ("DEPRIVED_NIGHTS", Unit::Count),
    ("CLEARING_NIGHTS", Unit::Count),
    // advance.rs: `dt * HANGOVER_ASLEEP_MULT`, a whole-number factor the
    // prose can say ("twice as fast"); beats the `_MULT` rule.
    ("HANGOVER_ASLEEP_MULT", Unit::Count),
    // doses.rs: a number of cups of coffee.
    ("COFFEE_WIRED_CUPS", Unit::Count),
    // demo.rs: the balance the demo pet works up to, a HAX amount.
    ("DEMO_HAX_TARGET", Unit::Hax),
];

/// The unit of a `Thresholds` field. An override wins over a suffix rule;
/// a field with neither is `Raw`, which the guide refuses to print.
pub fn unit_of(field: &str) -> Unit {
    if let Some((_, u)) = UNIT_OVERRIDES.iter().find(|(k, _)| *k == field) {
        return *u;
    }
    if field.ends_with("_PCT") {
        Unit::Pct
    } else if field.ends_with("_MIN") {
        Unit::Min
    } else if field.ends_with("_TICKS") || field.ends_with("_DURATION") {
        Unit::Ticks
    } else if field.ends_with("_PTS") {
        Unit::Count
    } else if field.ends_with("_HAX") {
        Unit::Hax
    } else {
        // `_PM`, `_DIV`, `_MULT` and anything unknown.
        Unit::Raw
    }
}

/// Constants that are not in the table, with their units.
pub fn fixed() -> Vec<(&'static str, u32, Unit)> {
    use crate::game::engine::needs as n;
    let p = |k, v: u8| (k, v as u32, Unit::Pct);
    alloc::vec![
        p("NEED_DANGER_LO_PCT", n::NEED_DANGER_LO_PCT),
        p("NEED_HI_EDGE_PCT", n::NEED_HI_EDGE_PCT),
        p("ENERGY_HI_EDGE_PCT", n::ENERGY_HI_EDGE_PCT),
        p("DIRT_DANGER_PCT", n::DIRT_DANGER_PCT),
        p("HEALTH_DANGER_PCT", n::HEALTH_DANGER_PCT),
        p("HUNGRY_PCT", n::HUNGRY_PCT),
        p("EXHAUSTED_PCT", n::EXHAUSTED_PCT),
        p("TIPSY_PCT", n::TIPSY_PCT),
        p("SORE_PCT", n::SORE_PCT),
        p("HAPPY_MOOD_PCT", n::HAPPY_MOOD_PCT),
        (
            "WATER_REFUSE_PCT",
            doses::WATER_REFUSE_PCT as u32,
            Unit::Pct
        ),
        ("WATER_CAP_PCT", doses::WATER_CAP_PCT as u32, Unit::Pct),
        ("WASH_REFUSE_PCT", doses::WASH_REFUSE_PCT as u32, Unit::Pct),
        (
            "SECOND_WORD_UNDER_PCT",
            (SECOND_WORD_UNDER / P) as u32,
            Unit::Pct
        ),
        (
            "RESTLESS_MIN",
            crate::game::engine::advance::RESTLESS_MIN as u32,
            Unit::Min
        ),
        (
            "ALERT_VISIBLE_SECS",
            crate::game::DIABETES_ALERT_MIN_VISIBLE_MS / 1000,
            Unit::Secs
        ),
        ("TICKS_PER_MINUTE", TICKS_PER_MINUTE, Unit::Count),
        ("PET_NAME_MAX", PET_NAME_MAX as u32, Unit::Count),
        ("REALM_MAX_PETS", REALM_MAX_PETS as u32, Unit::Count),
        ("DIARY_LEN", DIARY_LEN as u32, Unit::Count),
        ("MEDICATION_HARD_MULT", MEDICATION_HARD_MULT, Unit::Count),
        ("REHAB_HARD_MULT", REHAB_HARD_MULT, Unit::Count),
        (
            "PETS_CFG_NAME_CAP",
            crate::game::pet_registry::NAME_CAP as u32,
            Unit::Count
        ),
        (
            "COMFORT_PCT",
            crate::game::engine::debug::COMFORT_PCT as u32,
            Unit::Pct
        ),
        ("CATCH_UP_MAX_MIN", CATCH_UP_MAX_MIN, Unit::Min),
        (
            "STATION_COOLDOWN",
            crate::game::station::COOLDOWN_TICKS,
            Unit::Ticks
        ),
        (
            "FRIEND_BOOST_COOLDOWN",
            crate::game::friends::FRIEND_BOOST_COOLDOWN_TICKS,
            Unit::Ticks
        ),
        (
            "FRIENDS_MAX",
            crate::game::friends::FRIENDS_MAX as u32,
            Unit::Count
        ),
        (
            "STATION_FOOD_FLOOR_PCT",
            crate::game::engine::STATION_FOOD_FLOOR_PCT as u32,
            Unit::Pct
        ),
        (
            "STATION_BOOZE_CAP_PCT",
            crate::game::engine::STATION_BOOZE_CAP_PCT as u32,
            Unit::Pct
        ),
        ("IDLE_FRAME_SECS", IDLE_FRAME_SECS as u32, Unit::Secs),
        ("MINIGAME_HAX_REWARD", MINIGAME_HAX_REWARD, Unit::Hax),
        ("MINIGAME_HAX_DRAW", MINIGAME_HAX_DRAW, Unit::Hax),
        ("MINIGAME_HAX_LOSS", MINIGAME_HAX_LOSS, Unit::Hax),
        ("ONLYPETS_HOBBY_REWARD", ONLYPETS_HOBBY_REWARD, Unit::Hax),
        ("PLAY_HAX_COST", PLAY_HAX_COST, Unit::Hax),
        ("DRUG_HAX_COST", DRUG_HAX_COST, Unit::Hax),
        ("ASPIRINE_HAX_COST", ASPIRINE_HAX_COST, Unit::Hax),
        (
            "START_HAX",
            GameState::new_egg(1, PetKind(0)).money,
            Unit::Hax
        ),
    ]
}

const ACTIONS: [Action; 9] = [
    Action::Heal,
    Action::Play,
    Action::Medicate,
    Action::Ozempic,
    Action::Rehab,
    Action::Exercise,
    Action::Wash,
    Action::Relax,
    Action::OnlyPets,
];

/// `format!("{a:?}")` of an action, as a static string.
fn action_name(a: Action) -> &'static str {
    match a {
        Action::Heal => "Heal",
        Action::Play => "Play",
        Action::Medicate => "Medicate",
        Action::Ozempic => "Ozempic",
        Action::Rehab => "Rehab",
        Action::Exercise => "Exercise",
        Action::Wash => "Wash",
        Action::Relax => "Relax",
        Action::OnlyPets => "OnlyPets",
        Action::Feed => "Feed",
        Action::Drink => "Drink",
    }
}

/// Every priced item: foods, drinks, then the actions. The key is the menu
/// label (food, drink) or the action's name.
fn item_list() -> Vec<(&'static str, Action, Option<FoodKind>, Option<DrinkKind>)> {
    let mut v = Vec::new();
    for f in FoodKind::ALL {
        v.push((f.label(), Action::Feed, Some(f), None));
    }
    for d in DrinkKind::ALL {
        v.push((d.label(), Action::Drink, None, Some(d)));
    }
    for a in ACTIONS {
        v.push((action_name(a), a, None, None));
    }
    v
}

/// HAX price of every item, `None` for a free one.
pub fn prices(hard: bool) -> Vec<(&'static str, Option<u32>)> {
    item_list()
        .into_iter()
        .map(|(k, a, f, d)| (k, doses::price(a, f, d, hard)))
        .collect()
}

/// The pet the numbers are measured on: hatched, awake, every need mid-band.
fn reference() -> GameState {
    let mut s = GameState::new_egg(1, PetKind(0));
    s.phase = Phase::Active;
    s.hatching_countdown = 0;
    s.is_sleeping = false;
    let _ = s.debug_poke(DebugPoke::Comfort);
    s
}

/// Minutes (from the reference pet) until each word first applies.
pub fn until() -> Vec<(&'static str, Option<u32>)> {
    type Pred = fn(&GameState) -> bool;
    let words: [(&'static str, Pred); 5] = [
        ("peckish", |s| (s.fullness as i32) < needs::full_lo()),
        ("thirsty", |s| (s.hydration as i32) < needs::water_lo()),
        ("sleepy", |s| (s.energy as i32) < needs::energy_lo()),
        ("bored", |s| (s.fun as i32) < needs::fun_lo()),
        ("grubby", |s| (s.dirt as i32) > needs::dirt_hi()),
    ];
    let mut found: Vec<Option<u32>> = alloc::vec![None; words.len()];
    let mut s = reference();
    for m in 1..=4320u32 {
        s.catch_up(1);
        for (i, (_, pred)) in words.iter().enumerate() {
            if found[i].is_none() && pred(&s) {
                found[i] = Some(m);
            }
        }
    }
    words.iter().map(|(k, _)| *k).zip(found).collect()
}

/// Minutes a tired reference pet sleeps before it wakes by itself.
pub fn night_min() -> Option<u32> {
    let mut s = reference();
    s.energy = (needs::energy_lo() - P) as u16;
    doses::fall_asleep(&mut s).ok()?;
    for m in 1..=1440u32 {
        s.catch_up(1);
        if !s.is_sleeping {
            return Some(m);
        }
    }
    None
}

fn setup(a: Action, food: Option<FoodKind>, drink: Option<DrinkKind>) -> Option<DebugPoke> {
    match a {
        Action::Feed => food.map(|_| DebugPoke::Hungry),
        Action::Drink => drink.map(|_| DebugPoke::Thirsty),
        Action::Wash => Some(DebugPoke::Dirty),
        Action::Heal => Some(DebugPoke::Injured),
        Action::Medicate => Some(DebugPoke::Diabetic),
        Action::Ozempic => Some(DebugPoke::Fat),
        Action::Rehab => Some(DebugPoke::Dependent),
        Action::Play | Action::Relax | Action::Exercise | Action::OnlyPets => None,
    }
}

type Fields = Vec<(&'static str, i32)>;

/// What one dose does to the reference pet: the changed levels in tenths of
/// a percent, the changed timers as their value after; or why it is refused.
pub fn doses() -> Vec<(&'static str, Result<Fields, String>)> {
    type Level = (&'static str, fn(&GameState) -> i32);
    type Timer = (&'static str, fn(&GameState) -> i32);
    let levels: [Level; 11] = [
        ("full", |s| s.fullness as i32),
        ("water", |s| s.hydration as i32),
        ("energy", |s| s.energy as i32),
        ("fun", |s| s.fun as i32),
        ("dirt", |s| s.dirt as i32),
        ("booze", |s| s.booze as i32),
        ("weight", |s| s.weight as i32),
        ("fit", |s| s.fitness as i32),
        ("strain", |s| s.strain as i32),
        ("mood", |s| s.mood as i32),
        ("health", |s| s.health as i32),
    ];
    let timers: [Timer; 8] = [
        ("injured_min", |s| s.injured_min as i32),
        ("hangover_min", |s| s.hangover_min as i32),
        ("groggy_min", |s| s.groggy_min as i32),
        ("insulin_min", |s| s.insulin_min as i32),
        ("rehab_min", |s| s.rehab_min as i32),
        ("ozempic_cd_min", |s| s.ozempic_cd_min as i32),
        ("med_cd_min", |s| s.med_cd_min as i32),
        ("caffeine_min", |s| s.caffeine_min as i32),
    ];
    let mut out = Vec::new();
    for (key, a, food, drink) in item_list() {
        let mut s = reference();
        if let Some(p) = setup(a, food, drink) {
            let _ = s.debug_poke(p);
        }
        s.active_food = food;
        s.active_drink = drink;
        let before = s.clone();
        match doses::apply(&mut s, a) {
            Err(r) => out.push((key, Err(Reason::Refused(r).text().as_str().to_string()))),
            Ok(()) => {
                let mut f: Fields = Vec::new();
                for (name, get) in levels {
                    let d = get(&s) - get(&before);
                    if d != 0 {
                        f.push((name, d * 10 / P));
                    }
                }
                for (name, get) in timers {
                    if get(&s) != get(&before) {
                        f.push((name, get(&s)));
                    }
                }
                out.push((key, Ok(f)));
            }
        }
    }
    out
}

fn obj(v: Vec<(&str, J)>) -> J {
    J::Obj(v.into_iter().map(|(k, j)| (k.to_string(), j)).collect())
}

fn opt(v: Option<u32>) -> J {
    v.map_or(J::Null, |n| J::Int(n as i64))
}

fn price_obj(hard: bool) -> J {
    obj(prices(hard).into_iter().map(|(k, p)| (k, opt(p))).collect())
}

/// The whole dump for one preset. Installs the badge's preset for this
/// process first (a no-op after the first call).
pub fn dump(mode: Mode) -> J {
    let t = badge_preset(mode);
    sim::install_preset(t, mode, false);
    let mut values = Vec::new();
    let mut units = Vec::new();
    for k in Thresholds::FIELDS {
        values.push((*k, J::Int(t.value_of(k).expect("field") as i64)));
        units.push((*k, J::Str(unit_of(k).name().to_string())));
    }
    for (k, v, u) in fixed() {
        values.push((k, J::Int(v as i64)));
        units.push((k, J::Str(u.name().to_string())));
    }
    let cfg_keys = BORNPETS_CFG_KEYS
        .iter()
        .map(|(k, _)| J::Str(k.to_string()))
        .collect();
    let doses = doses()
        .into_iter()
        .map(|(k, r)| match r {
            Ok(f) => (
                k,
                obj(f.into_iter().map(|(n, x)| (n, J::Int(x as i64))).collect()),
            ),
            Err(why) => (k, obj(alloc::vec![("refused", J::Str(why))])),
        })
        .collect();
    obj(alloc::vec![
        ("preset", J::Str(mode.label().to_string())),
        ("values", obj(values)),
        ("units", obj(units)),
        ("cfg_keys", J::Arr(cfg_keys)),
        (
            "prices",
            obj(alloc::vec![
                ("normal", price_obj(false)),
                ("hard", price_obj(true))
            ]),
        ),
        (
            "until",
            obj(until().into_iter().map(|(k, m)| (k, opt(m))).collect()),
        ),
        ("night_min", opt(night_min())),
        ("doses", obj(doses)),
    ])
}

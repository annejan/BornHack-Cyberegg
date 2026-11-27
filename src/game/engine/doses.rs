//! Player actions as doses.
//!
//! An action's effect lands in one piece when its animation ends
//! ([`apply`]); no time passes inside a dose, the needs only age in
//! `update`'s segments. Refusals are decided by [`refusal`], which the
//! starters call before waking or charging and `apply` calls again, so the
//! rule exists once. Every dose number is a `Thresholds` field.

use super::causes::{Cause, StrainWhy};
use super::needs::{self, MAX, P};
use super::thresholds as t;
use super::{Action, DrinkKind, FoodKind, GameState};

/// Why an action does nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Refusal {
    NotThirsty,
    AlreadyClean,
    Injured,
    Asleep,
    NoNeed,
    Tired,
    /// Demo mode needs a pet in fair shape first.
    FixMeFirst,
    /// Health under `BATTLE_MIN_HEALTH_PCT`.
    TooWeak,
    /// Felt drink over `BATTLE_DRUNK_PCT`.
    TooDrunk,
    /// A demo pet and a player friend.
    DemoOnly,
    /// A player pet and a demo friend.
    NoDemo,
}

/// Why a menu row is disabled or an action was turned away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum Reason {
    Refused(Refusal),
    /// Still waiting, seconds left.
    Wait(u32),
    CantAfford,
    Busy,
    Hibernating,
    /// Still hatching.
    Egg,
    /// The pet has left.
    Gone,
}

impl Refusal {
    /// What the player is told.
    pub fn text(self) -> &'static str {
        match self {
            Refusal::NotThirsty => "not thirsty",
            Refusal::AlreadyClean => "already clean",
            Refusal::Injured => "injured",
            Refusal::Asleep => "asleep",
            Refusal::NoNeed => "no need",
            Refusal::Tired => "too tired",
            Refusal::FixMeFirst => "fix me first",
            Refusal::TooWeak => "too weak",
            Refusal::TooDrunk => "too drunk",
            Refusal::DemoOnly => "demo pets only",
            Refusal::NoDemo => "no demo pets",
        }
    }
}

impl Reason {
    /// The one place a reason becomes player-facing text.
    pub fn text(self) -> heapless::String<16> {
        let mut out = heapless::String::new();
        let _ = match self {
            Reason::Refused(r) => out.push_str(r.text()),
            Reason::Wait(secs) => {
                let _ = out.push_str("wait ");
                out.push_str(fmt_wait(secs).as_str())
            }
            Reason::CantAfford => out.push_str("can't afford"),
            Reason::Busy => out.push_str("busy"),
            Reason::Hibernating => out.push_str("hibernating"),
            Reason::Egg => out.push_str("still an egg"),
            Reason::Gone => out.push_str("pet is gone"),
        };
        out
    }
}

impl Reason {
    /// A (kind, seconds) pair so a toast can carry it in two atomics.
    pub fn pack(self) -> (u8, u32) {
        let (kind, secs) = match self {
            Reason::Refused(Refusal::NotThirsty) => (0, 0),
            Reason::Refused(Refusal::AlreadyClean) => (1, 0),
            Reason::Refused(Refusal::Injured) => (2, 0),
            Reason::Refused(Refusal::Asleep) => (3, 0),
            Reason::Refused(Refusal::NoNeed) => (4, 0),
            Reason::Refused(Refusal::Tired) => (5, 0),
            Reason::Refused(Refusal::FixMeFirst) => (12, 0),
            Reason::Refused(Refusal::TooWeak) => (13, 0),
            Reason::Refused(Refusal::TooDrunk) => (14, 0),
            Reason::Refused(Refusal::DemoOnly) => (15, 0),
            Reason::Refused(Refusal::NoDemo) => (16, 0),
            Reason::Wait(secs) => (6, secs),
            Reason::CantAfford => (7, 0),
            Reason::Busy => (8, 0),
            Reason::Hibernating => (9, 0),
            Reason::Egg => (10, 0),
            Reason::Gone => (11, 0),
        };
        (kind, secs)
    }

    /// Inverse of [`pack`](Self::pack); anything unknown reads as "no need".
    pub fn unpack(kind: u8, secs: u32) -> Self {
        match kind {
            0 => Reason::Refused(Refusal::NotThirsty),
            1 => Reason::Refused(Refusal::AlreadyClean),
            2 => Reason::Refused(Refusal::Injured),
            3 => Reason::Refused(Refusal::Asleep),
            5 => Reason::Refused(Refusal::Tired),
            6 => Reason::Wait(secs),
            7 => Reason::CantAfford,
            8 => Reason::Busy,
            9 => Reason::Hibernating,
            10 => Reason::Egg,
            11 => Reason::Gone,
            12 => Reason::Refused(Refusal::FixMeFirst),
            13 => Reason::Refused(Refusal::TooWeak),
            14 => Reason::Refused(Refusal::TooDrunk),
            15 => Reason::Refused(Refusal::DemoOnly),
            16 => Reason::Refused(Refusal::NoDemo),
            _ => Reason::Refused(Refusal::NoNeed),
        }
    }
}

/// The HAX price of an action in the current mode, `None` for a free one. The
/// one price table: the starters charge it, the menu shows it and the hint
/// checks it. `food` / `drink` name the choice for Feed / Drink. Healthy food
/// and water cost more than junk; hard (US) mode raises healthy food, the
/// medications and rehab.
pub fn price(
    a: Action,
    food: Option<FoodKind>,
    drink: Option<DrinkKind>,
    hard: bool,
) -> Option<u32> {
    match a {
        Action::Feed => food.map(|f| match f {
            FoodKind::Salad | FoodKind::Apple => {
                if hard {
                    20
                } else {
                    15
                }
            }
            FoodKind::Frikandel | FoodKind::Pizza | FoodKind::Cake => 10,
        }),
        Action::Drink => drink.map(|d| match d {
            DrinkKind::Water => 15,
            DrinkKind::Coffee | DrinkKind::Beer | DrinkKind::Wine | DrinkKind::Whiskey => 10,
        }),
        Action::Heal => Some(super::ASPIRINE_HAX_COST),
        Action::Play => Some(super::PLAY_HAX_COST),
        Action::Medicate | Action::Ozempic => Some(super::medication_price(hard)),
        Action::Rehab => Some(super::rehab_price(hard)),
        Action::Exercise | Action::Wash | Action::Relax | Action::OnlyPets => None,
    }
}

/// A wait of `secs` seconds as "45s", "59m" or "2h05" (the longest, 72 h,
/// is "72h00").
pub fn fmt_wait(secs: u32) -> heapless::String<8> {
    let mut out = heapless::String::new();
    let _ = if secs < 60 {
        core::fmt::Write::write_fmt(&mut out, format_args!("{secs}s"))
    } else if secs < 3600 {
        core::fmt::Write::write_fmt(&mut out, format_args!("{}m", secs / 60))
    } else {
        core::fmt::Write::write_fmt(
            &mut out,
            format_args!("{}h{:02}", secs / 3600, secs % 3600 / 60),
        )
    };
    out
}

/// Water is refused above this hydration and never takes it past `WATER_CAP_PCT`
/// (fixed safety edges of the hydration need, see `needs::build_edges`).
pub const WATER_REFUSE_PCT: i32 = 85;
pub const WATER_CAP_PCT: i32 = 90;
/// Wash is refused below this dirt level (fixed safety edge of the dirt need).
pub const WASH_REFUSE_PCT: i32 = 25;

/// A drink gives less than this share (percent) of its mood and energy to a
/// pet this drunk: it is barely felt.
const DULLED_DIM_PCT: i32 = 50;

/// A level shifted by `delta`, clamped to the level range.
fn bump(level: &mut u16, delta: i32) {
    *level = (*level as i32 + delta).clamp(0, MAX) as u16;
}

/// Percent points as a level delta.
fn lvl(pct: u8) -> i32 {
    pct as i32 * P
}

/// Whether `a` (with the chosen `drink`, for `Action::Drink`) would be refused.
/// Medical preconditions and cooldowns that are not met are `NoNeed`; a pet
/// too tired to work is `Tired`, an injured one `Injured`.
pub fn refusal(s: &GameState, a: Action, drink: Option<DrinkKind>) -> Option<Refusal> {
    let injured = s.injured_min > 0;
    match (a, drink) {
        (Action::Drink, Some(DrinkKind::Water)) if s.hydration as i32 > WATER_REFUSE_PCT * P => {
            Some(Refusal::NotThirsty)
        }
        (Action::Play | Action::Exercise, _) if injured => Some(Refusal::Injured),
        (Action::Wash, _) if (s.dirt as i32) < WASH_REFUSE_PCT * P => Some(Refusal::AlreadyClean),
        (Action::OnlyPets, _) if s.is_sleeping => Some(Refusal::Asleep),
        (Action::OnlyPets, _) if injured => Some(Refusal::Injured),
        (Action::OnlyPets, _) if (s.energy as i32) < needs::energy_lo() => Some(Refusal::Tired),
        (Action::Heal, _) if s.med_cd_min > 0 => Some(Refusal::NoNeed),
        (Action::Medicate, _) if !s.diabetic || s.insulin_min > t::DOSE_INSULIN_WINDOW_MIN() => {
            Some(Refusal::NoNeed)
        }
        (Action::Ozempic, _) if s.ozempic_cd_min > 0 => Some(Refusal::NoNeed),
        (Action::Rehab, _) if !s.dependent || s.rehab_min > 0 => Some(Refusal::NoNeed),
        _ => None,
    }
}

/// Apply the dose of the action that just ended.
pub fn apply(s: &mut GameState, a: Action) -> Result<(), Refusal> {
    if let Some(r) = refusal(s, a, s.active_drink) {
        return Err(r);
    }
    match a {
        Action::Feed => {
            if let Some(food) = s.active_food {
                eat(s, food);
            }
        }
        Action::Drink => {
            if let Some(drink) = s.active_drink {
                sip(s, drink);
            }
        }
        Action::Play => play(s),
        Action::Exercise => exercise(s),
        Action::Wash => bump(&mut s.dirt, -lvl(t::DOSE_WASH_DIRT_PCT())),
        Action::Relax => relax(s),
        Action::Heal => heal(s),
        Action::Medicate => s.insulin_min = t::DOSE_INSULIN_MIN(),
        Action::Ozempic => ozempic(s),
        Action::Rehab => s.rehab_min = t::DOSE_REHAB_MIN(),
        Action::OnlyPets => work(s),
    }
    Ok(())
}

/// Tired pets get twice the fun out of play (and mini-games).
fn fun_for(s: &GameState, pct: i32) -> i32 {
    let mult = if (s.energy as i32) < needs::energy_lo() {
        t::DOSE_TIRED_FUN_MULT() as i32
    } else {
        1
    };
    pct * P * mult
}

fn play(s: &mut GameState) {
    let fun = fun_for(s, t::DOSE_PLAY_FUN_PCT() as i32);
    bump(&mut s.fun, fun);
    bump(&mut s.energy, -lvl(t::DOSE_PLAY_ENERGY_PCT()));
    bump(&mut s.fullness, -lvl(t::DOSE_PLAY_FULL_PCT()));
    bump(&mut s.hydration, -lvl(t::DOSE_PLAY_WATER_PCT()));
    bump(&mut s.fitness, lvl(t::DOSE_PLAY_FIT_PCT()));
    bump(&mut s.dirt, lvl(t::DOSE_PLAY_DIRT_PCT()));
    s.groggy_min = s.groggy_min.saturating_sub(t::DOSE_PLAY_GROGGY_MIN());
}

/// A finished mini-game of grade `grade` (0..=2).
pub fn minigame(s: &mut GameState, grade: u8) {
    let pct =
        t::DOSE_MINI_FUN_BASE_PCT() as i32 + t::DOSE_MINI_FUN_GRADE_PCT() as i32 * grade as i32;
    let fun = fun_for(s, pct);
    bump(&mut s.fun, fun);
    bump(&mut s.energy, -lvl(t::DOSE_MINI_ENERGY_PCT()));
}

/// Meeting a mesh friend: a new one lifts the fun a lot, a known one a little.
/// Like every fun gain it is doubled while the pet is tired (spec rule 7),
/// judged on the state before the dose. No energy cost.
pub fn friend(s: &mut GameState, big: bool) {
    let pct = if big {
        t::DOSE_FRIEND_BIG_FUN_PCT()
    } else {
        t::DOSE_FRIEND_SMALL_FUN_PCT()
    };
    let fun = fun_for(s, pct as i32);
    bump(&mut s.fun, fun);
}

fn exercise(s: &mut GameState) {
    // Every multiplier looks at the pet as it was before the session.
    // i64 so absurd CFG multipliers cannot overflow; clamped before use.
    let mut strain = lvl(t::DOSE_EXERCISE_STRAIN_PCT()) as i64;
    let scale = |strain: i64, pct: u16| strain * pct as i64 / 100;
    // The session's handicaps: each feeds its multiplier and names the blame.
    let hungry = (s.fullness as i32) < needs::full_lo();
    let thirsty = (s.hydration as i32) < needs::water_lo();
    let tired = (s.energy as i32) < needs::energy_lo();
    let heavy = s.weight as i32 > needs::weight_hi();
    let blame = if hungry {
        Some(StrainWhy::Hungry)
    } else if thirsty {
        Some(StrainWhy::Thirsty)
    } else if tired {
        Some(StrainWhy::Tired)
    } else if heavy {
        Some(StrainWhy::Heavy)
    } else if s.deprived {
        Some(StrainWhy::WornOut)
    } else {
        None
    };
    if hungry || tired || thirsty {
        strain = scale(strain, t::DOSE_STRAIN_NEED_PCT());
    }
    if s.rested_min > 0 {
        strain = scale(strain, t::DOSE_STRAIN_RESTED_PCT());
    }
    if s.fitness as i32 >= lvl(t::DOSE_STRAIN_FIT_MIN_PCT()) {
        strain = scale(strain, t::DOSE_STRAIN_FIT_PCT());
    }
    if heavy {
        strain = scale(strain, t::DOSE_STRAIN_HEAVY_PCT());
    }
    if s.deprived {
        strain = scale(strain, t::DOSE_STRAIN_DEPRIVED_PCT());
    }
    let strain = strain.min(MAX as i64) as i32;
    bump(&mut s.strain, strain);
    bump(&mut s.energy, -lvl(t::DOSE_EXERCISE_ENERGY_PCT()));
    bump(&mut s.fullness, -lvl(t::DOSE_EXERCISE_FULL_PCT()));
    bump(&mut s.hydration, -lvl(t::DOSE_EXERCISE_WATER_PCT()));
    bump(&mut s.fun, lvl(t::DOSE_EXERCISE_FUN_PCT()));
    bump(&mut s.booze, -lvl(t::DOSE_EXERCISE_BOOZE_PCT()));
    bump(&mut s.dirt, lvl(t::DOSE_EXERCISE_DIRT_PCT()));
    bump(&mut s.weight, -lvl(t::DOSE_EXERCISE_WEIGHT_PCT()));
    s.hangover_min = s
        .hangover_min
        .saturating_sub(t::DOSE_EXERCISE_HANGOVER_MIN());
    s.groggy_min = 0;
    if !tear_if_over(s, blame.unwrap_or(StrainWhy::Overdone)) {
        bump(&mut s.fitness, lvl(t::DOSE_EXERCISE_FIT_PCT()));
        if let Some(why) = blame {
            s.causes.push(Cause::Strained(why));
        }
    }
}

/// The injury rule of every strain source: a strain over the tear line
/// injures (`DOSE_INJURY_MIN`, `DOSE_INJURY_HEALTH_PCT`, held at the demo
/// floor) and names `why`. Returns whether it tore.
fn tear_if_over(s: &mut GameState, why: StrainWhy) -> bool {
    if s.strain as i32 <= needs::strain_inj() {
        return false;
    }
    s.injured_min = t::DOSE_INJURY_MIN();
    let before = s.health;
    bump(&mut s.health, -lvl(t::DOSE_INJURY_HEALTH_PCT()));
    if s.demo.on {
        s.health = super::demo::floor(before, s.health, super::demo::health_floor());
    }
    s.causes.push(Cause::TornMuscle(why));
    true
}

/// One fight's costs (spec 12.2), the same on both badges. The handicaps
/// look at the pet as it was before the fight; `hp_left_pct` is the pet's
/// own HP left at the end.
pub(super) fn battle(s: &mut GameState, won: bool, hp_left_pct: u8) {
    let mut strain = lvl(t::BATTLE_STRAIN_PCT()) as i64;
    let scale = |strain: i64, pct: u16| strain * pct as i64 / 100;
    if (s.fullness as i32) < needs::full_lo()
        || (s.hydration as i32) < needs::water_lo()
        || (s.energy as i32) < needs::energy_lo()
    {
        strain = scale(strain, t::BATTLE_STRAIN_NEED_PCT());
    }
    if s.rested_min > 0 {
        strain = scale(strain, t::BATTLE_STRAIN_RESTED_PCT());
    }
    bump(&mut s.strain, strain.min(MAX as i64) as i32);
    bump(&mut s.hydration, -lvl(t::BATTLE_WATER_PCT()));
    bump(&mut s.energy, -lvl(t::BATTLE_ENERGY_PCT()));
    bump(&mut s.fun, lvl(t::BATTLE_FUN_PCT()));
    bump(&mut s.dirt, lvl(t::BATTLE_DIRT_PCT()));
    let mood = lvl(t::BATTLE_MOOD_PCT());
    bump(&mut s.mood, if won { mood } else { -mood });
    // Bruises: up to BATTLE_BRUISE_MAX_PCT at 0 % HP left.
    let lost_hp = 100 - hp_left_pct.min(100) as i32;
    let before = s.health;
    bump(&mut s.health, -(lost_hp * t::BATTLE_BRUISE_MAX_PCT() as i32 * P / 100));
    if s.demo.on {
        s.health = super::demo::floor(before, s.health, super::demo::health_floor());
    }
    if !tear_if_over(s, StrainWhy::Fight) && s.strain as i32 > needs::SORE_PCT as i32 * P {
        s.causes.push(Cause::Strained(StrainWhy::Fight));
    }
}

/// A dose that takes fitness: held at the demo floor while demo is on.
fn lower_fitness(s: &mut GameState, amount: i32) {
    let before = s.fitness;
    bump(&mut s.fitness, -amount);
    s.fitness = super::demo::fit_after_fall(s, before, s.fitness);
}

fn relax(s: &mut GameState) {
    let fun = s.fun as i32;
    bump(&mut s.fun, -lvl(t::DOSE_RELAX_FUN_PCT()));
    if fun >= needs::fun_lo() && (s.fun as i32) < needs::fun_lo() {
        s.causes.push(Cause::OverRelaxed);
    }
    bump(&mut s.strain, -lvl(t::DOSE_RELAX_STRAIN_PCT()));
    bump(&mut s.energy, lvl(t::DOSE_RELAX_ENERGY_PCT()));
    lower_fitness(s, lvl(t::DOSE_RELAX_FIT_PCT()));
}

/// Medicine: halves an injury (and numbs: booze), does nothing but numb a
/// healthy pet, mends a sick one.
fn heal(s: &mut GameState) {
    s.med_cd_min = t::DOSE_MED_CD_MIN();
    if s.injured_min > 0 {
        s.injured_min = s.injured_min.saturating_sub(t::DOSE_HEAL_INJURY_MIN());
        bump(&mut s.booze, lvl(t::DOSE_MED_BOOZE_PCT()));
    } else if s.health as i32 > lvl(t::DOSE_HEAL_HEALTHY_PCT()) {
        bump(&mut s.booze, lvl(t::DOSE_MED_BOOZE_PCT()));
        s.causes.push(Cause::OnlyNumbed);
    } else {
        bump(&mut s.health, lvl(t::DOSE_HEAL_HEALTH_PCT()));
    }
}

fn ozempic(s: &mut GameState) {
    bump(&mut s.weight, -lvl(t::DOSE_OZEMPIC_WEIGHT_PCT()));
    lower_fitness(s, lvl(t::DOSE_OZEMPIC_FIT_PCT()));
    s.ozempic_min = t::DOSE_OZEMPIC_MIN();
    s.ozempic_cd_min = t::DOSE_OZEMPIC_CD_MIN();
}

/// "Only pets": the pet works for HAX (plan 6 replaces the flat pay).
fn work(s: &mut GameState) {
    bump(&mut s.energy, -lvl(t::DOSE_WORK_ENERGY_PCT()));
    bump(&mut s.fun, lvl(t::DOSE_WORK_FUN_PCT()));
    bump(&mut s.mood, -lvl(t::DOSE_WORK_MOOD_PCT()));
    if s.money_live() {
        s.add_money(super::ONLYPETS_HOBBY_REWARD);
    }
}

/// Lie down. `night` is decided by how tired the pet is now: a worn-out pet
/// starts a night, a rested one a nap.
pub fn fall_asleep(s: &mut GameState) -> Result<(), Refusal> {
    if s.is_sleeping {
        return Err(Refusal::Asleep);
    }
    s.night = (s.energy as i32) < lvl(t::DOSE_NIGHT_ENERGY_PCT());
    // The awake reset of `advance`, for a pet that goes back to bed before
    // a minute awake has passed: below the line its oversleep is over, else
    // the next minute asleep wakes it (groggy) again, every minute.
    if (s.energy as i32) < lvl(t::OVERSLEEP_RESET_PCT()) {
        s.oversleep_min = 0;
    }
    s.is_sleeping = true;
    s.demo.clock_armed = false;
    s.sleep_min = 0;
    s.good_sleep_min = 0;
    s.bed_why = super::regime::unsettled_by(s);
    Ok(())
}

/// The player wakes the pet (grumpy under the engine's grumpy line).
pub fn wake_up(s: &mut GameState) -> Result<(), Refusal> {
    if !s.is_sleeping {
        return Err(Refusal::NoNeed);
    }
    super::advance::wake(s, !s.self_care);
    Ok(())
}

/// What one food does, from the thresholds.
struct Meal {
    full: u8,
    dirt: u8,
    weight_pm: u16,
    mood: u8,
    healthy: bool,
}

fn meal(food: FoodKind) -> Meal {
    let (full, dirt, weight_pm, mood, healthy) = match food {
        FoodKind::Salad => (
            t::DOSE_SALAD_FULL_PCT(),
            t::DOSE_SALAD_DIRT_PCT(),
            t::DOSE_SALAD_WEIGHT_PM(),
            0,
            true,
        ),
        FoodKind::Apple => (
            t::DOSE_APPLE_FULL_PCT(),
            t::DOSE_APPLE_DIRT_PCT(),
            t::DOSE_APPLE_WEIGHT_PM(),
            0,
            true,
        ),
        FoodKind::Frikandel => (
            t::DOSE_FRIKANDEL_FULL_PCT(),
            t::DOSE_FRIKANDEL_DIRT_PCT(),
            t::DOSE_FRIKANDEL_WEIGHT_PM(),
            t::DOSE_FRIKANDEL_MOOD_PCT(),
            false,
        ),
        FoodKind::Pizza => (
            t::DOSE_PIZZA_FULL_PCT(),
            t::DOSE_PIZZA_DIRT_PCT(),
            t::DOSE_PIZZA_WEIGHT_PM(),
            t::DOSE_PIZZA_MOOD_PCT(),
            false,
        ),
        FoodKind::Cake => (
            t::DOSE_CAKE_FULL_PCT(),
            t::DOSE_CAKE_DIRT_PCT(),
            t::DOSE_CAKE_WEIGHT_PM(),
            t::DOSE_CAKE_MOOD_PCT(),
            false,
        ),
    };
    Meal {
        full,
        dirt,
        weight_pm,
        mood,
        healthy,
    }
}

fn eat(s: &mut GameState, food: FoodKind) {
    let m = meal(food);
    // Eating past the band, counted up to the cap even though the stored
    // level tops out at 100 %, is a food coma and goes to the waist.
    let after = s.fullness as i32 + lvl(m.full);
    let excess = (after.min(lvl(t::DOSE_FULL_CAP_PCT())) - needs::full_hi()).max(0);
    if excess > 0 {
        s.causes.push(Cause::FoodComa);
    }
    bump(&mut s.fullness, lvl(m.full));
    bump(&mut s.dirt, lvl(m.dirt));
    bump(
        &mut s.weight,
        m.weight_pm as i32 * P / 10 + excess / t::DOSE_EXCESS_WEIGHT_DIV() as i32,
    );
    bump(
        &mut s.energy,
        -(excess / t::DOSE_EXCESS_ENERGY_DIV() as i32),
    );
    if m.healthy {
        bump(&mut s.hydration, lvl(t::DOSE_HEALTHY_WATER_PCT()));
    }
    if food == FoodKind::Salad {
        bump(&mut s.fitness, lvl(t::DOSE_SALAD_FIT_PCT()));
    }
    if m.mood > 0 {
        // A treat: full mood only when the last one was over three hours ago,
        // none at all on Ozempic; either way the window restarts.
        if s.ozempic_min == 0 {
            let pct = if s.junk_min == 0 {
                m.mood
            } else {
                s.causes.push(Cause::TreatTooSoon);
                m.mood / t::DOSE_JUNK_REPEAT_DIV()
            };
            bump(&mut s.mood, lvl(pct));
        } else {
            s.causes.push(Cause::NoTreatBuzz);
        }
        s.junk_min = t::DOSE_JUNK_MIN();
    }
}

/// What one non-water drink does, from the thresholds.
struct Pour {
    water: u8,
    belly: u8,
    booze: u8,
    mood: u8,
    energy: u8,
    fun: u8,
    weight: u8,
}

fn pour(drink: DrinkKind) -> Pour {
    let alcohol = |water, belly, booze, mood, weight| Pour {
        water,
        belly,
        booze,
        mood,
        energy: t::DOSE_ALCOHOL_ENERGY_PCT(),
        fun: t::DOSE_ALCOHOL_FUN_PCT(),
        weight,
    };
    match drink {
        DrinkKind::Coffee => Pour {
            water: t::DOSE_COFFEE_WATER_PCT(),
            belly: t::DOSE_COFFEE_FULL_PCT(),
            booze: 0,
            mood: t::DOSE_COFFEE_MOOD_PCT(),
            energy: t::DOSE_COFFEE_ENERGY_PCT(),
            fun: 0,
            weight: t::DOSE_COFFEE_WEIGHT_PCT(),
        },
        DrinkKind::Beer => alcohol(
            t::DOSE_BEER_WATER_PCT(),
            t::DOSE_BEER_BELLY_PCT(),
            t::DOSE_BEER_BOOZE_PCT(),
            t::DOSE_BEER_MOOD_PCT(),
            t::DOSE_BEER_WEIGHT_PCT(),
        ),
        DrinkKind::Wine => alcohol(
            t::DOSE_WINE_WATER_PCT(),
            t::DOSE_WINE_BELLY_PCT(),
            t::DOSE_WINE_BOOZE_PCT(),
            t::DOSE_WINE_MOOD_PCT(),
            0,
        ),
        DrinkKind::Whiskey => alcohol(
            t::DOSE_WHISKEY_WATER_PCT(),
            t::DOSE_WHISKEY_BELLY_PCT(),
            t::DOSE_WHISKEY_BOOZE_PCT(),
            t::DOSE_WHISKEY_MOOD_PCT(),
            0,
        ),
        DrinkKind::Water => unreachable!("water has its own dose"),
    }
}

fn sip(s: &mut GameState, drink: DrinkKind) {
    let (water, belly) = (s.hydration as i32, s.fullness as i32);
    pour_drink(s, drink);
    // Past the band on either side of the belly is sloshing.
    if (water <= needs::water_hi() && s.hydration as i32 > needs::water_hi())
        || (belly <= needs::full_hi() && s.fullness as i32 > needs::full_hi())
    {
        s.causes.push(Cause::Sloshing);
    }
}

fn pour_drink(s: &mut GameState, drink: DrinkKind) {
    if drink == DrinkKind::Water {
        // A pet only drinks what it can hold: never past WATER_CAP_PCT.
        let room = (WATER_CAP_PCT * P - s.hydration as i32).max(0) / P;
        let taken = room.min(t::DOSE_WATER_MAX_PCT() as i32);
        bump(&mut s.hydration, taken * P);
        if taken >= t::DOSE_WATER_BELLY_MIN_PCT() as i32 {
            bump(&mut s.fullness, lvl(t::DOSE_WATER_BELLY_PCT()));
        }
        bump(&mut s.booze, -lvl(t::DOSE_WATER_BOOZE_PCT()));
        s.hangover_min = s.hangover_min.saturating_sub(t::DOSE_WATER_HANGOVER_MIN());
        s.groggy_min = s.groggy_min.saturating_sub(t::DOSE_WATER_GROGGY_MIN());
        return;
    }

    let p = pour(drink);
    let alcoholic = drink.is_alcoholic();
    if alcoholic && s.rehab_min > 0 {
        // Relapse.
        s.rehab_min = 0;
        bump(&mut s.mood, -lvl(t::DOSE_RELAPSE_MOOD_PCT()));
        s.causes.push(Cause::Relapse);
    }
    // The state the pet was in when the glass came: tired hits harder, a
    // drunk pet gets less out of it, a dependent one half of that.
    let tired = (s.energy as i32) < needs::energy_lo();
    let mut dim = (100 - needs::pct(s.booze as i32)).max(0);
    if s.dependent {
        dim /= t::DOSE_DEPENDENT_DIV() as i32;
    }
    let amp = if tired {
        t::DOSE_TIRED_BOOZE_PCT() as i32
    } else {
        100
    };
    let (water, booze) = (s.hydration as i32, s.booze as i32);

    bump(&mut s.hydration, lvl(p.water));
    bump(&mut s.fullness, lvl(p.belly));
    bump(&mut s.weight, lvl(p.weight));
    bump(&mut s.fun, lvl(p.fun));
    if alcoholic {
        bump(&mut s.booze, lvl(p.booze) * amp / 100);
        bump(&mut s.mood, lvl(p.mood) * dim / 100);
        bump(&mut s.energy, lvl(p.energy) * dim / 100);
        if dim < DULLED_DIM_PCT {
            s.causes.push(Cause::DrinkDulled);
        }
        if water < needs::water_lo() {
            s.causes.push(Cause::DryMouth);
        }
        if booze <= needs::booze_hi() && s.booze as i32 > needs::booze_hi() {
            s.causes.push(Cause::OneTooMany);
        }
    } else {
        bump(&mut s.mood, lvl(p.mood));
        bump(&mut s.energy, lvl(p.energy));
    }
    if drink == DrinkKind::Coffee {
        coffee(s);
    }
}

/// Most caffeine (minutes) at which one more cup does not wire the pet:
/// `COFFEE_WIRED_CUPS - 1` cups' worth less the cup itself. Negative when
/// every cup wires (`COFFEE_WIRED_CUPS` 0 or 1).
pub fn caffeine_safe_min() -> i32 {
    let cup = t::COFFEE_WINDOW_MIN() as i32;
    (t::COFFEE_WIRED_CUPS() as i32 - 1).max(0) * cup - cup
}

/// Whether a cup of coffee now would wire the pet. With cups adding
/// `COFFEE_WINDOW_MIN` each and wearing off one a minute, the
/// `COFFEE_WIRED_CUPS`-th cup within `COFFEE_WINDOW_MIN` of the first is the
/// one; older cups still count for what is left of them.
pub fn coffee_would_wire(s: &GameState) -> bool {
    s.caffeine_min as i32 > caffeine_safe_min()
}

/// Minutes until a cup stops wiring the pet, for the segment solver; `None`
/// when that never changes by itself (already safe, or every cup wires).
pub fn caffeine_to_safe(s: &GameState) -> Option<u32> {
    let safe = caffeine_safe_min();
    (safe >= 0 && s.caffeine_min as i32 > safe).then(|| (s.caffeine_min as i32 - safe) as u32)
}

/// What coffee does on top of its pour: it ends grogginess and gives back
/// the energy the grogginess took, and a cup over the caffeine line wires
/// the pet: fun goes past the wired line by `COFFEE_WIRED_MIN` minutes of
/// fun drain, so the existing wired state runs for that long.
fn coffee(s: &mut GameState) {
    if s.groggy_min > 0 {
        s.groggy_min = 0;
        bump(&mut s.energy, lvl(t::GROGGY_ENERGY_PCT()));
        s.causes.push(Cause::PerkedUp);
    }
    if coffee_would_wire(s) {
        let spell = (t::COFFEE_WIRED_MIN() as i32).saturating_mul(t::FUN_DRAIN_PM() as i32);
        let wired = needs::fun_hi().saturating_add(spell);
        s.fun = s.fun.max(needs::clamp_level(wired) as u16);
        s.causes.push(Cause::CoffeeWired);
    }
    s.caffeine_min = s.caffeine_min.saturating_add(t::COFFEE_WINDOW_MIN());
}

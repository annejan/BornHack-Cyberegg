//! Demo mode (plan 3). The pet looks after itself through the delta engine's
//! events: its thresholds are solver lines (`levels`), its timers are solver
//! bounds (`timers`), and `settle` and `decide` read the state at a segment
//! boundary. Nothing here polls. The saved state sits in the reserved tail of
//! the format-3 save, so a plan-2 save (zeros there) reads as demo off.

use super::advance::SLOW_SCALE;
use super::causes::{Cause, CauseQueue};
use super::needs::{MAX, Need, P};
use super::thresholds as t;
use super::{Action, DrinkKind, FoodKind, GameState, MiniGame, Phase, TICKS_PER_MINUTE};

/// Where the demo block starts in the save, right after `saved_unix`.
pub const BLOCK_AT: usize = 149;
/// Bytes in the demo block.
pub const BLOCK_LEN: usize = 11;

/// Minutes in a day: bedtime and getting-up lines count minutes since 00:00.
pub const MINUTES_PER_DAY: u16 = 1440;
/// Mini-games the pet can pick from.
pub const GAME_COUNT: usize = MiniGame::ALL.len();
/// Grades a finished mini-game can have (0..=2).
pub const GRADES: u8 = 3;
/// Percent scale of the dice rolls (`rng % 100`).
const PERCENT: u32 = 100;
/// Most minutes the saved soft delay (one byte) holds.
const BYTE_MAX_MIN: u32 = u8::MAX as u32;
/// Bits the temperament takes in the flags byte.
pub const LAZY_BITS: u8 = 4;
/// Largest temperament the flags byte holds.
pub const LAZY_MAX: u8 = (1 << LAZY_BITS) - 1;
/// Flags byte: where the temperament starts.
const LAZY_SHIFT: u8 = 8 - LAZY_BITS;
/// Bit 15 of the last field is `pet_action`; the hours use the low 15 bits.
const PET_ACTION_BIT: u16 = 1 << 15;
/// Most whole hours the saved lifetime holds (3.7 years).
const HOURS_MAX: u16 = PET_ACTION_BIT - 1;
const MIN_PER_HOUR: u32 = 60;
/// Mixing multipliers for `seed_rng` (the 64-bit golden ratio, xxHash prime 2).
const GOLDEN_GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;
const XXH_PRIME64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;

// Flags byte of the block.
const FLAG_ON: u8 = 1;
const FLAG_SOFT_ARMED: u8 = 2;
const FLAG_CLOCK_ARMED: u8 = 4;
const FLAG_ANIMATED: u8 = 8;

/// The saved demo block (save bytes 149..160). All zero = demo off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Demo {
    pub on: bool,
    /// Soft delay `next_min` is running.
    pub soft_armed: bool,
    /// `clock_min` counts to bedtime (awake) or getting up (asleep).
    pub clock_armed: bool,
    /// Temperament 0..=`DEMO_LAZY_MAX_PCT`, re-rolled every `DEMO_LAZY_MIN`.
    pub lazy: u8,
    pub clock_min: u16,
    pub whim_min: u16,
    pub lazy_min: u16,
    /// Soft delay or dry-out interval.
    pub next_min: u8,
    /// Policy paused after the player acted.
    pub pause_min: u8,
    /// Care mode: false = instant with a note (default), true = animated.
    pub animated: bool,
    /// The running action is the pet's own.
    pub pet_action: bool,
    /// Minutes lived in demo; saved as whole hours (15 bits).
    pub demo_min: u32,
}

impl Demo {
    pub fn write(&self, b: &mut [u8; BLOCK_LEN]) {
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        b[0] = flag(self.on, FLAG_ON)
            | flag(self.soft_armed, FLAG_SOFT_ARMED)
            | flag(self.clock_armed, FLAG_CLOCK_ARMED)
            | flag(self.animated, FLAG_ANIMATED)
            | (self.lazy.min(LAZY_MAX)) << LAZY_SHIFT;
        b[1..3].copy_from_slice(&self.clock_min.to_le_bytes());
        b[3..5].copy_from_slice(&self.whim_min.to_le_bytes());
        b[5..7].copy_from_slice(&self.lazy_min.to_le_bytes());
        b[7] = self.next_min;
        b[8] = self.pause_min;
        let hours = (self.demo_min / MIN_PER_HOUR).min(HOURS_MAX as u32) as u16;
        let last = hours | if self.pet_action { PET_ACTION_BIT } else { 0 };
        b[9..11].copy_from_slice(&last.to_le_bytes());
    }

    pub fn read(b: &[u8; BLOCK_LEN]) -> Demo {
        let last = u16::from_le_bytes([b[9], b[10]]);
        Demo {
            on: b[0] & FLAG_ON != 0,
            soft_armed: b[0] & FLAG_SOFT_ARMED != 0,
            clock_armed: b[0] & FLAG_CLOCK_ARMED != 0,
            animated: b[0] & FLAG_ANIMATED != 0,
            lazy: b[0] >> LAZY_SHIFT,
            clock_min: u16::from_le_bytes([b[1], b[2]]),
            whim_min: u16::from_le_bytes([b[3], b[4]]),
            lazy_min: u16::from_le_bytes([b[5], b[6]]),
            next_min: b[7],
            pause_min: b[8],
            pet_action: last & PET_ACTION_BIT != 0,
            demo_min: (last & HOURS_MAX) as u32 * MIN_PER_HOUR,
        }
    }
}

/// A fall that crosses `floor` stops on it; a stat already below it does not
/// fall further and is not raised.
pub fn floor(before: u16, after: u16, floor: u16) -> u16 {
    after.max(before.min(floor))
}

/// The fitness floor as a level.
pub fn fit_floor() -> u16 {
    (t::DEMO_FIT_FLOOR_PCT() as i32 * P) as u16
}

/// The health floor as a level.
pub fn health_floor() -> u16 {
    (t::DEMO_HEALTH_FLOOR_PCT() as i32 * P) as u16
}

/// Fitness after a fall from `before` to `after`: held at the demo floor
/// while demo is on; unchanged while it is off.
pub fn fit_after_fall(s: &GameState, before: u16, after: u16) -> u16 {
    if s.demo.on {
        floor(before, after, fit_floor())
    } else {
        after
    }
}

/// A need that sits on its floor and still falls cannot move: the solver
/// skips it, like a need at 0, so a pet resting on the floor takes long
/// segments.
pub fn pinned(s: &GameState, need: Need, level: i32, rate: i32) -> bool {
    s.demo.on && need == Need::Fit && rate < 0 && level <= fit_floor() as i32
}

/// `pct` percent as a level.
#[inline]
fn lvl(pct: u8) -> i32 {
    pct as i32 * P
}

/// A policy line as a level, kept inside the level range.
#[inline]
fn line(level: i32) -> i32 {
    level.clamp(0, MAX)
}

/// Every level the policy reads, with the temperament applied. `decide` and
/// `levels` both take their lines from here, so the solver stops on exactly
/// the lines the choice reads.
struct Lines {
    wake_full: i32,
    edge_full: i32,
    soft_full: i32,
    edge_water: i32,
    soft_water: i32,
    water_refuse: i32,
    edge_energy: i32,
    soft_energy: i32,
    play_energy: i32,
    nap_energy: i32,
    rise_energy: i32,
    wired_fun: i32,
    soft_fun: i32,
    nap_fun: i32,
    soft_dirt: i32,
    wash_refuse: i32,
    edge_booze: i32,
    whim_lo: i32,
    whim_hi: i32,
    light_food: i32,
    rich_food: i32,
}

fn lines(s: &GameState) -> Lines {
    let lazy = lvl(s.demo.lazy);
    let half_lazy = lvl(s.demo.lazy / 2);
    Lines {
        wake_full: line(lvl(t::DEMO_WAKE_FULL_PCT())),
        edge_full: line(lvl(t::DEMO_EDGE_FULL_PCT())),
        soft_full: line(lvl(t::DEMO_SOFT_FULL_PCT()) - lazy),
        edge_water: line(lvl(t::DEMO_EDGE_WATER_PCT())),
        soft_water: line(lvl(t::DEMO_SOFT_WATER_PCT()) - lazy),
        water_refuse: line(super::doses::WATER_REFUSE_PCT * P),
        edge_energy: line(lvl(t::DEMO_EDGE_ENERGY_PCT())),
        soft_energy: line(lvl(t::DEMO_SOFT_ENERGY_PCT()) - half_lazy),
        play_energy: line(lvl(t::DEMO_PLAY_ENERGY_PCT())),
        nap_energy: line(lvl(t::DEMO_NAP_ENERGY_PCT())),
        rise_energy: line(lvl(t::DEMO_RISE_ENERGY_PCT())),
        wired_fun: line(lvl(t::DEMO_WIRED_FUN_PCT()) + lazy),
        soft_fun: line(lvl(t::DEMO_SOFT_FUN_PCT()) - lazy),
        nap_fun: line(lvl(t::DEMO_NAP_FUN_PCT())),
        soft_dirt: line(lvl(t::DEMO_SOFT_DIRT_PCT()) + lazy),
        wash_refuse: line(super::doses::WASH_REFUSE_PCT * P),
        edge_booze: line(lvl(t::DEMO_EDGE_BOOZE_PCT())),
        whim_lo: line(lvl(t::DEMO_WHIM_BOOZE_LO_PCT())),
        whim_hi: line(lvl(t::DEMO_WHIM_BOOZE_HI_PCT())),
        light_food: line(lvl(t::DEMO_LIGHT_FOOD_PCT())),
        rich_food: line(lvl(t::DEMO_RICH_FOOD_PCT())),
    }
}

/// Most policy lines one need carries (energy: five).
pub const MAX_LINES: usize = 8;

/// Demo lines for one need (levels, as `lvl(..)`), appended by
/// `solver::extra_levels` while demo is on: the policy lines of `decide` and
/// `settle`, and the fitness floor while fitness is above it. Health lines
/// are in `solver::sig`.
pub fn levels(s: &GameState, need: Need) -> ([i32; MAX_LINES], usize) {
    let l = lines(s);
    let mut v = [0; MAX_LINES];
    let mut set = |xs: &[i32]| {
        v[..xs.len()].copy_from_slice(xs);
        xs.len()
    };
    let n = match need {
        Need::Full => set(&[l.wake_full, l.edge_full, l.soft_full]),
        Need::Water => set(&[l.edge_water, l.soft_water, l.water_refuse]),
        Need::Energy => set(&[
            l.edge_energy,
            l.soft_energy,
            l.play_energy,
            l.nap_energy,
            l.rise_energy,
        ]),
        Need::Fun => set(&[l.wired_fun, l.soft_fun, l.nap_fun]),
        Need::Dirt => set(&[l.soft_dirt, l.wash_refuse]),
        Need::Booze => set(&[l.edge_booze, l.whim_lo, l.whim_hi]),
        Need::Weight => set(&[l.light_food, l.rich_food]),
        Need::Fit if s.fitness > fit_floor() => set(&[fit_floor() as i32]),
        Need::Fit | Need::Strain => 0,
    };
    debug_assert!(v[..n].iter().all(|&l| (0..=MAX).contains(&l)));
    (v, n)
}

// ---------------------------------------------------------------------------
// Policy: what the pet chooses
// ---------------------------------------------------------------------------

/// What the pet does next. Every choice maps onto a public starter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Eat(FoodKind),
    Drink(DrinkKind),
    Sleep,
    Nap,
    Wake,
    Wash,
    Play,
    /// A mini-game and the grade it ends with (0..`GRADES`).
    Game(MiniGame, u8),
    Relax,
    Medicine,
    Insulin,
    Rehab,
    /// "Only pets": the pet works for HAX.
    Work,
}

/// Whether the starter behind `c` would take it now. The pet never takes a
/// choice its starter would refuse: the row falls through to the next.
fn allowed(s: &GameState, c: Choice) -> bool {
    affordable(s, c)
        && match c {
        Choice::Eat(_) => s.refusal_for(Action::Feed, None).is_none(),
        Choice::Drink(d) => s.refusal_for(Action::Drink, Some(d)).is_none(),
        Choice::Sleep | Choice::Nap => s.sleep_refusal().is_none(),
        Choice::Wake => s.is_sleeping,
        Choice::Wash => s.refusal_for(Action::Wash, None).is_none(),
        Choice::Play => s.refusal_for(Action::Play, None).is_none(),
        Choice::Game(g, _) => s.game_refusal(g).is_none(),
        Choice::Relax => s.refusal_for(Action::Relax, None).is_none(),
        Choice::Medicine => s.refusal_for(Action::Heal, None).is_none(),
        Choice::Insulin => s.refusal_for(Action::Medicate, None).is_none(),
        Choice::Rehab => s.refusal_for(Action::Rehab, None).is_none(),
        Choice::Work => s.refusal_for(Action::OnlyPets, None).is_none(),
    }
}

/// The pet pays its own way while HAX is live: a choice it cannot afford
/// falls through to the next row (its health floors hold meanwhile).
fn affordable(s: &GameState, c: Choice) -> bool {
    let Some((a, food, drink)) = action_of(c) else {
        return true;
    };
    !s.money_live()
        || super::doses::price(a, food, drink, s.hard_mode).is_none_or(|p| s.can_afford(p))
}

/// The balance the pet works up to and no more (`DEMO_HAX_TARGET`).
fn hax_target() -> u32 {
    t::DEMO_HAX_TARGET() as u32
}

/// HAX is live and below the target: the pet has a reason to work.
fn wants_pay(s: &GameState) -> bool {
    s.money_live() && s.money < hax_target()
}

/// The food the urgent hunger row takes.
fn urgent_food(s: &GameState) -> FoodKind {
    if s.weight as i32 > lines(s).light_food {
        FoodKind::Apple
    } else {
        FoodKind::Pizza
    }
}

/// HAX is live and below the reserve (half the target, never less than the
/// urgent meal or water): the pet is about to be unable to look after itself
/// and goes to work, after the urgent care it can still pay for. It is also
/// the escape hatch of a broke pet: work is never gated on money.
fn broke(s: &GameState) -> bool {
    let meal = super::doses::price(Action::Feed, Some(urgent_food(s)), None, s.hard_mode);
    let water = super::doses::price(Action::Drink, None, Some(DrinkKind::Water), s.hard_mode);
    let cheapest = meal.into_iter().chain(water).min().unwrap_or(0);
    s.money_live() && s.money < cheapest.max(hax_target() / 2)
}

/// A sleeper that wakes up hungry has to be able to do something about it:
/// pay for the meal, or work for it (rested enough). Else it stays in bed
/// and recovers (a broke, starving, exhausted pet would otherwise wake and
/// fall asleep again, over and over).
fn can_feed_itself(s: &GameState) -> bool {
    let meal = super::doses::price(Action::Feed, Some(urgent_food(s)), None, s.hard_mode);
    !s.money_live()
        || meal.is_none_or(|p| s.can_afford(p))
        || (s.injured_min == 0 && (s.energy as i32) >= super::needs::energy_lo())
}

/// Demo keeps the balance near `DEMO_HAX_TARGET`: above it the balance falls
/// by 1 HAX every `DEMO_HAX_DRIFT_MIN` minutes of demo time, never below the
/// target; below it nothing tops it up. The steps are the crossings of
/// `demo_min` between `before` and now, so one long update and many short
/// ones give the same balance. Called by `advance` after `demo_min` moved.
pub fn drift_hax(s: &mut GameState, before: u32) {
    if !s.money_live() || s.money <= hax_target() {
        return;
    }
    let every = t::DEMO_HAX_DRIFT_MIN().max(1) as u32;
    let steps = s.demo.demo_min / every - before / every;
    if steps > 0 {
        s.money = hax_target().max(s.money.saturating_sub(steps));
        s.request_save();
    }
}

/// The first row that applies and is allowed.
fn first(s: &GameState, rows: &[(bool, Choice)]) -> Option<Choice> {
    rows.iter()
        .find(|&&(on, c)| on && allowed(s, c))
        .map(|&(_, c)| c)
}

/// The dice, without stepping them: a roll of 0..100.
fn roll(s: &GameState) -> u32 {
    s.demo_rng % PERCENT
}

/// A draw of 0..=`n` that steps the dice (`settle` and `act`: every delay
/// they set is a draw).
fn draw(s: &mut GameState, n: u32) -> u32 {
    let v = s.demo_rng % (n + 1);
    s.demo_rng = crate::platform::xorshift32(s.demo_rng);
    v
}

fn food_draw(s: &GameState) -> FoodKind {
    let v = roll(s);
    let mut upto = t::DEMO_PICK_APPLE_PCT() as u32;
    let mut food = if v < upto {
        FoodKind::Apple
    } else {
        upto += t::DEMO_PICK_SALAD_PCT() as u32;
        if v < upto {
            FoodKind::Salad
        } else {
            upto += t::DEMO_PICK_PIZZA_PCT() as u32;
            if v < upto {
                FoodKind::Pizza
            } else {
                upto += t::DEMO_PICK_FRIK_PCT() as u32;
                if v < upto {
                    FoodKind::Frikandel
                } else {
                    FoodKind::Cake
                }
            }
        }
    };
    let (weight, l) = (s.weight as i32, lines(s));
    if weight > l.light_food
        && matches!(food, FoodKind::Pizza | FoodKind::Frikandel | FoodKind::Cake)
    {
        food = FoodKind::Salad;
    }
    if weight < l.rich_food && matches!(food, FoodKind::Apple | FoodKind::Salad) {
        food = FoodKind::Pizza;
    }
    food
}

/// The pet's drink. It never takes the coffee that would wire it: water
/// instead.
fn drink_draw(s: &GameState) -> DrinkKind {
    let v = roll(s);
    let water = t::DEMO_PICK_WATER_PCT() as u32;
    let coffee = water + t::DEMO_PICK_COFFEE_PCT() as u32;
    if v < water {
        DrinkKind::Water
    } else if v < coffee || s.booze as i32 > lines(s).whim_lo {
        if coffee_ok(s) {
            DrinkKind::Coffee
        } else {
            DrinkKind::Water
        }
    } else {
        DrinkKind::Beer
    }
}

/// A coffee now would not wire the pet. The solver stops where this flips
/// (`doses::caffeine_to_safe`).
fn coffee_ok(s: &GameState) -> bool {
    !super::doses::coffee_would_wire(s)
}

/// A mini-game (the first one not cooling down, from the dice's pick) or
/// plain play.
fn play_or_game(s: &GameState) -> Choice {
    if roll(s) < t::DEMO_PICK_GAME_PCT() as u32 {
        let start = s.demo_rng as usize % GAME_COUNT;
        let game = (0..GAME_COUNT)
            .map(|k| MiniGame::ALL[(start + k) % GAME_COUNT])
            .find(|&g| s.game_refusal(g).is_none());
        if let Some(g) = game {
            return Choice::Game(g, (s.demo_rng % GRADES as u32) as u8);
        }
    }
    // Below the target a shift is the better play: it pays instead of costing.
    if wants_pay(s) && allowed(s, Choice::Work) {
        return Choice::Work;
    }
    Choice::Play
}

/// The dependence counter is past the line where the pet stops asking for
/// beer (`dependence_min` counts quarter-minutes). Demo never adds to the
/// counter (it only decays), so this only holds back a pet that carries a
/// high counter in from player time.
fn beer_stopped(s: &GameState) -> bool {
    s.dependence_min > beer_stop_counter()
}

fn beer_stop_counter() -> u32 {
    t::DEMO_BEER_STOP_MIN() as u32 * SLOW_SCALE
}

/// The soft rows (row 12): the first one that is true and allowed.
fn soft_pick(s: &GameState) -> Option<Choice> {
    let l = lines(s);
    let rows = [
        (
            (s.hydration as i32) < l.soft_water,
            Choice::Drink(drink_draw(s)),
        ),
        ((s.fullness as i32) < l.soft_full, Choice::Eat(food_draw(s))),
        (
            s.day_min.is_none() && (s.energy as i32) < l.soft_energy,
            Choice::Sleep,
        ),
        (s.dirt as i32 > l.soft_dirt, Choice::Wash),
        (
            (s.fun as i32) < l.soft_fun && s.energy as i32 > l.play_energy && s.injured_min == 0,
            play_or_game(s),
        ),
        // Last: below the target the pet works, when nothing else is due.
        (
            wants_pay(s) && s.energy as i32 > l.play_energy,
            Choice::Work,
        ),
    ];
    first(s, &rows)
}

/// The whim rows (row 13).
fn whim_pick(s: &GameState) -> Option<Choice> {
    let l = lines(s);
    let (booze, dice) = (s.booze as i32, roll(s));
    let beer = !beer_stopped(s);
    let rows = [
        (
            booze < l.whim_lo && beer && dice < t::DEMO_WHIM_BEER_LO_PCT() as u32,
            Choice::Drink(DrinkKind::Beer),
        ),
        (
            booze < l.whim_lo && s.weight as i32 <= l.light_food,
            Choice::Eat(FoodKind::Cake),
        ),
        (
            booze < l.whim_hi && beer && dice < t::DEMO_WHIM_BEER_HI_PCT() as u32,
            Choice::Drink(DrinkKind::Beer),
        ),
        (
            (s.energy as i32) < l.nap_energy && (s.fun as i32) < l.nap_fun,
            Choice::Nap,
        ),
    ];
    first(s, &rows)
}

/// While the pet is drunk the soft delay is the dry-out interval, and the
/// pet wants water as soon as it is allowed.
fn drunk(s: &GameState) -> bool {
    s.booze as i32 > lines(s).edge_booze
}

/// Whether the soft delay should be running: an awake pet with a soft row
/// true, or a drunk one that may drink water.
fn soft_wanted(s: &GameState) -> bool {
    if s.is_sleeping {
        false
    } else if drunk(s) {
        allowed(s, Choice::Drink(DrinkKind::Water))
    } else {
        soft_pick(s).is_some()
    }
}

/// Which kind of row picked a choice: it decides what `act` re-arms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Row {
    /// Insulin, rehab, the edge rows and the bedtime and boredom rows.
    Urgent,
    /// A soft row, after the soft delay (or the dry-out water).
    Soft,
    /// A whim.
    Whim,
}

/// What the pet does now, or `None`. Pure: reads the dice without stepping
/// them. The first row that applies and is not refused (table in the plan).
pub fn decide(s: &GameState) -> Option<Choice> {
    decide_row(s).map(|(c, _)| c)
}

/// `decide`, with the kind of row that chose.
fn decide_row(s: &GameState) -> Option<(Choice, Row)> {
    if !s.demo.on
        || s.phase != Phase::Active
        || s.hibernating
        || s.active_action.is_some()
        || s.demo.pause_min != 0
    {
        return None;
    }
    let d = &s.demo;
    let l = lines(s);
    let clock_due = d.clock_armed && d.clock_min == 0;
    if s.is_sleeping {
        let wake = clock_due || ((s.fullness as i32) < l.wake_full && can_feed_itself(s));
        return wake.then_some((Choice::Wake, Row::Urgent));
    }
    let food = urgent_food(s);
    let rows = [
        (s.is_diabetic_unmedicated(), Choice::Insulin),
        (s.is_alcoholic_untreated(), Choice::Rehab),
        ((s.fullness as i32) < l.edge_full, Choice::Eat(food)),
        (
            (s.hydration as i32) < l.edge_water,
            Choice::Drink(DrinkKind::Water),
        ),
        ((s.energy as i32) < l.edge_energy, Choice::Sleep),
        (
            // The solver's line is `health > MED` (`sig`), so "below" is its
            // complement and the flip lands on a segment boundary.
            (s.health as i32) <= lvl(t::DEMO_MED_HEALTH_PCT()),
            Choice::Medicine,
        ),
    ];
    if let Some(c) = first(s, &rows) {
        return Some((c, Row::Urgent));
    }
    let delay_done = d.soft_armed && d.next_min == 0;
    if drunk(s) {
        let water = Choice::Drink(DrinkKind::Water);
        return (delay_done && allowed(s, water)).then_some((water, Row::Soft));
    }
    if let Some(c) = first(
        s,
        &[
            // Too poor for care: work now (urgent care came first above).
            (broke(s), Choice::Work),
            (clock_due, Choice::Sleep),
            (s.fun as i32 > l.wired_fun, Choice::Relax),
        ],
    ) {
        return Some((c, Row::Urgent));
    }
    if delay_done && let Some(c) = soft_pick(s) {
        return Some((c, Row::Soft));
    }
    // A groggy morning: a coffee perks the pet up, unless it would wire it.
    let coffee = Choice::Drink(DrinkKind::Coffee);
    if s.groggy_min > 0 && coffee_ok(s) && allowed(s, coffee) {
        return Some((coffee, Row::Urgent));
    }
    if d.whim_min == 0 {
        return whim_pick(s).map(|c| (c, Row::Whim));
    }
    None
}

/// Whether the demo pet's help line is up: demo, and health under
/// `DEMO_HELP_HEALTH_PCT` (design 8.3), so it agrees with the floored
/// percentage on screen. The solver's line is `health > HELP - 1` (`sig`),
/// so the flip lands on a segment boundary.
pub fn needs_help(s: &GameState) -> bool {
    s.demo.on && (s.health as i32) < lvl(t::DEMO_HELP_HEALTH_PCT())
}

/// The demo part of the wake view: bit 0 a choice is waiting, bit 1 the
/// help line.
pub fn wake_view(s: &GameState) -> u8 {
    (decide(s).is_some() as u8) | ((needs_help(s) as u8) << 1)
}

// ---------------------------------------------------------------------------
// Acting: the pet looks after itself
// ---------------------------------------------------------------------------

/// What the pet last did by itself, for the one-line note under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Note {
    Ate(FoodKind),
    Drank(DrinkKind),
    OneTooMany,
    Napping,
    OffToBed,
    UpAndAbout,
    Washing,
    Played,
    Rested,
    Medicine,
    Insulin,
    Rehab,
    Worked,
}

impl Note {
    /// Every note once (host-only, for the guide's word list).
    #[cfg(any(test, feature = "simulator"))]
    pub const ALL: [Note; 21] = [
        Note::Ate(FoodKind::Apple),
        Note::Ate(FoodKind::Salad),
        Note::Ate(FoodKind::Pizza),
        Note::Ate(FoodKind::Frikandel),
        Note::Ate(FoodKind::Cake),
        Note::Drank(DrinkKind::Water),
        Note::Drank(DrinkKind::Coffee),
        Note::Drank(DrinkKind::Beer),
        Note::Drank(DrinkKind::Wine),
        Note::Drank(DrinkKind::Whiskey),
        Note::OneTooMany,
        Note::Napping,
        Note::OffToBed,
        Note::UpAndAbout,
        Note::Washing,
        Note::Played,
        Note::Rested,
        Note::Medicine,
        Note::Insulin,
        Note::Rehab,
        Note::Worked,
    ];

    /// The words (at most 21 columns).
    pub fn text(self) -> &'static str {
        match self {
            Note::Ate(FoodKind::Apple) => "had an apple",
            Note::Ate(FoodKind::Salad) => "had a salad",
            Note::Ate(FoodKind::Pizza) => "had some pizza",
            Note::Ate(FoodKind::Frikandel) => "had a Frikandel spec",
            Note::Ate(FoodKind::Cake) => "had some cake",
            Note::Drank(DrinkKind::Water) => "had some water",
            Note::Drank(DrinkKind::Coffee) => "had a coffee",
            Note::Drank(DrinkKind::Beer) => "fancied a beer",
            Note::Drank(DrinkKind::Wine) => "had some wine",
            Note::Drank(DrinkKind::Whiskey) => "had a whiskey",
            Note::OneTooMany => "one too many...",
            Note::Napping => "napping",
            Note::OffToBed => "off to bed",
            Note::UpAndAbout => "up and about",
            Note::Washing => "having a wash",
            Note::Played => "had a play",
            Note::Rested => "had a rest",
            Note::Medicine => "took medicine",
            Note::Insulin => "took insulin",
            Note::Rehab => "went to rehab",
            Note::Worked => "did a shift",
        }
    }
}

/// Most acts one boundary runs. Rows 3 to 13 can all be pending at once; what
/// is left waits for the next boundary (the next update).
pub const MAX_ACTS_PER_BOUNDARY: u8 = 8;

/// The note an act leaves.
fn note_for(c: Choice) -> Note {
    match c {
        Choice::Eat(f) => Note::Ate(f),
        Choice::Drink(d) => Note::Drank(d),
        Choice::Sleep => Note::OffToBed,
        Choice::Nap => Note::Napping,
        Choice::Wake => Note::UpAndAbout,
        Choice::Wash => Note::Washing,
        Choice::Play | Choice::Game(..) => Note::Played,
        Choice::Relax => Note::Rested,
        Choice::Medicine => Note::Medicine,
        Choice::Insulin => Note::Insulin,
        Choice::Rehab => Note::Rehab,
        Choice::Work => Note::Worked,
    }
}

/// The action behind a timed choice, with its food or drink.
fn action_of(c: Choice) -> Option<(Action, Option<FoodKind>, Option<DrinkKind>)> {
    Some(match c {
        Choice::Eat(f) => (Action::Feed, Some(f), None),
        Choice::Drink(d) => (Action::Drink, None, Some(d)),
        Choice::Wash => (Action::Wash, None, None),
        Choice::Play => (Action::Play, None, None),
        Choice::Relax => (Action::Relax, None, None),
        Choice::Medicine => (Action::Heal, None, None),
        Choice::Insulin => (Action::Medicate, None, None),
        Choice::Rehab => (Action::Rehab, None, None),
        Choice::Work => (Action::OnlyPets, None, None),
        Choice::Sleep | Choice::Nap | Choice::Wake | Choice::Game(..) => return None,
    })
}

/// Take `c` now, through the real starter and dose, and leave a note. The act
/// takes no ticks: the dose lands in the same minute. It steps the dice once,
/// plus once for each delay it draws. Called directly, it re-arms the delays
/// of the row `decide` would pick for `c` (none when `decide` disagrees).
pub fn act(s: &mut GameState, c: Choice) {
    let row = decide_row(s).filter(|&(d, _)| d == c).map(|(_, r)| r);
    act_row(s, c, row);
}

/// `act`, with the row that chose `c` already known.
fn act_row(s: &mut GameState, c: Choice, row: Option<Row>) {
    s.self_care = true;
    let started = act_start(s, c);
    debug_assert!(started, "{c:?} was refused");
    match row {
        Some(Row::Whim) => {
            let wait = t::DEMO_WHIM_BASE_MIN() as u32 + draw(s, t::DEMO_WHIM_SPREAD_MIN() as u32);
            s.demo.whim_min = wait.min(u16::MAX as u32) as u16;
        }
        Some(Row::Soft) => {
            s.demo.soft_armed = false;
            s.demo.next_min = 0;
        }
        Some(Row::Urgent) | None => {}
    }
    s.demo_rng = crate::platform::xorshift32(s.demo_rng);
    // A timed act leaves its note when it ends (`note_own_act`), at once in
    // instant care, at the animation's end in animated care.
    if action_of(c).is_none() {
        s.note = Some((note_for(c), super::status::status_line(s)));
    }
    s.self_care = false;
    #[cfg(feature = "simulator")]
    super::sim::note_self_care();
}

/// The note an act of its own leaves when `action` ends, with the causes its
/// dose queued.
pub(super) fn note_own_act(s: &mut GameState, action: Action, queued: &CauseQueue) {
    let note = match action {
        Action::Feed => s.active_food.map(Note::Ate),
        Action::Drink => s.active_drink.map(Note::Drank),
        Action::Wash => Some(Note::Washing),
        Action::Play => Some(Note::Played),
        Action::Relax => Some(Note::Rested),
        Action::Heal => Some(Note::Medicine),
        Action::Medicate => Some(Note::Insulin),
        Action::Rehab => Some(Note::Rehab),
        Action::OnlyPets => Some(Note::Worked),
        Action::Exercise | Action::Ozempic => None,
    };
    let too_many = queued.iter().any(|&q| q == Cause::OneTooMany);
    let note = if too_many {
        Some(Note::OneTooMany)
    } else {
        note
    };
    s.note = note.map(|n| (n, super::status::status_line(s)));
}

/// Start `c` through its starter. A timed one is finished at once in instant
/// care and left running in animated care (it ends out of the engine-time
/// stream, see `GameState::action_end_tick`).
fn act_start(s: &mut GameState, c: Choice) -> bool {
    let started = match (action_of(c), c) {
        (Some((a, food, drink)), _) => s.start_choice(a, food, drink),
        (None, Choice::Sleep) => s.sleep(),
        (None, Choice::Nap) => {
            let ok = s.sleep();
            let wait = t::DEMO_NAP_BASE_MIN() as u32 + draw(s, t::DEMO_NAP_SPREAD_MIN() as u32);
            s.demo.clock_min = wait.min(u16::MAX as u32) as u16;
            s.demo.clock_armed = true;
            ok
        }
        (None, Choice::Wake) => s.wake(),
        (None, Choice::Game(g, grade)) => {
            s.award_inspiration(g, grade);
            true
        }
        // `action_of` answers every other choice.
        (None, _) => false,
    };
    if started && action_of(c).is_some() {
        // The pet's own action: instant care ends it now, animated care
        // leaves it to run out of the engine-time stream.
        s.demo.pet_action = true;
        if !s.demo.animated {
            s.finish_action();
        }
    }
    started
}

/// Everything the pet does at a boundary: settle, decide, act, until nothing
/// is left to do (at most `MAX_ACTS_PER_BOUNDARY` acts). Returns the acts.
/// Hitting the cap with a choice left is a policy bug (a cycle), so it
/// asserts; `thresholds::validate` keeps the lines from making one.
pub fn run(s: &mut GameState) -> u8 {
    let (acts, capped) = run_capped(s);
    // Limit: the cap counts acts within one call. In animated care the first
    // timed act leaves an action running, so `run_capped` returns there, and
    // the update loop calls `run` again with a fresh count. A cycle made only
    // of timed acts is therefore not caught here; it would repeat back to
    // back (each act takes at least two ticks), so the loop still ends.
    // Instant care runs every act in the same call, so the cap catches it.
    debug_assert!(
        !capped,
        "the demo policy cycles: {acts} acts and more to do"
    );
    acts
}

/// `run`, also saying whether the cap stopped it with a choice left.
pub fn run_capped(s: &mut GameState) -> (u8, bool) {
    // The pet's own action is under way: the policy looks again when it ends.
    if s.demo.pet_action {
        return (0, false);
    }
    let mut acts = 0;
    loop {
        settle(s);
        match decide_row(s) {
            Some((c, row)) if acts < MAX_ACTS_PER_BOUNDARY => {
                act_row(s, c, Some(row));
                acts += 1;
            }
            Some(_) => return (acts, true),
            None => return (acts, false),
        }
    }
}

// ---------------------------------------------------------------------------
// Time: the clock minute and the timers
// ---------------------------------------------------------------------------

/// Slots of `timers`: clock, whim, lazy, soft delay, pause, the dependence
/// line, then one per mini-game cooldown.
const T_CLOCK: usize = 0;
const T_WHIM: usize = 1;
const T_LAZY: usize = 2;
const T_NEXT: usize = 3;
const T_PAUSE: usize = 4;
/// Minutes until the beer counter decays to the line where beer stops being
/// refused.
pub const T_DEPENDENCE: usize = 5;
const T_GAMES: usize = 6;
pub const TIMER_COUNT: usize = T_GAMES + GAME_COUNT;

/// Remaining minutes of each running timer, for `solver::timer_bound`;
/// `u32::MAX` when it is not running. All of them are plain: nothing the
/// regime reads depends on them, so the segment simply ends when one runs
/// out and `settle` and `decide` see it at that boundary.
pub fn timers(s: &GameState) -> [u32; TIMER_COUNT] {
    let mut v = [u32::MAX; TIMER_COUNT];
    if !s.demo.on {
        return v;
    }
    let d = &s.demo;
    let running = |on: bool, minutes: u32| if on && minutes > 0 { minutes } else { u32::MAX };
    v[T_CLOCK] = running(d.clock_armed, d.clock_min as u32);
    v[T_WHIM] = running(true, d.whim_min as u32);
    v[T_LAZY] = running(true, d.lazy_min as u32);
    v[T_NEXT] = running(d.soft_armed, d.next_min as u32);
    v[T_PAUSE] = running(true, d.pause_min as u32);
    // The beer counter decays while sober; it crosses its line at a minute.
    let sober = (s.booze as i32) < lvl(t::BOOZE_CLEAR_PCT());
    v[T_DEPENDENCE] = running(sober, s.dependence_min.saturating_sub(beer_stop_counter()));
    for (slot, game) in v[T_GAMES..].iter_mut().zip(MiniGame::ALL) {
        // A cooldown counts ticks; the starter takes the game at 0.
        *slot = running(
            true,
            (s.game_cooldown(game) as u32).div_ceil(TICKS_PER_MINUTE),
        );
    }
    v
}

/// Time passes by `dt` minutes (called from `advance`): the local minute of
/// the day moves on whatever the mode, the demo timers count down.
pub fn tick(s: &mut GameState, dt: u32) {
    let day = MINUTES_PER_DAY as u32;
    s.day_min = s.day_min.map(|m| ((m as u32 + dt) % day) as u16);
    if !s.demo.on {
        return;
    }
    let sub = |timer: u32| timer.saturating_sub(dt);
    let d = &mut s.demo;
    if d.clock_armed {
        d.clock_min = sub(d.clock_min as u32) as u16;
    }
    d.whim_min = sub(d.whim_min as u32) as u16;
    d.lazy_min = sub(d.lazy_min as u32) as u16;
    d.next_min = sub(d.next_min as u32) as u8;
    d.pause_min = sub(d.pause_min as u32) as u8;
}

// ---------------------------------------------------------------------------
// Settle: the bookkeeping at a segment boundary
// ---------------------------------------------------------------------------

/// Minutes from minute-of-day `from` to the next `to` (0 when equal).
fn until(from: u16, to: u16) -> u32 {
    let day = MINUTES_PER_DAY as u32;
    (to as u32 % day + day - from as u32 % day) % day
}

/// Inside the night window (bedtime to getting up, wrapping midnight).
fn in_night(m: u16) -> bool {
    let (bed, up) = (t::DEMO_BED_AT_MIN(), t::DEMO_UP_AT_MIN());
    if bed >= up {
        m >= bed || m < up
    } else {
        (bed..up).contains(&m)
    }
}

/// Bookkeeping at a segment boundary: re-roll the temperament, arm or
/// disarm the soft delay, arm the clock, re-arm a whim that has nothing to
/// do. Never acts and never changes what the player sees (`visible()`
/// apart from the choice bit of the wake view).
pub fn settle(s: &mut GameState) {
    if !s.demo.on {
        return;
    }
    if s.demo.lazy_min == 0 {
        let lazy = draw(s, t::DEMO_LAZY_MAX_PCT() as u32).min(LAZY_MAX as u32);
        s.demo.lazy = lazy as u8;
        s.demo.lazy_min = t::DEMO_LAZY_MIN();
    }
    arm_clock(s);
    arm_soft(s);
    if s.demo.whim_min == 0 && whim_pick(s).is_none() {
        let wait = t::DEMO_WHIM_BASE_MIN() as u32 + draw(s, t::DEMO_WHIM_SPREAD_MIN() as u32);
        s.demo.whim_min = wait.min(u16::MAX as u32) as u16;
    }
}

/// Arm the clock for the pet's present state, when it is not armed.
fn arm_clock(s: &mut GameState) {
    if s.demo.clock_armed {
        return;
    }
    let rested = s.energy as i32 >= lines(s).rise_energy;
    let (bed, up) = (t::DEMO_BED_AT_MIN(), t::DEMO_UP_AT_MIN());
    let minutes = match (s.day_min, s.is_sleeping) {
        // Own rhythm: a rested sleeper gets up after a while.
        (None, true) if rested => {
            Some(t::DEMO_RISE_BASE_MIN() as u32 + draw(s, t::DEMO_RISE_SPREAD_MIN() as u32))
        }
        (None, _) => None,
        (Some(m), false) if in_night(m) && !rested => Some(0),
        (Some(m), false) => {
            let bed_at = bed as u32 + draw(s, t::DEMO_BED_SPREAD_MIN() as u32);
            let bed_at = (bed_at % MINUTES_PER_DAY as u32) as u16;
            // Awake at night and rested: the next evening, so a pet woken
            // at night does not flap in and out of bed.
            Some(if in_night(m) {
                until(m, up) + until(up, bed_at)
            } else {
                until(m, bed_at)
            })
        }
        (Some(m), true) => {
            let before_bed = until(m, bed) <= t::DEMO_BED_SPREAD_MIN() as u32;
            Some(if in_night(m) || before_bed {
                let up_at = up as u32 + draw(s, t::DEMO_UP_SPREAD_MIN() as u32);
                until(m, (up_at % MINUTES_PER_DAY as u32) as u16)
            } else {
                t::DEMO_EDGE_NAP_MIN() as u32
            })
        }
    };
    if let Some(minutes) = minutes {
        s.demo.clock_min = minutes.min(u16::MAX as u32) as u16;
        s.demo.clock_armed = true;
    }
}

/// Arm the soft delay when a reason shows up, disarm it when none is left.
fn arm_soft(s: &mut GameState) {
    let wanted = soft_wanted(s);
    let dry = drunk(s);
    // A delay armed in the other state (sober or drunk) is re-armed.
    if wanted && (!s.demo.soft_armed || s.soft_dry != dry) {
        s.soft_dry = dry;
        let (base, spread) = if dry {
            (t::DEMO_DRY_BASE_MIN(), t::DEMO_DRY_SPREAD_MIN())
        } else {
            (t::DEMO_REACT_BASE_MIN(), t::DEMO_REACT_SPREAD_MIN())
        };
        let wait = base as u32 + draw(s, spread as u32);
        s.demo.next_min = wait.min(BYTE_MAX_MIN) as u8;
        s.demo.soft_armed = true;
    } else if !wanted && s.demo.soft_armed {
        s.demo.soft_armed = false;
        s.soft_dry = false;
        s.demo.next_min = 0;
    }
}

/// A nonzero rng state from two numbers (xorshift32 would stick at 0).
pub fn seed_rng(a: u64, b: u64) -> u32 {
    let mixed = a.wrapping_mul(GOLDEN_GAMMA) ^ b.wrapping_mul(XXH_PRIME64_2);
    let folded = (mixed ^ (mixed >> 32)) as u32;
    if folded == 0 { 1 } else { folded }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::engine::{GameState, PetKind, Phase, SAVE_SIZE_V2};

    #[test]
    fn both_load_paths_seed_the_dice_alike() {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s.age_ticks = 12_345;
        s.generation = 3;
        let v3 = s.to_bytes();
        let mut v2 = [0u8; SAVE_SIZE_V2];
        v2.copy_from_slice(&v3[..SAVE_SIZE_V2]);
        v2[0] = 2;
        let a = GameState::from_bytes(&v3).unwrap();
        let b = GameState::from_bytes_v2(&v2).unwrap();
        assert_eq!(a.demo_rng, b.demo_rng);
        assert_eq!(a.demo_rng, seed_rng(12_345, 3));
        assert_ne!(a.demo_rng, 0);
    }
}

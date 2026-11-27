//! The pet's condition as the fight sees it (plan 5, spec 15.7 and 18.4).
//!
//! [`Body`] is the pet in whole percent; [`derive`] turns it into a
//! [`Snapshot`]: six fighting numbers and twenty condition flags. Port of
//! the prototype's `battle::derive`, integer math with truncating division.
//! The balance numbers and the need bands are read from `thresholds` (so a
//! Casual or custom band moves the fight with it); the slopes and divisors
//! of the formula are structure and stay literals.

use super::needs::SORE_PCT;
use super::{GameState, regime, thresholds as t, trait_pct};

/// The pet as the fight sees it, every field in whole percent (traits 0..=100).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Body {
    pub fit: i32,
    pub energy: i32,
    pub booze: i32,
    pub mood: i32,
    pub health: i32,
    pub strain: i32,
    pub full: i32,
    pub water: i32,
    pub fun: i32,
    pub dirt: i32,
    pub weight: i32,
    pub vit: i32,
    pub cur: i32,
    pub res: i32,
    pub rested: bool,
    pub injured: bool,
    pub groggy: bool,
    pub hangover: bool,
    pub craving: bool,
    pub deprived: bool,
    pub demo: bool,
}

impl Body {
    /// The prototype's average pet (`Cond::BASE`).
    pub const REFERENCE: Body = Body {
        fit: 60,
        energy: 70,
        booze: 0,
        mood: 75,
        health: 85,
        strain: 0,
        full: 55,
        water: 60,
        fun: 50,
        dirt: 30,
        weight: 50,
        vit: 50,
        cur: 50,
        res: 50,
        rested: false,
        injured: false,
        groggy: false,
        hangover: false,
        craving: false,
        deprived: false,
        demo: false,
    };

    /// The pet's state now: each level as whole percent, the traits as
    /// percent, a flag for each running timer.
    pub fn of(s: &GameState) -> Body {
        let pct = |v: u16| super::level_pct(v) as i32;
        Body {
            fit: pct(s.fitness),
            energy: pct(s.energy),
            booze: pct(s.booze),
            mood: pct(s.mood),
            health: pct(s.health),
            strain: pct(s.strain),
            full: pct(s.fullness),
            water: pct(s.hydration),
            fun: pct(s.fun),
            dirt: pct(s.dirt),
            weight: pct(s.weight),
            vit: trait_pct(s.vitality) as i32,
            cur: trait_pct(s.curiosity) as i32,
            res: trait_pct(s.resilience) as i32,
            rested: s.rested_min > 0,
            injured: s.injured_min > 0,
            groggy: s.groggy_min > 0,
            hangover: s.hangover_min > 0,
            craving: regime::craving(s),
            deprived: s.deprived,
            demo: s.demo.on,
        }
    }
}

/// The twenty conditions the pre-battle screen names. The bit of a factor in
/// [`Flags`] is its number; declaration order is the display priority within
/// each sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Factor {
    // plus (helps)
    Rested,
    Heavy,
    Tipsy,
    Cheerful,
    // minus (hurts)
    Injured,
    Sloppy,
    HungOver,
    TooLight,
    Hungry,
    Thirsty,
    Craving,
    Wired,
    Stuffed,
    Bored,
    Down,
    Groggy,
    WornOut,
    Waterlogged,
    Sore,
    Filthy,
}

impl Factor {
    pub const ALL: [Factor; 20] = [
        Factor::Rested,
        Factor::Heavy,
        Factor::Tipsy,
        Factor::Cheerful,
        Factor::Injured,
        Factor::Sloppy,
        Factor::HungOver,
        Factor::TooLight,
        Factor::Hungry,
        Factor::Thirsty,
        Factor::Craving,
        Factor::Wired,
        Factor::Stuffed,
        Factor::Bored,
        Factor::Down,
        Factor::Groggy,
        Factor::WornOut,
        Factor::Waterlogged,
        Factor::Sore,
        Factor::Filthy,
    ];

    /// The first four factors help the pet; the rest hurt it.
    pub fn helps(self) -> bool {
        (self as u8) < 4
    }

    /// The on-screen word (at most 11 columns, so `+word` fits half a line).
    pub fn text(self) -> &'static str {
        match self {
            Factor::Rested => "well rested",
            Factor::Heavy => "heavy",
            Factor::Tipsy => "tipsy",
            Factor::Cheerful => "cheerful",
            Factor::Injured => "injured",
            Factor::Sloppy => "sloppy",
            Factor::HungOver => "hung over",
            Factor::TooLight => "too light",
            Factor::Hungry => "hungry",
            Factor::Thirsty => "thirsty",
            Factor::Craving => "craving",
            Factor::Wired => "wired",
            Factor::Stuffed => "stuffed",
            Factor::Bored => "bored",
            Factor::Down => "down",
            Factor::Groggy => "groggy",
            Factor::WornOut => "worn out",
            Factor::Waterlogged => "waterlogged",
            Factor::Sore => "sore",
            Factor::Filthy => "filthy",
        }
    }
}

/// Bit set of [`Factor`]s: bit = the factor's number (0..=19), bit 22 is
/// reserved for the star (plan 8, always 0 here), bit 23 is the demo bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Flags(pub u32);

const DEMO_BIT: u32 = 1 << 23;

impl Flags {
    pub fn has(self, f: Factor) -> bool {
        self.0 >> (f as u8) & 1 == 1
    }

    pub fn demo(self) -> bool {
        self.0 & DEMO_BIT != 0
    }

    pub fn with(self, f: Factor) -> Flags {
        Flags(self.0 | 1 << (f as u8))
    }

    pub fn with_demo(self, on: bool) -> Flags {
        Flags(if on {
            self.0 | DEMO_BIT
        } else {
            self.0 & !DEMO_BIT
        })
    }
}

/// Six fighting numbers and the condition flags. This is what travels in a
/// challenge; both badges run the fight from two of these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub power: u8,
    pub guard: u8,
    pub speed: u8,
    pub hp: u8,
    pub stamina: u8,
    pub focus: i8,
    pub flags: Flags,
}

/// Inclusive wire ranges. `derive` clamps into them; a received snapshot
/// outside them is refused (Task 5).
pub const WIRE_POWER: (u8, u8) = (5, 70);
pub const WIRE_GUARD: (u8, u8) = (0, 40);
pub const WIRE_SPEED: (u8, u8) = (5, 100);
pub const WIRE_HP: (u8, u8) = (15, 120);
pub const WIRE_STAMINA: (u8, u8) = (0, 10);
pub const WIRE_FOCUS: (i8, i8) = (-100, 20);

/// Bytes of a snapshot on the wire.
pub const SNAPSHOT_LEN: usize = 9;

/// Flag bits that never travel: 20 and 21 are unused, 22 is the plan-8 star.
const WIRE_FLAG_HOLES: u32 = 0b111 << 20;

fn in_range(v: u8, r: (u8, u8)) -> bool {
    v >= r.0 && v <= r.1
}

impl Snapshot {
    /// power, guard, speed, hp, stamina, focus (i8 as u8), flags as 3 bytes
    /// little-endian.
    pub fn to_wire(&self) -> [u8; SNAPSHOT_LEN] {
        let f = self.flags.0.to_le_bytes();
        [
            self.power,
            self.guard,
            self.speed,
            self.hp,
            self.stamina,
            self.focus as u8,
            f[0],
            f[1],
            f[2],
        ]
    }

    /// None when the input is short, a number is outside its `WIRE_*` range
    /// (so `hp` is never 0 and `stamina` never 255: `fight` divides by `hp`,
    /// `decisive` adds 1 to `stamina`), or flag bits 20, 21 or 22 are set.
    pub fn from_wire(b: &[u8]) -> Option<Snapshot> {
        if b.len() < SNAPSHOT_LEN {
            return None;
        }
        let focus = b[5] as i8;
        let flags = u32::from_le_bytes([b[6], b[7], b[8], 0]);
        let ok = in_range(b[0], WIRE_POWER)
            && in_range(b[1], WIRE_GUARD)
            && in_range(b[2], WIRE_SPEED)
            && in_range(b[3], WIRE_HP)
            && in_range(b[4], WIRE_STAMINA)
            && focus >= WIRE_FOCUS.0
            && focus <= WIRE_FOCUS.1
            && flags & WIRE_FLAG_HOLES == 0;
        ok.then_some(Snapshot {
            power: b[0],
            guard: b[1],
            speed: b[2],
            hp: b[3],
            stamina: b[4],
            focus,
            flags: Flags(flags),
        })
    }

    /// A beacon's six numbers (no condition flags), clamped into the `WIRE_*`
    /// ranges (an old badge's numbers are on another scale), with the demo bit.
    pub fn from_beacon(
        power: u8,
        guard: u8,
        speed: u8,
        hp: u8,
        stamina: u8,
        focus: i8,
        demo: bool,
    ) -> Snapshot {
        Snapshot {
            power: power.clamp(WIRE_POWER.0, WIRE_POWER.1),
            guard: guard.clamp(WIRE_GUARD.0, WIRE_GUARD.1),
            speed: speed.clamp(WIRE_SPEED.0, WIRE_SPEED.1),
            hp: hp.clamp(WIRE_HP.0, WIRE_HP.1),
            stamina: stamina.clamp(WIRE_STAMINA.0, WIRE_STAMINA.1),
            focus: focus.clamp(WIRE_FOCUS.0, WIRE_FOCUS.1),
            flags: Flags(0).with_demo(demo),
        }
    }
}

/// How much of the drink the pet feels: a light pet feels more of it.
/// `booze * (base - weight) / 100`, kept in 0..=100 (a custom base below the
/// weight must not turn the drink into a bonus). The one copy of the rule;
/// the snapshot, its `Tipsy` and `Sloppy` flags and the too-drunk refusal
/// all read it.
pub fn felt(b: &Body) -> i32 {
    felt_with(b, t::BATTLE_FELT_BASE_PCT() as i32)
}

/// [`felt`] for an explicit base (the config's `BATTLE_FELT_BASE_PCT`).
pub fn felt_with(b: &Body, base: i32) -> i32 {
    (b.booze * (base - b.weight) / 100).clamp(0, 100)
}

/// Liquid courage: a tent over the felt drink, its peak at 30, gone at 60.
fn courage(load: i32) -> i32 {
    let peak = t::BATTLE_COURAGE_PEAK_PTS() as i32;
    if load <= 30 {
        load * peak / 30
    } else {
        (peak - (load - 30) * peak / 30).max(0)
    }
}

/// The prototype's `derive`: `[power, guard, speed, hp, stamina, focus]`
/// with the prototype's floors, before the wire clamp. Under the presets only
/// focus has no floor, so only focus can leave its wire range.
pub fn derive_raw(b: &Body) -> [i32; 6] {
    let pts = |v: u8| v as i32;
    let load = felt(b);
    let mut focus_adj = 0;
    let courage = courage(load);

    // POWER: mass, courage, mood, soreness.
    let mut power = pts(t::BATTLE_POWER_BASE_PTS())
        + b.weight * t::BATTLE_POWER_WEIGHT_PCT() as i32 / 100
        + courage;
    let low_mood = t::BATTLE_LOW_MOOD_PCT() as i32;
    if b.mood < low_mood {
        power -= (low_mood - b.mood) / 3;
    } else if b.mood >= t::BATTLE_HIGH_MOOD_PCT() as i32 {
        power += pts(t::BATTLE_HIGH_MOOD_PTS());
    }
    let sore = SORE_PCT as i32;
    if b.strain > sore {
        power -= (b.strain - sore) / 4;
    }

    // GUARD: resilience and mass; a felt drink makes it reckless.
    let guard = b.res / 5 + b.weight * t::BATTLE_GUARD_WEIGHT_PCT() as i32 / 100
        - pts(t::BATTLE_GUARD_BASE_PTS())
        - load / 8;

    // SPEED: fitness, vitality, belly, sleep state. Heavy is slow, light is quick.
    let mut speed = pts(t::BATTLE_SPEED_BASE_PTS())
        + b.fit * t::BATTLE_SPEED_FIT_PCT() as i32 / 100
        + b.vit / 5
        - (b.weight - 50).max(0) * 3 / 5
        + (50 - b.weight).max(0) / 5;
    let full_hi = t::FULL_HI_PCT() as i32;
    if b.full > full_hi {
        speed -= (b.full - full_hi) * 3 / 4 + 4;
    }
    let water_hi = t::WATER_HI_PCT() as i32;
    if b.water > water_hi {
        speed -= (b.water - water_hi) / 2 + 3;
    }
    if b.rested {
        speed += pts(t::BATTLE_RESTED_SPEED_PTS());
    }
    if b.groggy {
        speed -= pts(t::BATTLE_GROGGY_SPEED_PTS());
    }

    // STAMINA: rounds at full strength. Energy times fitness, vitality, belly.
    let mut stamina = 1 + b.energy * (40 + b.fit) / 2600 + (b.vit - 50) / 25;
    let heavy = b.weight > t::WEIGHT_HI_PCT() as i32;
    if heavy {
        stamina -= 1; // excess weight tires
    }
    if b.deprived {
        stamina -= 1;
    }
    if heavy && b.fit < t::BATTLE_HEAVY_UNFIT_FIT_PCT() as i32 {
        stamina -= 1; // and tires an unfit pet twice
    }
    let weight_lo = t::WEIGHT_LO_PCT() as i32;
    if b.weight < weight_lo {
        power -= (weight_lo - b.weight) / 2; // too light to hurt
    }
    if b.hangover {
        speed -= pts(t::BATTLE_HANGOVER_SPEED_PTS());
        focus_adj = -pts(t::BATTLE_HANGOVER_FOCUS_PTS());
        stamina -= 1;
    }
    if b.craving {
        focus_adj -= pts(t::BATTLE_CRAVING_FOCUS_PTS());
        power -= pts(t::BATTLE_CRAVING_POWER_PTS());
    }
    let full_lo = t::FULL_LO_PCT() as i32;
    if b.full < full_lo {
        stamina -= 1 + (full_lo - b.full) / 15;
    }
    let water_lo = t::WATER_LO_PCT() as i32;
    if b.water < water_lo {
        stamina -= 1 + (water_lo - b.water) / 15;
    }
    if b.rested {
        stamina += 1;
    }

    // FOCUS (hit-chance points): stimulation band, drink, curiosity, mood, grime.
    let mut focus = (b.cur - 50) / 5 + focus_adj;
    let fun_lo = t::FUN_LO_PCT() as i32;
    if b.fun < fun_lo {
        focus -= (fun_lo - b.fun) / 2; // bored: not into it
    }
    let fun_hi = t::FUN_HI_PCT() as i32;
    if b.fun > fun_hi {
        focus -= b.fun - fun_hi; // wired: wild swings
    }
    focus -= load / 6; // any drink costs a little aim
    let sloppy = t::BATTLE_SLOPPY_PCT() as i32;
    if load > sloppy {
        focus -= (load - sloppy) * 5 / 4;
    }
    if b.mood < low_mood {
        focus -= (low_mood - b.mood) / 4;
    }
    let dirt_hi = t::DIRT_HI_PCT() as i32;
    if b.dirt > dirt_hi {
        focus -= (b.dirt - dirt_hi) / 4;
    }
    if b.water < water_lo {
        focus -= (water_lo - b.water) / 3;
    }
    if b.groggy {
        focus -= pts(t::BATTLE_GROGGY_FOCUS_PTS());
    }
    if b.rested {
        focus += pts(t::BATTLE_RESTED_FOCUS_PTS());
    }

    // HP: health, vitality, mass.
    let hp = (pts(t::BATTLE_HP_BASE_PTS()) + b.vit / 2) * b.health / 100
        + b.weight * t::BATTLE_HP_WEIGHT_PCT() as i32 / 100
        - 3;

    // Puncher's chance: nobody is entirely harmless (owner decisions 18 and 23).
    let mut power = power.max(pts(t::BATTLE_POWER_FLOOR_PTS()));
    let hp = hp.max(pts(t::BATTLE_HP_FLOOR_PTS()));
    let stamina = stamina.max(pts(t::BATTLE_STAMINA_FLOOR_PTS()));
    let guard = guard.max(pts(t::BATTLE_GUARD_FLOOR_PTS()));
    let mut speed = speed;
    if b.injured {
        let pct = t::BATTLE_INJURED_PCT() as i32;
        power = power * pct / 100;
        speed = speed * pct / 100;
    }
    [
        power.max(5),
        guard.max(0),
        speed.max(5),
        hp.max(15),
        stamina.max(0),
        focus,
    ]
}

/// The condition flags of a body.
fn flags_of(b: &Body, load: i32) -> Flags {
    let (full_lo, full_hi) = (t::FULL_LO_PCT() as i32, t::FULL_HI_PCT() as i32);
    let (water_lo, water_hi) = (t::WATER_LO_PCT() as i32, t::WATER_HI_PCT() as i32);
    let (fun_lo, fun_hi) = (t::FUN_LO_PCT() as i32, t::FUN_HI_PCT() as i32);
    let sloppy = t::BATTLE_SLOPPY_PCT() as i32;
    let on = [
        (Factor::Rested, b.rested),
        (Factor::Heavy, b.weight > t::WEIGHT_HI_PCT() as i32),
        (Factor::Tipsy, courage(load) > 0 && load <= sloppy),
        (Factor::Cheerful, b.mood >= t::BATTLE_HIGH_MOOD_PCT() as i32),
        (Factor::Injured, b.injured),
        (Factor::Sloppy, load > sloppy),
        (Factor::HungOver, b.hangover),
        (Factor::TooLight, b.weight < t::WEIGHT_LO_PCT() as i32),
        (Factor::Hungry, b.full < full_lo),
        (Factor::Thirsty, b.water < water_lo),
        (Factor::Craving, b.craving),
        (Factor::Wired, b.fun > fun_hi),
        (Factor::Stuffed, b.full > full_hi),
        (Factor::Bored, b.fun < fun_lo),
        (Factor::Down, b.mood < t::BATTLE_LOW_MOOD_PCT() as i32),
        (Factor::Groggy, b.groggy),
        (Factor::WornOut, b.deprived),
        (Factor::Waterlogged, b.water > water_hi),
        (Factor::Sore, b.strain > SORE_PCT as i32),
        (Factor::Filthy, b.dirt > t::DIRT_HI_PCT() as i32),
    ];
    let mut flags = Flags::default().with_demo(b.demo);
    for (f, set) in on {
        if set {
            flags = flags.with(f);
        }
    }
    flags
}

/// The snapshot of a body: [`derive_raw`] clamped into the wire ranges,
/// plus the condition flags.
pub fn derive(b: &Body) -> Snapshot {
    let r = derive_raw(b);
    let wire = |v: i32, (lo, hi): (u8, u8)| v.clamp(lo as i32, hi as i32) as u8;
    Snapshot {
        power: wire(r[0], WIRE_POWER),
        guard: wire(r[1], WIRE_GUARD),
        speed: wire(r[2], WIRE_SPEED),
        hp: wire(r[3], WIRE_HP),
        stamina: wire(r[4], WIRE_STAMINA),
        focus: r[5].clamp(WIRE_FOCUS.0 as i32, WIRE_FOCUS.1 as i32) as i8,
        flags: flags_of(b, felt(b)),
    }
}

// ---------------------------------------------------------------------
// The fight. Everything from `fight` to `win_chance` is wire contract:
// both badges run it from the same two snapshots, so it is `const` and
// reads no config.
// ---------------------------------------------------------------------

pub const ROUNDS: u8 = 10;
/// hit% = clamp(HIT_BASE + (speedA' - speedD') / 3 + focusA, HIT_MIN, HIT_MAX)
pub const HIT_BASE: i32 = 72;
pub const HIT_MIN: i32 = 15;
pub const HIT_MAX: i32 = 95;
/// Past its stamina a pet strikes at this percent of its speed and power.
pub const TIRED_PCT: i32 = 60;
pub const DAMAGE_MIN: i32 = 3;
pub const CHANCE_FIGHTS: u32 = 64;

/// How a fight ended. `a` is the challenger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub a_won: bool,
    pub a_hp_pct: u8,
    pub b_hp_pct: u8,
    pub rounds: u8,
}

fn strike(att: &Snapshot, def: &Snapshot, round: i32, s: &mut u32) -> i32 {
    let tired = |p: &Snapshot| round > p.stamina as i32;
    let slow = |p: &Snapshot| {
        if tired(p) {
            p.speed as i32 * TIRED_PCT / 100
        } else {
            p.speed as i32
        }
    };
    let hit = (HIT_BASE + (slow(att) - slow(def)) / 3 + att.focus as i32).clamp(HIT_MIN, HIT_MAX);
    *s = crate::platform::xorshift32(*s);
    if (*s % 100) as i32 >= hit {
        return 0;
    }
    let pw = if tired(att) {
        att.power as i32 * TIRED_PCT / 100
    } else {
        att.power as i32
    };
    let base = (pw - def.guard as i32 / 2).max(DAMAGE_MIN);
    *s = crate::platform::xorshift32(*s);
    base * (85 + (*s % 31) as i32) / 100
}

/// The fight: up to [`ROUNDS`] rounds, the faster pet strikes first.
/// `a` is the challenger.
pub fn fight(seed: u32, a: &Snapshot, b: &Snapshot) -> Outcome {
    let mut s = seed | 1;
    let (mut ha, mut hb) = (a.hp as i32, b.hp as i32);
    let mut round = 0;
    while ha > 0 && hb > 0 && round < ROUNDS as i32 {
        round += 1;
        let a_first = a.speed > b.speed
            || (a.speed == b.speed && {
                s = crate::platform::xorshift32(s);
                s & 1 == 0
            });
        if a_first {
            hb -= strike(a, b, round, &mut s);
            if hb > 0 {
                ha -= strike(b, a, round, &mut s);
            }
        } else {
            ha -= strike(b, a, round, &mut s);
            if ha > 0 {
                hb -= strike(a, b, round, &mut s);
            }
        }
    }
    let pa = ha.max(0) * 100 / a.hp as i32;
    let pb = hb.max(0) * 100 / b.hp as i32;
    let a_won = if ha > 0 && hb <= 0 {
        true
    } else if hb > 0 && ha <= 0 {
        false
    } else if pa != pb {
        pa > pb
    } else {
        s = crate::platform::xorshift32(s);
        s & 1 == 0
    };
    Outcome {
        a_won,
        a_hp_pct: pa as u8,
        b_hp_pct: pb as u8,
        rounds: round as u8,
    }
}

/// Seed of a live fight: both badges compute it from the same four values,
/// challenger first: `xorshift32(xorshift32(nonces ^ ids ^ 0x9E37_79B9))`.
/// A fallback fight uses `nonce_b = 0`. `fight` ORs 1 into the seed itself.
pub fn seed(nonce_a: u16, nonce_b: u16, id_a: [u8; 2], id_b: [u8; 2]) -> u32 {
    let ids = (u16::from_le_bytes(id_a) as u32) << 16 | u16::from_le_bytes(id_b) as u32;
    let x = ((nonce_a as u32) << 16 | nonce_b as u32) ^ ids ^ 0x9E37_79B9;
    crate::platform::xorshift32(crate::platform::xorshift32(x))
}

/// Seed of the k-th chance fight (wire contract).
pub fn chance_seed(k: u32) -> u32 {
    k.wrapping_mul(2_654_435_761).wrapping_add(7)
}

/// The challenger's chance in percent, rounded to the nearest 5 (ties up):
/// 64 fights on `chance_seed(0..64)`. The target shows `100 - win_chance`.
pub fn win_chance(a: &Snapshot, b: &Snapshot) -> u8 {
    let w = (0..CHANCE_FIGHTS)
        .filter(|&k| fight(chance_seed(k), a, b).a_won)
        .count() as u32;
    (((w * 100 + CHANCE_FIGHTS / 2) / CHANCE_FIGHTS + 2) / 5 * 5) as u8
}

/// How the pet feels about a fight against an average pet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Top,
    In,
    Off,
    Poor,
}

impl Form {
    pub const ALL: [Form; 4] = [Form::Top, Form::In, Form::Off, Form::Poor];

    pub fn text(self) -> &'static str {
        match self {
            Form::Top => "top form",
            Form::In => "in form",
            Form::Off => "off form",
            Form::Poor => "poor form",
        }
    }
}

/// The pet's own chance against `derive(&Body::REFERENCE)` as `a`, read
/// through the per-badge form keys (not part of the wire contract).
pub fn form(own: &Snapshot) -> Form {
    let c = win_chance(own, &derive(&Body::REFERENCE));
    if c >= t::BATTLE_FORM_TOP_PCT() {
        Form::Top
    } else if c >= t::BATTLE_FORM_IN_PCT() {
        Form::In
    } else if c >= t::BATTLE_FORM_OFF_PCT() {
        Form::Off
    } else {
        Form::Poor
    }
}

/// Up to two factor lines for one side: the first helping and the first
/// hurting factor in `Factor` order; with none of one sign, the first two
/// of the other.
pub fn factors(s: &Snapshot) -> [Option<Factor>; 2] {
    let mut helps = Factor::ALL
        .iter()
        .copied()
        .filter(|&f| f.helps() && s.flags.has(f));
    let mut hurts = Factor::ALL
        .iter()
        .copied()
        .filter(|&f| !f.helps() && s.flags.has(f));
    let (h, u) = (helps.next(), hurts.next());
    match (h, u) {
        (Some(_), Some(_)) => [h, u],
        (Some(_), None) => [h, helps.next()],
        (None, Some(_)) => [u, hurts.next()],
        (None, None) => [None, None],
    }
}

/// What stood out in a lost fight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decisive {
    Factor(Factor),
    OutOfSteam(u8),
}

/// Why the loser lost, if one thing stands out: its first hurting factor;
/// else, if it fought past its stamina, `OutOfSteam(stamina + 1)`; else
/// `None`. Returns `(loser_is_a, why)`.
pub fn decisive(o: &Outcome, a: &Snapshot, b: &Snapshot) -> Option<(bool, Decisive)> {
    let loser_is_a = !o.a_won;
    let l = if loser_is_a { a } else { b };
    if let Some(f) = Factor::ALL
        .iter()
        .copied()
        .find(|&f| !f.helps() && l.flags.has(f))
    {
        return Some((loser_is_a, Decisive::Factor(f)));
    }
    if o.rounds > l.stamina {
        return Some((loser_is_a, Decisive::OutOfSteam(l.stamina + 1)));
    }
    None
}

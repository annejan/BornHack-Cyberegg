//! v3 battle prototype. Integer math. All inputs are percentages 0..=100.
#[derive(Clone, Copy, Debug)]
pub struct Cond { pub deprived: bool, pub weight: i32, pub hangover: bool, pub craving: bool, pub fit: i32, pub energy: i32, pub load: i32, pub mood: i32, pub health: i32, pub strain: i32, pub full: i32, pub hyd: i32, pub stim: i32, pub dirt: i32,
                  pub rested: bool, pub injured: bool, pub groggy: bool, pub vit: i32, pub cur: i32, pub res: i32 }
impl Cond { pub const BASE: Cond = Cond { deprived: false, weight: 50, hangover: false, craving: false, fit: 60, energy: 70, load: 0, mood: 75, health: 85, strain: 0, full: 55, hyd: 60, stim: 50, dirt: 30, rested: false, injured: false, groggy: false, vit: 50, cur: 50, res: 50 }; }

/// Five battle numbers. `focus` is a hit-chance modifier in percentage points.
#[derive(Clone, Copy, Debug)]
pub struct Combat { pub power: i32, pub guard: i32, pub speed: i32, pub hp: i32, pub stamina: i32, pub focus: i32 }

pub fn derive(c0: &Cond) -> Combat {
    // a lean pet feels its drink more: effective load = load * (50 + fit) / 100
    // a light pet feels its drink more: effective booze = booze * (150 - weight) / 100
    let c = &Cond { load: (c0.load * (150 - c0.weight) / 100).min(100), ..*c0 };
    let mut focus_adj = 0;
    let mass = c.weight;                                      // mass is body weight, not lack of fitness                                   // heavy = low fitness
    // liquid courage: tent, peak +8 at load 30, gone at 60
    let courage = if c.load <= 30 { c.load * 8 / 30 } else { (8 - (c.load - 30) * 8 / 30).max(0) };
    // POWER: mass, courage, mood, soreness
    let mut power = 22 + mass * 28 / 100 + courage;
    if c.mood < 40 { power -= (40 - c.mood) / 3; } else if c.mood >= 80 { power += 3; }
    if c.strain > 40 { power -= (c.strain - 40) / 4; }
    // GUARD: mass and resilience
    let guard = c.res / 5 + mass * 26 / 100 - 7 - c.load / 8;      // drink makes it reckless
    // SPEED: fitness, vitality, belly, sleep state
    let mut speed = 25 + c.fit * 40 / 100 + c.vit / 5 - (c.weight - 50).max(0) * 3 / 5 + (50 - c.weight).max(0) / 5;   // heavy is slow, light is quick
    if c.full > 78 { speed -= (c.full - 78) * 3 / 4 + 4; }
    if c.hyd > 83 { speed -= (c.hyd - 83) / 2 + 3; }           // waterlogged
    if c.rested { speed += 5; }
    if c.groggy { speed -= 10; }
    // STAMINA (rounds at full strength): energy x fitness, vitality, fed, rested
    let mut stamina = 1 + c.energy * (40 + c.fit) / 2600 + (c.vit - 50) / 25;
    if c.weight > 63 { stamina -= 1; }
    if c.deprived { stamina -= 1; }                        // excess weight tires
    if c.weight > 63 && c.fit < 50 { stamina -= 1; }          // and tires an unfit pet twice
    if c.weight < 38 { power -= (38 - c.weight) / 2; }        // too light to hurt
    if c.hangover { speed -= 8; focus_adj = -12; stamina -= 1; }
    if c.craving { focus_adj -= 10; power -= 3; }
    if c.full < 38 { stamina -= 1 + (38 - c.full) / 15; }
    if c.hyd < 38 { stamina -= 1 + (38 - c.hyd) / 15; }
    if c.rested { stamina += 1; }
    // FOCUS (hit-chance points): stimulation band, drink, curiosity, mood, grime
    let mut focus = (c.cur - 50) / 5 + focus_adj;
    if c.stim < 33 { focus -= (33 - c.stim) / 2; }           // bored: not into it
    if c.stim > 73 { focus -= c.stim - 73; }                 // wired: wild swings
    focus -= c.load / 6;                                     // any drink costs a little aim
    if c.load > 40 { focus -= (c.load - 40) * 5 / 4; }       // sloppy
    if c.mood < 40 { focus -= (40 - c.mood) / 4; }
    if c.dirt > 58 { focus -= (c.dirt - 58) / 4; }
    if c.hyd < 38 { focus -= (38 - c.hyd) / 3; }
    if c.groggy { focus -= 5; }
    if c.rested { focus += 5; }
    // HP: health, vitality, mass
    let hp = (45 + c.vit / 2) * c.health / 100 + mass * 25 / 100 - 3;
    // puncher's chance: nobody is entirely harmless (owner decision 18 and 23)
    power = power.max(33); let hp = hp.max(60); stamina = stamina.max(1); let guard = guard.max(14);
    if c.injured { power = power * 6 / 10; speed = speed * 6 / 10; }
    Combat { power: power.max(5), guard: guard.max(0), speed: speed.max(5), hp: hp.max(15), stamina: stamina.max(0), focus }
}

fn rng(s: &mut u32) -> u32 { let mut x = *s; x ^= x << 13; x ^= x >> 17; x ^= x << 5; *s = x; x }

pub struct Outcome { pub a_won: bool, pub a_hp_pct: i32, pub b_hp_pct: i32, pub rounds: i32 }

fn strike(att: &Combat, def: &Combat, round: i32, s: &mut u32) -> i32 {
    let tired_a = round > att.stamina; let tired_d = round > def.stamina;
    let spd_a = if tired_a { att.speed * 60 / 100 } else { att.speed };
    let spd_d = if tired_d { def.speed * 60 / 100 } else { def.speed };
    let hit = (72 + (spd_a - spd_d) / 3 + att.focus).clamp(15, 95);
    if (rng(s) % 100) as i32 >= hit { return 0; }
    let pw = if tired_a { att.power * 60 / 100 } else { att.power };
    let base = (pw - def.guard / 2).max(3);
    base * (85 + (rng(s) % 31) as i32) / 100
}

pub fn fight(seed: u32, a: &Combat, b: &Combat) -> Outcome {
    let mut s = seed | 1;
    let (mut ha, mut hb) = (a.hp, b.hp);
    let mut round = 0;
    while ha > 0 && hb > 0 && round < 10 {
        round += 1;
        let a_first = a.speed > b.speed || (a.speed == b.speed && rng(&mut s) & 1 == 0);
        if a_first { hb -= strike(a, b, round, &mut s); if hb > 0 { ha -= strike(b, a, round, &mut s); } }
        else { ha -= strike(b, a, round, &mut s); if ha > 0 { hb -= strike(a, b, round, &mut s); } }
    }
    let (pa, pb) = (ha.max(0) * 100 / a.hp, hb.max(0) * 100 / b.hp);
    let a_won = if ha > 0 && hb <= 0 { true } else if hb > 0 && ha <= 0 { false } else if pa != pb { pa > pb } else { rng(&mut s) & 1 == 0 };
    Outcome { a_won, a_hp_pct: pa, b_hp_pct: pb, rounds: round }
}

pub fn builds() -> Vec<(&'static str, Cond)> {
    let b = Cond::BASE;
    let fit = Cond { fit: 90, energy: 75, mood: 85, health: 95, ..b };
    vec![
        ("fit, normal weight", fit),
        ("fit, one beer", Cond { load: 15, ..fit }),
        ("thin and fit", Cond { weight: 30, ..fit }),
        ("sumo (heavy, fit)", Cond { weight: 80, fit: 80, energy: 75, mood: 85, health: 90, ..b }),
        ("sumo, two beers", Cond { weight: 80, fit: 80, energy: 75, mood: 85, health: 90, load: 30, ..b }),
        ("well rested", Cond { fit: 75, energy: 95, mood: 85, health: 90, rested: true, ..b }),
        ("average", b),
        ("fat and unfit", Cond { weight: 85, fit: 15, energy: 65, mood: 70, health: 55, ..b }),
        ("fat, unfit, three beers", Cond { weight: 85, fit: 15, energy: 65, mood: 70, health: 55, load: 45, ..b }),
        ("thin and unfit", Cond { weight: 25, fit: 20, health: 55, mood: 65, ..b }),
        ("wasted", Cond { load: 70, energy: 60, ..b }),
        ("dependent, sober (craving)", Cond { craving: true, mood: 55, ..b }),
        ("wired", Cond { stim: 92, energy: 55, ..b }),
        ("bored", Cond { stim: 8, mood: 55, ..b }),
        ("hungry", Cond { full: 18, mood: 60, ..b }),
        ("stuffed", Cond { full: 95, energy: 60, ..b }),
        ("thirsty", Cond { hyd: 25, ..b }),
        ("exhausted", Cond { energy: 15, mood: 55, ..b }),
        ("hung over", Cond { hangover: true, energy: 45, mood: 50, hyd: 30, ..b }),
        ("injured", Cond { injured: true, strain: 50, mood: 55, ..b }),
    ]
}

pub fn matrix() {
    let bs = builds();
    println!("\nCombat stats per build:");
    println!("| build | power | guard | speed | hp | stamina rounds | focus |\n|---|---|---|---|---|---|---|");
    for (n, c) in &bs { let k = derive(c); println!("| {n} | {} | {} | {} | {} | {} | {} |", k.power, k.guard, k.speed, k.hp, k.stamina, k.focus); }
    println!("\nWin rate of row against column, % (2000 fights each, row challenges):");
    print!("| row \\ col |"); for (n, _) in &bs { print!(" {n} |"); } println!(" avg |");
    print!("|---|"); for _ in &bs { print!("---|"); } println!("---|");
    for (i, (n, a)) in bs.iter().enumerate() {
        print!("| {n} |"); let mut tot = 0; let mut cnt = 0;
        for (j, (_, b)) in bs.iter().enumerate() {
            let (ka, kb) = (derive(a), derive(b));
            let mut w = 0;
            for k in 0..2000u32 { if fight(k.wrapping_mul(2654435761).wrapping_add((i * 131 + j * 17) as u32 + 7), &ka, &kb).a_won { w += 1; } }
            let p = w * 100 / 2000; if i != j { tot += p; cnt += 1; }
            print!(" {p} |");
        }
        println!(" {} |", tot / cnt);
    }
}

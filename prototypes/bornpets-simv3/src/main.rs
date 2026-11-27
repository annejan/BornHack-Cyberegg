//! BornPets v3 "comfort band" model prototype. Integer math only.
//! Step = 1 minute (6 engine ticks). Scale 0..=65535, P = 1 %.
mod battle;
const P: i32 = 655;
const MAX: i32 = 65535;
fn pc(v: i32) -> i32 { v / P }
fn cl(v: i32) -> i32 { v.clamp(0, MAX) }

// ---- tunables (per minute unless noted) ----
const FULL_DECAY: i32 = 54;       // 100 % in 20 h awake
const FULL_DECAY_SLEEP: i32 = 30;
const ENERGY_DRAIN: i32 = 36;     // a 16 h day costs 53 % before actions
const SLEEP_GAIN: i32 = 120;      // 0 -> 100 % in 9.1 h at full quality
const HYD_DECAY: i32 = 45;        // 66 % per 16 h day
const HYD_DECAY_SLEEP: i32 = 20;  // 15 % per 8 h night
const HYD_LO: i32 = 38 * P; const HYD_HI: i32 = 83 * P;
const STIM_DECAY: i32 = 40;       // 59 % per 16 h day
const LOAD_DECAY: i32 = 36;       // 79 %/day (same as DRUNK_SOBER_RATE)
const STRAIN_DECAY: i32 = 80;     // 35 % in 4.8 h
const FIT_DECAY: i32 = 4;         // 8.8 %/day above 50 %, half below, quarter below 25 %
const MOOD_STEP: i32 = 120;       // mood follows its target at 11 %/h
// bands
const FULL_LO: i32 = 38 * P; const FULL_HI: i32 = 78 * P;
const W_LO: i32 = 38 * P; const W_HI: i32 = 63 * P; const FIT_LO: i32 = 43 * P; const DIRT_HI: i32 = 58 * P;
const EN_LO: i32 = 33 * P;
const STIM_LO: i32 = 33 * P; const STIM_HI: i32 = 73 * P;
const LOAD_HI: i32 = 38 * P; const LOAD_DANGER: i32 = 70 * P;
const STRAIN_INJ: i32 = 60 * P;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Food { Salad, Apple, Frik, Pizza, Cake }
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drink { Water, Cola, Beer, Wine, Whiskey }
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act { Station(u8), Relax, Insulin, Ozempic, Rehab, Work, Wash, Eat(Food), Drink(Drink), Play, Mini(i32), Sport, Sleep, Wake, Med, Friend }

#[derive(Clone)]
struct Pet {
    full: i32, hyd: i32, energy: i32, stim: i32, fit: i32, dirt: i32, annoyed: u32, load: i32, strain: i32, mood: i32, health: i32,
    sleeping: bool, sleep_min: i32, oversleep: i32, rested_min: i32, injured_min: i32, groggy_min: i32,
    good_sleep_acc: i32,
    cd_med: i32,
    no_groggy: bool, stations: u32, walk_next: i64, night: bool, walks: bool, bill_count: u32, reversed: u32, deprived: bool, short_nights: i32, good_nights: i32, dep_min: i64, declined_min: i32, rev_acc: i32, bed_at: i64, up_at: i64, refused: u32,
    habits: bool, weight: i32, hangover_min: i32, peak40: bool, drunk_acc: i32, dependent: bool, rehab_min: i32, recovering_min: i32, junk_min: i32,
    diabetic: bool, over_acc: i32, ins_min: i32, ozempic_min: i32, cd_ozempic: i32, cd_work: i32, relapses: u32, dep_days: i64,
    evict_min: i32, evicted: bool, notices: [i64; 3], week_doses: i32, money: i32, econ: bool, hard: bool, insured_min: i32, week_care: i32, week_left: i32, ins_doses: i32, bill_due: i32, bill_amt: i32, auth_due: i32, auth_what: u8, auth_ok: u8,
    spent_care: i32, spent_food: i32, earned: i32, bills: i32, min_money: i32, denied: u32,
    t: i64, delta: bool, segs: u64, obs_changes: u32, last_obs: u64, last_wake: i64, cap30: u32, cap120: u32,
    demo_next: i64, whim_next: i64, lazy_next: i64, nap_until: i64, wake_at: i64, sport_at: i64, armed: bool,
    leaving_min: i32, gone: bool, demo: bool, lazy: i32, rng: u32, nap_left: i32, auto_acts: u32, band: [i64; 5],
    // stats
    injuries: u32, grumpy: u32, groggy: u32, presses: u32, mood_sum: i64, mins: i64, good_min: i64, min_health: i32, leaving_total: i64,
}
impl Pet {
    fn new() -> Self { Pet { hyd: 65*P, dirt: 10*P, annoyed: 0, full: 60*P, energy: 80*P, stim: 50*P, fit: 60*P, load: 0, strain: 0, mood: 70*P, health: 85*P,
        no_groggy: false, stations: 0, walk_next: 0, night: false, walks: false, bill_count: 0, reversed: 0, deprived: false, short_nights: 0, good_nights: 0, dep_min: 0, declined_min: 0, rev_acc: 0, bed_at: 480 + 840 + 45, up_at: 0, refused: 0,
        habits: true, weight: 50*P, hangover_min: 0, peak40: false, drunk_acc: 0, dependent: false, rehab_min: 0, recovering_min: 0, junk_min: 0,
        diabetic: false, over_acc: 0, ins_min: 0, ozempic_min: 0, cd_ozempic: 0, cd_work: 0, relapses: 0, dep_days: 0,
        evict_min: 0, evicted: false, notices: [0; 3], week_doses: 0, money: 100, econ: false, hard: false, insured_min: 0, week_care: 0, week_left: 10080, ins_doses: 0, bill_due: 0, bill_amt: 0, auth_due: 0, auth_what: 0, auth_ok: 0,
        spent_care: 0, spent_food: 0, earned: 0, bills: 0, min_money: 100, denied: 0,
        sleeping: false, sleep_min: 0, oversleep: 0, rested_min: 0, injured_min: 0, groggy_min: 0, good_sleep_acc: 0, cd_med: 0,
        t: 480, delta: false, segs: 0, obs_changes: 0, last_obs: 0, last_wake: 480, cap30: 0, cap120: 0,
        demo_next: 0, whim_next: 480 + 240, lazy_next: 0, nap_until: 0, wake_at: 0, sport_at: 0, armed: false,
        leaving_min: 0, gone: false, demo: false, lazy: 0, rng: 0x1234_5678, nap_left: 0, auto_acts: 0, band: [0; 5], injuries: 0, grumpy: 0, groggy: 0, presses: 0, mood_sum: 0, mins: 0, good_min: 0, min_health: MAX, leaving_total: 0 } }

    fn sleep_quality_pm(&self) -> i32 { // per-mille
        let unsettled = self.load > 30*P || self.full > 85*P || self.full < 25*P || self.stim > STIM_HI || self.hyd < 25*P || self.hyd > 90*P || self.rehab_min > 0;
        if unsettled { 600 } else { 1000 }
    }
    fn mood_target(&self) -> i32 {
        let mut pen = 0;
        if self.full < FULL_LO { pen += (FULL_LO - self.full) * 3 / 2; }
        if self.full > FULL_HI { pen += self.full - FULL_HI; }
        if self.hyd < HYD_LO { pen += (HYD_LO - self.hyd) * 3 / 2; }
        if self.hyd > HYD_HI { pen += self.hyd - HYD_HI; }
        if self.energy < EN_LO && !self.sleeping { pen += (EN_LO - self.energy) * 3 / 2; }
        if self.stim < STIM_LO && !self.sleeping { pen += self.stim_low_pen(); }
        if self.stim > STIM_HI { pen += (self.stim - STIM_HI) * 3 / 2; }
        if self.load > LOAD_HI { pen += self.load - LOAD_HI; }
        if self.injured_min > 0 { pen += 20 * P; }
        if self.groggy_min > 0 { pen += 10 * P; }
        if self.health < 50 * P { pen += 50 * P - self.health; }
        if self.fit < FIT_LO { pen += (FIT_LO - self.fit) / 2; }
        if self.dirt > DIRT_HI { pen += self.dirt - DIRT_HI; }
        if self.weight > W_HI { pen += (self.weight - W_HI) / 2; }
        if self.weight < W_LO { pen += (W_LO - self.weight) / 2; }
        if self.deprived { pen += 10 * P; }
        if self.declined_min > 0 { pen += 3 * P; }
        if self.hangover_min > 0 { pen += 15 * P; }
        if self.rehab_min > 0 { pen += 20 * P; }                                           // withdrawal
        else if self.dependent && self.load < 10 * P && !self.sleeping { pen += 15 * P; }  // craving
        if self.money < 0 { pen += 5 * P; }                                                // collections calls
        cl(MAX - pen)
    }
    fn stim_low_pen(&self) -> i32 { STIM_LO - self.stim }

    /// Thirst multiplier in percent, stepwise in treat load (was continuous: not delta-friendly).
    fn ecap(&self) -> i32 { if self.deprived { 70 * P } else { MAX } }
    fn hyd_mult(&self) -> i32 { let l = pc(self.load); if l < 15 { 100 } else if l < 40 { 110 } else if l < 70 { 125 } else { 140 } }
    /// Health ceiling, stepwise in fitness (was 50 % + fit/2).
    fn ceiling(&self) -> i32 { let f = pc(self.fit); P * (if f < 25 { 55 } else if f < 50 { 65 } else if f < 70 { 80 } else { 100 } - if self.diabetic && self.ins_min == 0 { 20 } else { 0 }) }
    /// Per-minute rates of the eight needs in the current regime: full, hyd, energy, stim, dirt, load, strain, fit.
    fn rates(&self) -> [i32; 9] {
        let hm = self.hyd_mult();
        let (full, hyd, energy, stim);
        if self.sleeping {
            full = -FULL_DECAY_SLEEP; hyd = -HYD_DECAY_SLEEP * hm / 100;
            energy = if self.energy < self.ecap() { SLEEP_GAIN * self.sleep_quality_pm() / 1000 } else { 0 };
            stim = if self.stim > 40 * P { -STIM_DECAY } else { 0 };
        } else {
            full = -FULL_DECAY; hyd = -HYD_DECAY * hm / 100;
            let mut m = 1000;
            if self.fit < FIT_LO { m = m * 125 / 100; }
            if self.fit >= 70 * P { m = m * 90 / 100; }
            if self.load > LOAD_HI { m = m * 150 / 100; }
            if self.stim > STIM_HI { m = m * 150 / 100; }
            if self.hangover_min > 0 { m = m * 125 / 100; }
            if self.weight > W_HI || self.weight < W_LO { m = m * 115 / 100; }
            energy = -ENERGY_DRAIN * m / 1000; stim = -STIM_DECAY;
        }
        let mut load = if self.hyd < HYD_LO { LOAD_DECAY * 2 / 3 } else { LOAD_DECAY };
        if self.dependent { load = load * 2 / 3; }
        let mut fit = if self.fit > 50 * P { FIT_DECAY } else if self.fit > 25 * P { FIT_DECAY / 2 } else { 1 };
        if self.weight > W_HI { fit = fit * 3 / 2; }                       // carrying excess weight
        // metabolism pulls weight back toward the band; an empty or stuffed belly shifts it
        let weight = (if self.weight > W_HI { -3 } else if self.weight < W_LO { 0 } else { -1 })
                   + (if self.full < FULL_LO { -3 } else if self.full > FULL_HI { 3 } else { 0 });
        [full, hyd, energy, stim, 8, -load, -STRAIN_DECAY, -fit, weight]
    }
    fn levels(&self) -> [i32; 9] { [self.full, self.hyd, self.energy, self.stim, self.dirt, self.load, self.strain, self.fit, self.weight] }
    /// Thresholds (percent) at which some rate, penalty, health term or demo rule changes, per need.
    fn thresholds(&self, i: usize) -> Vec<i32> {
        let lz = if self.demo { self.lazy } else { 0 };
        let mut v: Vec<i32> = match i {
            0 => vec![15, 25, 38, 78, 85, 95],                 // fullness
            1 => vec![15, 25, 38, 83, 85, 90, 95],             // hydration
            2 => vec![0, 10, 33, 50, 70, 85, 100],             // energy
            3 => vec![33, 40, 73],                             // stimulation
            4 => vec![25, 58, 85],                             // dirt
            5 => vec![10, 15, 30, 38, 70],                     // booze
            8 => vec![20, 38, 63, 65, 80],                     // weight
            6 => vec![15, 40, 60],                             // strain
            _ => vec![25, 43, 50, 60, 61, 70],                 // fitness
        };
        if self.demo { v.extend(match i { 0 => vec![18, 20, 36 - lz, 41], 1 => vec![22, 48 - lz, 39], 2 => vec![12, 26, 41, 55, 97],
            3 => vec![27 - lz, 45, 79 + lz], 4 => vec![61 + lz], 7 => vec![45], 5 => vec![45, 55], _ => vec![] }); }
        v
    }
    /// Longest step (minutes, >= 1) over which no need crosses a threshold and no timer expires.
    fn seg_len(&self, max: i64) -> i64 {
        let mut n = max.max(1);
        let (lv, rt) = (self.levels(), self.rates());
        for i in 0..9 {
            let r = rt[i]; if r == 0 { continue; }
            if (r < 0 && lv[i] == 0) || (r > 0 && lv[i] == MAX) { continue; }      // pinned at a clamp
            let mut ths = self.thresholds(i); ths.push(0); ths.push(100);
            for th in ths { let tv = if th >= 100 { MAX } else { th.max(0) * P };
                let dist = tv - lv[i];
                if (dist > 0) == (r > 0) && dist != 0 { n = n.min(((dist.abs() - 1) / r.abs()) as i64); } }
        }
        let mut timers = vec![self.rested_min, self.injured_min, self.groggy_min, self.cd_med, self.rehab_min, self.recovering_min, self.junk_min, self.ins_min, self.ozempic_min,
            self.cd_ozempic, self.cd_work, self.insured_min, self.bill_due, self.auth_due, self.week_left, if self.sleeping { (self.hangover_min + 1) / 2 } else { self.hangover_min }];
        if self.hard && self.econ && self.money < -600 { timers.push(2880 - self.evict_min); }
        if self.load > LOAD_HI && !self.dependent { let thr = if self.recovering_min > 0 { 400 } else { 1200 }; timers.push(thr - self.drunk_acc); }
        if self.weight > 80 * P && !self.diabetic { timers.push(4320 - self.over_acc); }
        if self.diabetic && self.weight < 65 * P && self.fit > 60 * P { timers.push(10080 - self.rev_acc); }
        if self.sleeping { timers.push(720 - self.sleep_min); if self.energy >= self.ecap() { timers.push(300 - self.oversleep); timers.push(241 - self.oversleep); } }
        if self.health == 0 { timers.push(720 - self.leaving_min); }
        for t in timers { if t > 0 { n = n.min(t as i64); } }
        if self.demo { for due in [self.demo_next, self.whim_next, self.lazy_next, self.nap_until, self.bed_at, self.up_at, if self.walks { self.walk_next } else { 0 }] { if due > self.t { n = n.min(due - self.t); } } }
        n.max(1)
    }
    /// Signature of everything that is derived rather than a need level: used to keep mood and health
    /// from crossing one of their own thresholds inside a segment.
    fn sig(&self) -> (i32, i32, bool, bool, i32) {
        let mb = match pc(self.mood) { 80.. => 0, 60..=79 => 1, 40..=59 => 2, 20..=39 => 3, _ => 4 };
        let hb = if self.health == 0 { 0 } else if self.health < 25 * P { 1 } else if self.health < 30 * P { 2 } else if self.health < 45 * P { 3 } else if self.health < 50 * P { 4 } else { 5 };
        (mb, hb, self.sleeping, self.gone || self.dependent || self.diabetic || self.hangover_min > 0 || self.deprived, (self.health - self.ceiling()).signum())
    }
    /// Closed-form advance by `dt` minutes at the rates of the current regime.
    fn advance(&mut self, dt: i64) {
        if self.gone { return; }
        let d = dt as i32; let r = self.rates();
        let was_full_energy = self.energy >= self.ecap();
        if self.sleeping {
            self.sleep_min += d;
            if !was_full_energy { if self.sleep_quality_pm() == 1000 { self.good_sleep_acc += d; } } else { self.oversleep += d; }
        }
        self.full = cl(self.full + r[0] * d); self.hyd = cl(self.hyd + r[1] * d); self.energy = cl(self.energy + r[2] * d);
        if self.sleeping && r[2] > 0 { self.energy = self.energy.min(self.ecap()); }
        self.stim = cl(self.stim + r[3] * d); if self.sleeping && r[3] != 0 { self.stim = self.stim.max(40 * P); }
        // slow accumulators, by the regime at the start of the segment
        if self.load > LOAD_HI { self.drunk_acc += d; } else if self.load < 15 * P { self.drunk_acc = (self.drunk_acc - d / 4).max(0); }
        if self.weight > 80 * P { self.over_acc += d; } else if self.weight < 65 * P { self.over_acc = (self.over_acc - d / 4).max(0); }
        if self.dependent { self.dep_days += dt; }
        self.dirt = cl(self.dirt + r[4] * d); self.load = cl(self.load + r[5] * d); self.strain = cl(self.strain + r[6] * d); self.fit = cl(self.fit + r[7] * d);
        self.weight = cl(self.weight + r[8] * d);
        if self.load > LOAD_HI { self.peak40 = true; }
        if self.peak40 && self.load < 15 * P { self.peak40 = false; self.hangover_min = 480; }
        if !self.dependent && !self.demo && self.drunk_acc >= if self.recovering_min > 0 { 400 } else { 1200 } { self.dependent = true; }
        if !self.diabetic && self.over_acc >= 4320 { self.diabetic = true; self.rev_acc = 0; }
        if self.diabetic && self.weight < 65 * P && self.fit > 60 * P { self.rev_acc += d; if self.rev_acc >= 10080 { self.diabetic = false; self.over_acc = 0; self.reversed += 1; } }
        if self.deprived { self.dep_min += dt; }
        let hd = if self.sleeping { 2 * d } else { d }; self.hangover_min = (self.hangover_min - hd).max(0);
        if self.rehab_min > 0 { self.rehab_min = (self.rehab_min - d).max(0); if self.rehab_min == 0 { self.dependent = false; self.drunk_acc = 0; self.recovering_min = 10080; } }
        // hard-mode paperwork
        if self.bill_due > 0 { self.bill_due -= d; if self.bill_due <= 0 { self.bill_due = 0; self.money -= self.bill_amt; self.bills += self.bill_amt; self.bill_amt = 0; self.bill_count += 1; } }
        if self.auth_due > 0 { self.auth_due -= d; if self.auth_due <= 0 { self.auth_due = 0;
            if self.auth_ok == 0 && self.rnd() % 100 < 25 { self.denied += 1; self.auth_ok = 2; self.auth_due = 120 + (self.rnd() % 120) as i32; }   // denied: appeal filed, always granted
            else { self.auth_ok = 1; } } }
        self.week_left -= d; if self.week_left <= 0 { self.week_left = 10080; self.week_care = 0; self.week_doses = 0; }
        self.min_money = self.min_money.min(self.money);
        // eviction (hard mode): a 48 h countdown while HAX is under the eviction line; drains back at half speed above it
        if self.hard && self.econ {
            if self.money < -150 { self.notices[0] += dt; } if self.money < -350 { self.notices[1] += dt; }
            if self.money < -600 { self.notices[2] += dt; self.evict_min += d; if self.evict_min >= 2880 { self.evicted = true; self.gone = true; } }
            else if self.evict_min > 0 { self.evict_min = (self.evict_min - (d + 1) / 2).max(0); }
        }
        if self.sleeping { if self.sleep_min >= 720 || self.oversleep >= 300 { self.wake(false); } }
        else {
            if self.energy < 85 * P { self.oversleep = 0; }
            if self.energy == 0 { if self.health > 33 * P { self.health = cl(self.health - 3 * P); } self.night = true; self.sleeping = true; self.sleep_min = 0; self.good_sleep_acc = 0; }
        }
        for t in [&mut self.rested_min, &mut self.injured_min, &mut self.groggy_min, &mut self.cd_med, &mut self.recovering_min, &mut self.junk_min, &mut self.ins_min,
                  &mut self.ozempic_min, &mut self.cd_ozempic, &mut self.cd_work, &mut self.insured_min, &mut self.declined_min] { *t = (*t - d).max(0); }
        // --- mood: linear chase of the target, at most MOOD_STEP per minute ---
        let old_mood = self.mood;
        let tgt = self.mood_target();
        self.mood = cl(tgt.clamp(self.mood - MOOD_STEP * d, self.mood + MOOD_STEP * d));
        // --- health: slow ---
        let mut dmg = 0;
        if self.full < 15 * P { dmg += 40; }
        if self.full > 95 * P { dmg += 8; }
        if self.hyd < 15 * P { dmg += 40; }
        if self.hyd > 95 * P { dmg += 8; }
        if self.energy < 10 * P && !self.sleeping && self.health > 30 * P { dmg += 8; }      // sleep shortage alone never takes health under 30 %
        if self.load > LOAD_DANGER { dmg += 40; }
        if self.mood < 20 * P { dmg += 10; }
        if self.dirt > 85 * P { dmg += 10; }
        if self.weight > 80 * P { dmg += 2; }
        if self.weight < 20 * P { dmg += 10; }
        let ceiling = self.ceiling();
        let rec = (if self.mood >= 60 * P { 14 } else if self.mood >= 40 * P { 5 } else { 0 }) / if self.deprived { 2 } else { 1 };
        let mut h = self.health - dmg * d;
        if h < ceiling { h = (h + rec * d).min(ceiling.max(h)); } else if h > ceiling { h = (h - 3 * d).max(ceiling); }
        self.health = cl(h);
        if self.demo { self.health = self.health.max(25 * P); self.fit = self.fit.max(45 * P); }   // demo pet stays okay-ish fit without exercising on screen
        // --- leaving ---
        if self.health == 0 { self.leaving_min += d; self.leaving_total += dt; if self.leaving_min >= 720 { self.gone = true; } }
        else if self.leaving_min > 0 { let dec = ((self.mins + dt) / 2 - self.mins / 2) as i32; self.leaving_min = (self.leaving_min - dec).max(0); }
        // --- bookkeeping ---
        self.mins += dt; self.t += dt; self.segs += 1;
        self.mood_sum += (old_mood as i64 + self.mood as i64) / 2 * dt; if self.mood >= 60 * P { self.good_min += dt; }
        self.min_health = self.min_health.min(self.health);
        let b = match pc(self.mood) { 80.. => 0, 60..=79 => 1, 40..=59 => 2, 20..=39 => 3, _ => 4 }; self.band[b] += dt;
    }
    /// What the screen shows: zone of every need plus the flags. A change means a wake-up and a refresh.
    fn obs(&self) -> u64 {
        let z = |v: i32, t: &[i32]| -> u64 { t.iter().filter(|x| v >= **x * P).count() as u64 };
        let mut o = z(self.full, &[15, 38, 78, 95]);
        o = o * 8 + z(self.hyd, &[15, 38, 83, 95]); o = o * 8 + z(self.energy, &[10, 33]); o = o * 8 + z(self.stim, &[33, 73]);
        o = o * 8 + z(self.dirt, &[58, 85]); o = o * 8 + z(self.load, &[38, 70]); o = o * 8 + z(self.health, &[1, 30, 50]);
        o = o * 8 + z(self.mood, &[20, 40, 60]); o = o * 8 + z(self.weight, &[20, 38, 63, 80]); o = o * 2 + self.deprived as u64;
        o = o * 2 + (self.hangover_min > 0) as u64; o = o * 2 + (self.rehab_min > 0) as u64;
        o * 16 + self.sleeping as u64 + 2 * (self.injured_min > 0) as u64 + 4 * (self.groggy_min > 0) as u64 + 8 * (self.gone || self.leaving_min > 0) as u64
    }
    fn disp(&self) -> u64 {
        let pens = [ (FULL_LO - self.full).max(0) * 3 / 2, (self.full - FULL_HI).max(0), (HYD_LO - self.hyd).max(0) * 3 / 2, (self.hyd - HYD_HI).max(0),
            if self.sleeping { 0 } else { (EN_LO - self.energy).max(0) * 3 / 2 }, if self.sleeping { 0 } else { (STIM_LO - self.stim).max(0) }, (self.stim - STIM_HI).max(0) * 3 / 2,
            (self.load - LOAD_HI).max(0), (self.dirt - DIRT_HI).max(0), (self.weight - W_HI).max(0) / 2, (W_LO - self.weight).max(0) / 2, (FIT_LO - self.fit).max(0) / 2,
            if self.hangover_min > 0 { 15 * P } else { 0 }, if self.injured_min > 0 { 20 * P } else { 0 }, if self.groggy_min > 0 { 10 * P } else { 0 }, (50 * P - self.health).max(0) ];
        let (mut bi, mut bv) = (0usize, 3 * P);                      // under 3 points nothing is shown: "content"
        for (i, v) in pens.iter().enumerate() { if *v > bv { bv = *v; bi = i + 1; } }
        let danger = self.full < 15 * P || self.hyd < 15 * P || self.load > LOAD_DANGER || self.dirt > 85 * P || self.health < 30 * P;
        (bi as u64) * 8 + self.sleeping as u64 + 2 * danger as u64 + 4 * (self.gone || self.leaving_min > 0) as u64
    }
    fn note_wake(&mut self, engine: bool) {
        let gap = (self.t - self.last_wake).max(0);
        self.cap30 += ((gap - 1).max(0) / 30) as u32; self.cap120 += ((gap - 1).max(0) / 120) as u32;
        self.last_wake = self.t; if engine { self.obs_changes += 1; }
    }
    /// Catch up to absolute minute `now`. Oracle: one-minute steps. Delta: one closed-form step per segment.
    fn update_to(&mut self, now: i64) {
        while self.t < now && !self.gone {
            let mut dt = if self.delta { self.seg_len(now - self.t) } else { 1 };
            if dt > 1 {
                let s0 = self.sig();
                loop { let mut trial = self.clone(); trial.advance(dt); if trial.sig() == s0 || dt == 1 { break; } dt = (dt + 1) / 2; }
            }
            self.advance(dt);
            if self.demo { for _ in 0..4 { let n = self.auto_acts; self.demo_policy(); if self.auto_acts == n { break; } } }   // an action is itself an event
            let o = self.disp(); if o != self.last_obs { self.last_obs = o; self.note_wake(true); }
        }
    }

    fn wake(&mut self, by_player: bool) {
        if !self.sleeping { return; }
        self.sleeping = false;
        if by_player && self.energy < 50 * P { self.mood = cl(self.mood - 10 * P); self.grumpy += 1; }
        // sleep-deprived: three nights in a row under 6 h; two good nights (6.5 h or more) clear it. Sessions under 2 h are naps.
        if self.night {
            if self.sleep_min < 360 { self.short_nights += 1; self.good_nights = 0; if self.short_nights >= 3 { self.deprived = true; } }
            else { self.short_nights = 0; if self.sleep_min >= 390 { self.good_nights += 1; if self.good_nights >= 2 { self.deprived = false; } } }
        }
        if self.no_groggy { self.no_groggy = false; self.oversleep = 0; }
        if self.oversleep > 240 { self.groggy_min = 120; self.energy = cl(self.energy - 10 * P); self.fit = cl(self.fit - 2 * P); self.groggy += 1; }
        if self.good_sleep_acc >= 300 { self.rested_min = 960; } // 5 h of good-quality recovery -> well rested for 16 h
    }

    /// Treat mood bump shrinks with current load (tolerance): base * (100 - load%) / 100.
    fn treat(&mut self, mood: i32, energy: i32, load: i32) {
        let dim = (100 - pc(self.load)).max(0) / if self.dependent { 2 } else { 1 };
        self.mood = cl(self.mood + mood * P * dim / 100);
        self.energy = cl(self.energy + energy * P * dim / 100);
        let amp = if self.energy < EN_LO { 150 } else { 100 }; // booze/junk while tired hits harder
        self.load = cl(self.load + load * P * amp / 100);
    }

    /// Charge a price. Care (medicine) goes through the hard-mode deductible and copay; nothing is ever refused for lack of money in hard mode (debt).
    /// Hard mode: one paid medication request in four shows "Prior authorization denied"; the only button is Appeal, which grants it at once.
    fn prior_auth(&mut self) { if self.hard && self.econ && self.rnd() % 100 < 25 { self.denied += 1; self.presses += 1; } }
    fn pay(&mut self, base: i32, care: bool) {
        if !self.econ { return; }
        let mut price = base;
        if care && self.hard && self.insured_min > 0 {
            // deductible 150 HAX per game week, then a 20 % copay
            let room = (150 - self.week_care).max(0);
            let full = price.min(room); price = full + (price - full) * 10 / 100;
        }
        if care { self.week_care += base; self.spent_care += price; } else { self.spent_food += price; }
        self.money -= price;
    }
    fn act(&mut self, a: Act) -> bool {
        if self.gone { return false; }
        if let Act::Station(k) = a {   // free, works asleep or awake, never pushes a need past its band
            match k {
                0 => { self.full = self.full.max(60 * P); self.hyd = self.hyd.max(60 * P); }                                   // "more food"
                1 => { self.health = self.health.max(self.ceiling()); self.hangover_min = 0; self.injured_min = 0; self.groggy_min = 0; self.load = self.load.min(20 * P); }   // "more drugs"
                3 => { self.dependent = false; self.drunk_acc = 0; self.rehab_min = 0; self.diabetic = false; self.over_acc = 0; self.load = self.load.min(5 * P); self.fit = self.fit.max(50 * P); }   // clinic
                _ => { self.deprived = false; self.short_nights = 0; self.groggy_min = 0; self.oversleep = 0; self.energy = self.energy.max(self.ecap()); self.no_groggy = true; }                  // "sleep like a bear"
            }
            self.mood = cl(self.mood + 5 * P); self.stations += 1; return true;
        }
        if a == Act::Drink(Drink::Water) && self.hyd > 85 * P { self.refused += 1; return false; }   // Not thirsty.
        if a == Act::Wash && self.dirt < 25 * P { self.refused += 1; return false; }                 // Already clean.
        if self.sleeping {
            match a { Act::Wake => { self.wake(true); self.presses += 1; return true; }
                      Act::Sleep | Act::Mini(_) => return false,
                      _ => { self.wake(true); } } // any care action wakes the pet first
        }
        self.presses += 1;
        match a {
            Act::Eat(f) => {
                // (fullness, mood bump, weight in tenths of a percent)
                let (gain, mood, wt) = match f {
                    Food::Salad => (20, 0, 0), Food::Apple => (25, 0, 5),
                    Food::Frik => (35, 6, 20), Food::Pizza => (40, 8, 20), Food::Cake => (10, 15, 30) };
                self.pay(match f { Food::Salad | Food::Apple => if self.hard { 10 } else { 8 }, Food::Cake => 8, _ => 6 }, false);
                let after = self.full + gain * P;
                let excess = (after.min(MAX + 20 * P) - FULL_HI).max(0); // overeating
                self.full = cl(after);
                if f == Food::Salad || f == Food::Apple { self.hyd = cl(self.hyd + 8 * P); }
                self.dirt = cl(self.dirt + if f == Food::Cake { 5 } else { 10 } * P);
                self.weight = cl(self.weight + wt * P / 10 + excess / 4);   // overeating goes to weight
                self.energy = cl(self.energy - excess / 2);                 // food coma
                if f == Food::Salad { self.fit = cl(self.fit + P); }
                if mood > 0 { // junk tolerance: full bump only if the last treat was over 3 h ago; none on Ozempic
                    if self.ozempic_min == 0 { let m = if self.junk_min == 0 { mood } else { mood / 3 }; self.mood = cl(self.mood + m * P); }
                    self.junk_min = 180; }
            }
            Act::Drink(d) => { let (h, belly) = match d { Drink::Water => (30, 2), Drink::Cola => (20, 2), Drink::Beer => (20, 2), Drink::Wine => (8, 1), Drink::Whiskey => (0, 0) };
              // a pet only drinks what it can hold: plain water never takes it past 90 %
              let h = if d == Drink::Water { h.min((90 * P - self.hyd).max(0) / P) } else { h };
              let belly = if h >= 15 || d != Drink::Water { belly } else { 0 };
              self.hyd = cl(self.hyd + h * P); self.full = cl(self.full + belly * P);
              self.pay(match d { Drink::Water => 1, Drink::Cola => 5, Drink::Beer => 8, Drink::Wine => 10, Drink::Whiskey => 12 }, false);
              if !matches!(d, Drink::Water | Drink::Cola) && self.rehab_min > 0 { self.rehab_min = 0; self.relapses += 1; self.mood = cl(self.mood - 20 * P); }   // relapse
              match d {
                Drink::Water => { self.load = cl(self.load - 5 * P); self.hangover_min = (self.hangover_min - 60).max(0); self.groggy_min = (self.groggy_min - 30).max(0); }
                Drink::Cola => { self.energy = cl(self.energy + 8 * P); self.mood = cl(self.mood + 3 * P); self.weight = cl(self.weight + P); }
                Drink::Beer => { self.treat(12, 5, 15); self.stim = cl(self.stim + 5 * P); self.weight = cl(self.weight + P); }
                Drink::Wine => { self.treat(15, 5, 20); self.stim = cl(self.stim + 5 * P); }
                Drink::Whiskey => { self.treat(20, 5, 30); self.stim = cl(self.stim + 5 * P); }
            } },
            Act::Play | Act::Mini(_) | Act::Friend => {
                if self.injured_min > 0 && a == Act::Play { self.presses -= 1; return false; }
                let (mut s, e) = match a { Act::Play => (25, 5), Act::Mini(g) => (10 + 7 * g, 3), _ => (15, 2) };
                if let Act::Mini(g) = a { if self.econ { self.money += 5 * g; self.earned += 5 * g; } }
                if self.energy < EN_LO { s *= 2; } // stimulation while tired counts double
                self.stim = cl(self.stim + s * P);
                self.energy = cl(self.energy - e * P);
                if a == Act::Play { self.groggy_min = (self.groggy_min - 60).max(0); self.hyd = cl(self.hyd - 4 * P); self.dirt = cl(self.dirt + 3 * P); self.full = cl(self.full - 3 * P); self.fit = cl(self.fit + 2 * P); }
            }
            Act::Sport => {
                if self.injured_min > 0 { self.presses -= 1; return false; }
                let mut st = 35 * P;
                if self.full < FULL_LO || self.energy < EN_LO || self.hyd < HYD_LO { st = st * 150 / 100; }
                self.hyd = cl(self.hyd - 12 * P);
                if self.rested_min > 0 { st = st * 70 / 100; }
                if self.fit >= 70 * P { st = st * 85 / 100; }
                if self.weight > W_HI { st = st * 120 / 100; }          // hard on the joints
                if self.deprived { st = st * 125 / 100; }
                self.weight = cl(self.weight - 3 * P); self.hangover_min = (self.hangover_min - 120).max(0); self.groggy_min = 0;
                self.strain = cl(self.strain + st);
                self.energy = cl(self.energy - 10 * P);
                self.full = cl(self.full - 8 * P);
                self.stim = cl(self.stim + 10 * P);
                self.load = cl(self.load - 5 * P); // sweat it out
                self.dirt = cl(self.dirt + 12 * P);
                if self.strain > STRAIN_INJ {
                    self.injured_min = 1440; self.health = cl(self.health - 5 * P); self.injuries += 1;
                } else { self.fit = cl(self.fit + 12 * P); }
            }
            Act::Wash => { self.dirt = cl(self.dirt - 70 * P); }                // "Already clean."
            Act::Sleep => { self.night = self.energy < 50 * P; self.sleeping = true; self.sleep_min = 0; self.good_sleep_acc = 0; }
            Act::Wake => { self.presses -= 1; return false; }
            Act::Station(_) => {}
            Act::Med => { if self.cd_med > 0 { self.presses -= 1; return false; }
                self.prior_auth(); self.pay(5, true); self.cd_med = 240;
                if self.injured_min > 0 { self.injured_min = (self.injured_min - 720).max(0); self.load = cl(self.load + 10 * P); }   // painkillers
                else if self.health > 80 * P { self.load = cl(self.load + 10 * P); }                                               // nothing to fix
                else { self.health = cl(self.health + 10 * P); }
                if self.hard && self.econ { self.bill_due = 120 + (self.rnd() % 240) as i32; self.bill_amt += if self.insured_min > 0 { 20 + (self.rnd() % 40) as i32 } else { 40 + (self.rnd() % 80) as i32 }; } }
            Act::Relax => { self.stim = cl(self.stim - 25 * P); self.strain = cl(self.strain - 15 * P); self.energy = cl(self.energy + 3 * P); self.fit = cl(self.fit - P); }
            Act::Insulin => { if !self.diabetic || self.ins_min > 120 { self.presses -= 1; return false; }
                let price = if self.hard { (30 * (100 + 5 * self.week_doses) / 100).min(60) } else { 5 };   // creeps 5 % per dose, resets with the plan week
                self.week_doses += 1;
                self.prior_auth(); self.ins_doses += 1; self.pay(price, true); self.ins_min = 480; }
            Act::Ozempic => { if self.cd_ozempic > 0 { self.presses -= 1; return false; }
                self.prior_auth();
                self.pay(if self.hard { 180 } else { 60 }, true);
                self.weight = cl(self.weight - 15 * P); self.fit = cl(self.fit - 5 * P); self.ozempic_min = 1440; self.cd_ozempic = 2880; }
            Act::Rehab => { if !self.dependent || self.rehab_min > 0 { self.presses -= 1; return false; }
                self.prior_auth();
                self.pay(if self.hard { 750 } else { 150 }, true); self.rehab_min = 4320; }
            Act::Work => { if self.cd_work > 0 || self.energy < EN_LO || self.injured_min > 0 { self.presses -= 1; return false; }
                let pay = if self.money < 0 { 35 } else { 45 };                 // 10 HAX garnished while in debt
                self.money += pay; self.earned += pay; self.energy = cl(self.energy - 15 * P); self.stim = cl(self.stim + 10 * P); self.mood = cl(self.mood - 5 * P);
                self.cd_work = 60; self.insured_min = 2880; }
        }
        true
    }
}

impl Pet {
    fn rnd(&mut self) -> u32 { let mut x = self.rng; x ^= x << 13; x ^= x >> 17; x ^= x << 5; self.rng = x; x }
    /// true with probability pct/100
    fn chance(&mut self, pct: i32) -> bool { (self.rnd() % 100) as i32 <= pct - 1 }
    fn auto(&mut self, a: Act) { if self.act(a) { self.auto_acts += 1; self.presses -= 1; self.note_wake(false); } }
    /// Demo-mode self-care. Called every 10 minutes. One action at most per call.
    fn demo_policy(&mut self) {
        let now = self.t;
        if now >= self.lazy_next { self.lazy = (self.rnd() % 16) as i32; self.lazy_next = now + 360; } // temperament for the next 6 h
        let lz = self.lazy;
        let (f, e, st, hy) = (pc(self.full), pc(self.energy), pc(self.stim), pc(self.hyd));
        if self.sleeping {
            if self.nap_until > 0 { if now >= self.nap_until { self.nap_until = 0; self.wake(false); self.auto_acts += 1; self.note_wake(false); } return; }
            if f < 18 { self.wake(false); return; }
            // follows the clock: up between 07:00 and 08:30, drawn when it went to bed
            if self.up_at > 0 && now >= self.up_at { self.up_at = 0; self.wake(false);
                let day = now / 1440; self.bed_at = day * 1440 + 22 * 60 + (self.rnd() % 91) as i64; if self.bed_at <= now { self.bed_at += 1440; } }
            return;
        }
        // bedtime by the clock: between 22:00 and 23:30
        if now >= self.bed_at { self.auto(Act::Sleep); self.nap_until = 0;
            let day = now / 1440 + 1; self.up_at = day * 1440 + 7 * 60 + (self.rnd() % 91) as i64; self.bed_at = i64::MAX; return; }
        // 1. edge rules (the floor): act at once when the threshold is crossed
        if f < 20 { self.auto(Act::Eat(Food::Pizza)); return; }
        if hy < 22 { self.auto(Act::Drink(Drink::Water)); return; }
        if e < 12 { self.auto(Act::Sleep); self.nap_until = now + 120; return; }      // worn out in the daytime: a two-hour nap
        if self.health < 45 * P && self.cd_med == 0 { self.auto(Act::Med); return; }
        if self.load > 55 * P { if now >= self.demo_next { self.auto(Act::Drink(Drink::Water)); self.demo_next = now + 20 + (self.rnd() % 40) as i64; } return; }
        // 2. soft rules: when a need crosses its soft threshold a delay is drawn once; the pet acts when it runs out
        if st - lz > 78 { self.auto(Act::Relax); return; }
        if self.walks && self.fit < 45 * P && e > 40 && self.injured_min == 0 && now >= self.walk_next { self.walk_next = now + 180 + (self.rnd() % 120) as i64; self.auto(Act::Play); return; }   // a walk: active play, not sport                    // wired: lies down
        let soft = if hy + lz < 48 { 1 } else if f + lz < 36 { 2 } else if pc(self.dirt) - lz > 60 { 4 }
                   else if st + lz < 27 && e > 25 && self.injured_min == 0 { 5 } else { 0 };
        if soft != 0 {
            if !self.armed { self.armed = true; self.demo_next = now + 10 + (self.rnd() % 21) as i64; }
            if now >= self.demo_next {
                self.armed = false;
                match soft {
                    1 => { let r = self.rnd() % 100; let d = if r < 70 { Drink::Water } else if r < 90 || self.load > 15 * P { Drink::Cola } else { Drink::Beer }; self.auto(Act::Drink(d)); }
                    2 => { let r = self.rnd() % 100;
                           let mut food = if r < 40 { Food::Apple } else if r < 55 { Food::Salad } else if r < 80 { Food::Pizza } else if r < 92 { Food::Frik } else { Food::Cake };
                           if self.weight > 56 * P && r >= 55 { food = Food::Salad; } if self.weight < 42 * P && r < 55 { food = Food::Pizza; }
                           self.auto(Act::Eat(food)); if self.chance(15) { self.auto(Act::Eat(Food::Cake)); } }
                    3 => self.auto(Act::Sleep),
                    4 => self.auto(Act::Wash),
                    _ => { let a = if self.chance(30) { Act::Mini((self.rnd() % 3) as i32) } else { Act::Play }; self.auto(a); }
                }
            }
            return;
        }
        self.armed = false;
        // (the demo pet never exercises on its own: sport is the player's job)
        // 3. whims on their own timer: a treat, one too many, or a nap
        if now >= self.whim_next {
            self.whim_next = now + 180 + (self.rnd() % 600) as i64;
            let dry = self.drunk_acc > 300;                                  // the demo pet never drinks itself into dependence
            if self.load < 15 * P { let a = if self.chance(50) && !dry { Act::Drink(Drink::Beer) } else { Act::Eat(Food::Cake) }; self.auto(a); }
            else if self.load < 45 * P && !dry && self.chance(35) { self.auto(Act::Drink(Drink::Beer)); }
            else if e < 55 && st < 45 { self.auto(Act::Sleep); self.nap_until = now + 40 + (self.rnd() % 60) as i64; }
        }
    }
}
fn nobody(_d: u32, _m: u32, _p: &Pet) -> Vec<Act> { vec![] }
/// Player looks once a day at 18:00 and fixes the single worst thing.
fn helper(_d: u32, m: u32, p: &Pet) -> Vec<Act> {   // one look a day at 18:00: feed if needed, then exercise the pet (sport is the player's job in demo mode)
    if !at(m,18,0) { return vec![]; }
    let mut v = vec![];
    if p.full < 45 * P { v.push(Act::Eat(Food::Apple)); }
    if p.hyd < 45 * P { v.push(Act::Drink(Drink::Water)); }
    if p.strain < 20 * P && p.energy > 33 * P && p.injured_min == 0 { v.push(Act::Sport); }
    if p.dirt > 50 * P { v.push(Act::Wash); }
    v
}
impl Pet {
    fn cond(&self) -> battle::Cond { battle::Cond { fit: pc(self.fit), energy: pc(self.energy), load: pc(self.load), mood: pc(self.mood), health: pc(self.health),
        deprived: self.deprived, weight: pc(self.weight), hangover: self.hangover_min > 0, craving: self.dependent && self.load < 10 * P && self.rehab_min == 0, strain: pc(self.strain), hyd: pc(self.hyd), full: pc(self.full), stim: pc(self.stim), dirt: pc(self.dirt), rested: self.rested_min > 0, injured: self.injured_min > 0,
        groggy: self.groggy_min > 0, vit: 50, cur: 50, res: 50 } }
    /// Costs of one battle for the challenger. Returns false if the pet cannot fight.
    fn battle_cost(&mut self, won: bool, hp_left_pct: i32) -> bool {
        if self.injured_min > 0 || self.sleeping { return false; }
        let mut st = 20 * P;
        if self.full < FULL_LO || self.energy < EN_LO || self.hyd < HYD_LO { st = st * 150 / 100; }
        self.hyd = cl(self.hyd - 8 * P);
        if self.rested_min > 0 { st = st * 70 / 100; }
        self.strain = cl(self.strain + st);
        self.energy = cl(self.energy - 10 * P);
        self.stim = cl(self.stim + 20 * P);
        self.dirt = cl(self.dirt + 8 * P);
        self.mood = cl(self.mood + if won { 8 * P } else { -8 * P });
        self.health = cl(self.health - (100 - hp_left_pct) * P / 20);     // bruises: up to -5 %
        if self.strain > STRAIN_INJ { self.injured_min = 1440; self.health = cl(self.health - 5 * P); self.injuries += 1; }
        true
    }
}
fn brawler(d: u32, m: u32, p: &Pet) -> Vec<Act> {
    let rich = |p: &Pet| if p.weight < 76 * P { Food::Pizza } else { Food::Apple };
    if at(m,10,0) { if p.dirt > 50 * P { vec![Act::Wake, Act::Wash, Act::Eat(rich(p))] } else { vec![Act::Wake, Act::Eat(rich(p))] } }
    else if at(m,14,0) { if d % 2 == 0 && p.weight < 76 * P { vec![Act::Eat(Food::Cake)] } else { vec![Act::Drink(Drink::Cola)] } }
    else if at(m,17,0) { vec![Act::Play] }
    else if at(m,18,30) { vec![Act::Eat(if p.full < 40 * P { rich(p) } else { Food::Apple })] }
    else if at(m,19,30) { vec![Act::Drink(Drink::Beer), Act::Drink(Drink::Beer)] }  // two for the fight at 20:00
    else if at(m,22,0) { vec![Act::Drink(Drink::Water), Act::Mini(1)] }
    else if at(m,1,0) { if p.full < 45 * P { vec![Act::Eat(Food::Apple), Act::Sleep] } else { vec![Act::Sleep] } } else { vec![] }
}
fn fighter(_d: u32, m: u32, p: &Pet) -> Vec<Act> {
    if at(m,7,30) { if p.full < 40 * P { vec![Act::Wake, Act::Eat(Food::Apple), Act::Eat(Food::Salad)] } else { vec![Act::Wake, Act::Eat(Food::Apple)] } }
    else if at(m,9,30) { vec![Act::Sport] }
    else if at(m,12,30) { let f = if p.full < 45 * P { Food::Pizza } else { Food::Apple }; if p.dirt > 45 * P { vec![Act::Wash, Act::Eat(f)] } else { vec![Act::Eat(f)] } }
    else if at(m,15,30) { vec![Act::Play] }
    else if at(m,18,30) { if p.full < 45 * P { vec![Act::Eat(Food::Apple), Act::Eat(Food::Salad)] } else if p.full < 65 * P { vec![Act::Eat(Food::Apple)] } else { vec![Act::Play] } }
    else if at(m,20,30) { vec![Act::Wash] }
    else if at(m,22,30) { vec![Act::Sleep] } else { vec![] }
}
fn tipsy_fighter(d: u32, m: u32, p: &Pet) -> Vec<Act> { if at(m,19,30) { vec![Act::Drink(Drink::Beer)] } else { fighter(d, m, p) } }
fn sumo(_d: u32, m: u32, p: &Pet) -> Vec<Act> {
    let rich = |p: &Pet| if p.weight < 74 * P { Food::Pizza } else { Food::Apple };
    if at(m,7,30) { vec![Act::Wake, Act::Eat(rich(p))] } else if at(m,9,30) { vec![Act::Sport] }
    else if at(m,12,30) { if p.dirt > 45 * P { vec![Act::Wash, Act::Eat(rich(p))] } else { vec![Act::Eat(rich(p))] } }
    else if at(m,15,30) { if p.weight < 74 * P { vec![Act::Eat(Food::Cake), Act::Play] } else { vec![Act::Play] } }
    else if at(m,18,30) { if p.full < 60 * P { vec![Act::Eat(rich(p))] } else { vec![] } }
    else if at(m,20,30) { vec![Act::Wash] }
    else if at(m,22,30) { vec![Act::Sleep] } else { vec![] }
}
// ---- hard-mode money runs ----
fn earner(d: u32, m: u32, p: &Pet) -> Vec<Act> {
    let mut v = balanced(d, m, p);
    if p.rehab_min > 0 { for a in v.iter_mut() { if matches!(a, Act::Drink(Drink::Beer)) { *a = Act::Drink(Drink::Water); } } }
    if at(m,16,30) { v.push(Act::Work); }
    if at(m,11,0) && p.money < 60 { v.push(Act::Work); }
    v
}
fn earner2(d: u32, m: u32, p: &Pet) -> Vec<Act> {   // two shifts every day, 09:00 and 14:00
    let mut v: Vec<Act> = balanced(d, m, p);
    if p.rehab_min > 0 { for a in v.iter_mut() { if matches!(a, Act::Drink(Drink::Beer)) { *a = Act::Drink(Drink::Water); } } }
    if at(m,9,0) || at(m,14,0) { v.push(Act::Work); }
    v
}
fn idler(d: u32, m: u32, p: &Pet) -> Vec<Act> { balanced(d, m, p) }                                                                 // never works
fn party_idler(d: u32, m: u32, p: &Pet) -> Vec<Act> { party_earner(d, m, p).into_iter().filter(|a| *a != Act::Work).collect() }
fn party_earner(d: u32, m: u32, p: &Pet) -> Vec<Act> {
    let mut v: Vec<Act> = balanced(d, m, p);
    if p.rehab_min > 0 { for a in v.iter_mut() { if matches!(a, Act::Drink(Drink::Beer)) { *a = Act::Drink(Drink::Water); } } }
    if at(m,12,0) { v.push(Act::Work); }                                        // works before the afternoon, while it still can
    if at(m,17,0) && p.money < 60 { v.push(Act::Work); }
    if p.rehab_min == 0 && at(m,22,0) { v.push(Act::Drink(Drink::Beer)); }     // a second beer every evening
    if p.dependent && p.rehab_min == 0 && d >= 2 && (at(m,13,0) || at(m,18,30)) { v.push(Act::Rehab); }
    v
}
fn money_runs() {
    println!("\n| run | mode | battles/day | HAX day 7 | day 14 | day 21 | day 30 | lowest | work | battle prizes | food+drink | care paid | surprise bills | denial popups | avg mood | end health | notes |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    // (name, routine, kind: 0 healthy 1 diabetic 2 dependent, battles per day, opponent: 0 average pet, 1 an equal)
    let runs: Vec<(&str, Routine, u8, u32, u8)> = vec![
        ("balanced, one shift a day", earner, 0, 0, 0), ("balanced, never works", idler, 0, 0, 0),
        ("balanced, one shift + 1 battle vs average pets", earner, 0, 1, 0),
        ("fit fighter, no work, 1 battle vs average pets", fighter, 0, 1, 0), ("fit fighter, no work, 3 battles vs average pets", fighter, 0, 3, 0),
        ("fit fighter, no work, 3 battles vs equals", fighter, 0, 3, 1),
        ("diabetic, one shift (second only when under 60 HAX)", earner, 1, 0, 0), ("diabetic, one shift + 1 battle vs equals", earner, 1, 1, 1), ("diabetic, two shifts every day", earner2, 1, 0, 0), ("diabetic, never works", idler, 1, 0, 0),
        ("diabetic, never works, 3 battles vs average pets", fighter, 1, 3, 0),
        ("dependent party animal, works, takes rehab", party_earner, 2, 0, 0), ("dependent party animal, never works, takes rehab", party_idler, 2, 0, 0)];
    let avg = battle::derive(&battle::Cond::BASE);
    for (name, r, kind, per_day, opp) in &runs { for hard in [false, true] {
        let mut p = Pet::new(); p.delta = true; p.econ = true; p.hard = hard; p.last_obs = p.disp();
        if *kind == 1 { p.diabetic = true; } if *kind == 2 { p.dependent = true; p.habits = false; }
        let mut marks = vec![]; let mut seed = 4242u32; let (mut prizes, mut fought, mut won) = (0, 0u32, 0u32); let mut won_day = (u32::MAX, 0u32);
        'o: for d in 0..30u32 {
            for m in (if d == 0 { 480 } else { 0 })..1440u32 {
                drive(&mut p, *r, d, m, true);
                if m >= 1200 && (m - 1200) % 30 == 0 && (m - 1200) / 30 < *per_day {
                    p.update_to(d as i64 * 1440 + m as i64);
                    let k = battle::derive(&p.cond()); let o = if *opp == 1 { k } else { avg };
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let res = battle::fight(seed, &k, &o);
                    if p.battle_cost(res.a_won, res.a_hp_pct) { fought += 1; if res.a_won { won += 1; if won_day.0 != d { won_day = (d, 0); } let prize = if won_day.1 == 0 { 100 } else { 20 }; won_day.1 += 1; p.money += prize; prizes += prize; } }
                    // a player treats an injury or low health with medicine: this is what triggers surprise bills
                    if (p.injured_min > 0 || p.health < 60 * P) && p.cd_med == 0 { p.act(Act::Med); }
                }
                if p.gone { break 'o; }
            }
            p.update_to((d as i64 + 1) * 1440);
            if [6, 13, 20, 29].contains(&d) { marks.push(p.money); }
        }
        while marks.len() < 4 { marks.push(p.money); }
        println!("| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} ({}) | {} | {} | {} | {}{}{}{}{} |", if hard { "hard" } else { "normal" }, per_day, marks[0], marks[1], marks[2], marks[3], p.min_money, p.earned, prizes, p.spent_food, p.spent_care, p.bills, p.bill_count, p.denied,
            (p.mood_sum / p.mins.max(1)) as i32 / P, pc(p.health),
            if p.evicted { format!("EVICTED day {:.1}; ", p.t as f32 / 1440.0) } else if p.gone { "GONE (health); ".to_string() } else { String::new() },
            if hard { format!("days under -150/-350/-600: {:.1}/{:.1}/{:.1}; ", p.notices[0] as f32 / 1440.0, p.notices[1] as f32 / 1440.0, p.notices[2] as f32 / 1440.0) } else { String::new() },
            if *per_day > 0 { format!("{won}/{fought} fights won, {} injuries; ", p.injuries) } else { String::new() },
            if *kind == 2 { if p.dependent { "still dependent; ".to_string() } else { "rehab completed; ".to_string() } } else { String::new() },
            if *kind == 1 { format!("{} insulin doses{}", p.ins_doses, if p.diabetic { "" } else { ", diabetes reversed" }) } else { String::new() });
    } }
}
fn lifestyle(delta: bool) {
    println!("\nlifestyles, {}", if delta { "delta engine" } else { "step oracle" });
    let avg = battle::derive(&battle::Cond::BASE);
    let who: Vec<(&str, Routine, u32)> = vec![("fit fighter, 1 battle/day", fighter, 1), ("fit fighter + 1 beer before", tipsy_fighter, 1), ("brawler, 1 battle/day", brawler, 1),
        ("fit fighter, 6 battles/day", fighter, 6), ("brawler, 6 battles/day", brawler, 6), ("brawler, no battles", brawler, 0), ("sumo (heavy and fit), 1 battle/day", sumo, 1)];
    let mut snaps: Vec<Vec<battle::Combat>> = vec![];
    println!("\n| lifestyle | outcome | avg mood | end health | min health | end fit | load at fight | injuries | fights fought | fights refused | win % vs average pet | power/guard/speed/stamina/focus at day 20 |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (name, r, per_day) in &who {
        let mut p = Pet::new(); p.delta = delta; p.habits = !(name.starts_with("brawler") || name.starts_with("sumo")); let mut seed = 99u32; let (mut fought, mut refused, mut wins, mut trials, mut loadsum) = (0u32, 0u32, 0u32, 0u32, 0i64);
        let mut snap = vec![]; let mut gone = None;
        'o: for d in 0..30u32 { for m in (if d == 0 { 480 } else { 0 })..1440u32 {
            drive(&mut p, *r, d, m, true);
            let fight_now = m >= 1200 && (m - 1200) % 30 == 0 && (m - 1200) / 30 < *per_day;
            if fight_now { p.update_to(d as i64 * 1440 + m as i64); }
            // battles from 20:00, every 30 minutes
            if m >= 1200 && (m - 1200) % 30 == 0 && (m - 1200) / 30 < *per_day {
                let k = battle::derive(&p.cond());
                if m == 1200 { snap.push(k); loadsum += pc(p.load) as i64;
                    for i in 0..200u32 { if battle::fight(d * 1000 + i + 5, &k, &avg).a_won { wins += 1; } trials += 1; } }
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let o = battle::fight(seed, &k, &avg);
                if p.battle_cost(o.a_won, o.a_hp_pct) { fought += 1; } else { refused += 1; }
            }
            if p.gone { gone = Some(d); break 'o; }
        } p.update_to((d as i64 + 1) * 1440); if p.gone { gone = Some(d); break 'o; } }
        let k = snap.get(20).copied().unwrap_or(avg);
        println!("| {name} | {} | {} | {} | {} | {} w{}{}{} | {} | {} | {fought} | {refused} | {} | {}/{}/{}/{}/{} |", match gone { Some(d) => format!("gone d{d}"), None => "alive".into() },
            (p.mood_sum / p.mins.max(1)) as i32 / P, pc(p.health), pc(p.min_health), pc(p.fit), pc(p.weight), if p.diabetic { " diabetic" } else { "" }, if p.dependent { " dependent" } else { "" }, loadsum / snap.len().max(1) as i64, p.injuries, wins * 100 / trials.max(1), k.power, k.guard, k.speed, k.stamina, k.focus);
        snaps.push(snap);
    }
    // head to head, day by day, using each pet's 20:00 snapshot
    let n = snaps[0].len().min(snaps[2].len());
    for (a, b, la, lb) in [(0usize, 2usize, "fit fighter", "brawler"), (1, 2, "tipsy fit fighter", "brawler"), (0, 1, "fit fighter", "tipsy fit fighter"), (6, 0, "sumo", "fit fighter"), (6, 2, "sumo", "brawler")] {
        let n = n.min(snaps[a].len()).min(snaps[b].len()); let (mut w, mut t) = (0, 0);
        for d in 0..n { for i in 0..200u32 { if battle::fight(d as u32 * 777 + i + 1, &snaps[a][d], &snaps[b][d]).a_won { w += 1; } t += 1; } }
        println!("head to head over {n} days: {la} beats {lb} in {} % of fights", w * 100 / t.max(1));
    }
}
type Routine = fn(day: u32, m: u32, p: &Pet) -> Vec<Act>;
fn at(m: u32, h: u32, mi: u32) -> bool { m == h * 60 + mi }

fn balanced(_d: u32, m: u32, _p: &Pet) -> Vec<Act> {
    if at(m,8,0) { vec![Act::Wake, Act::Eat(Food::Apple)] } else if at(m,10,30) { vec![Act::Play] } else if at(m,18,0) { vec![Act::Wash] }
    else if at(m,13,0) { vec![Act::Eat(Food::Apple)] } else if at(m,15,30) { vec![Act::Sport] }
    else if at(m,18,30) { vec![Act::Eat(Food::Pizza)] } else if at(m,20,30) { vec![Act::Mini(2), Act::Drink(Drink::Beer)] }
    else if at(m,23,30) { if _p.full < 55 * P { vec![Act::Eat(Food::Salad), Act::Sleep] } else { vec![Act::Sleep] } } else { vec![] }
}
fn sporty(_d: u32, m: u32, _p: &Pet) -> Vec<Act> {
    if at(m,7,30) { vec![Act::Wake, Act::Eat(Food::Apple), Act::Eat(Food::Salad)] } else if at(m,9,30) { vec![Act::Sport] }
    else if at(m,12,30) { if _p.dirt > 45 * P { vec![Act::Wash, Act::Eat(Food::Pizza)] } else { vec![Act::Eat(Food::Pizza)] } } else if at(m,16,30) { vec![Act::Sport] }
    else if at(m,18,30) { if _p.full < 45 * P { vec![Act::Eat(Food::Apple), Act::Eat(Food::Salad)] } else { vec![Act::Eat(Food::Apple)] } }
    else if at(m,17,0) { vec![Act::Wash] } else if at(m,20,30) { vec![Act::Play] }
    else if at(m,22,30) { vec![Act::Sleep] } else { vec![] }
}
fn lazy_treats(d: u32, m: u32, _p: &Pet) -> Vec<Act> {
    if at(m,10,0) { vec![Act::Wake, Act::Eat(Food::Pizza)] } else if at(m,14,0) { vec![Act::Eat(Food::Cake), Act::Drink(Drink::Cola)] }
    else if at(m,16,0) { if d % 2 == 0 { vec![Act::Sport] } else { vec![Act::Play] } }
    else if at(m,19,0) { vec![Act::Eat(if _p.full < 40 * P { Food::Pizza } else { Food::Frik })] } else if at(m,21,30) { if _p.dirt > 55 * P { vec![Act::Wash, Act::Mini(1)] } else { vec![Act::Mini(1)] } }
    else if at(m,23,59) { if _p.full < 50 * P { vec![Act::Eat(Food::Apple)] } else { vec![] } }
    else if at(m,1,0) { vec![Act::Sleep] } else { vec![] }
}
fn party(d: u32, m: u32, _p: &Pet) -> Vec<Act> {
    if at(m,11,0) { if _p.dirt > 50 * P { vec![Act::Wake, Act::Wash, Act::Drink(Drink::Water), Act::Eat(Food::Frik)] } else { vec![Act::Wake, Act::Drink(Drink::Water), Act::Eat(Food::Frik)] } }
    else if at(m,14,0) { if d % 2 == 0 { vec![Act::Sport] } else { vec![Act::Eat(Food::Apple)] } }
    else if at(m,16,30) { vec![Act::Drink(Drink::Beer), Act::Friend] }
    else if at(m,19,0) { vec![Act::Eat(Food::Pizza)] }
    else if at(m,21,0) { vec![Act::Drink(Drink::Beer), Act::Friend] }
    else if at(m,23,30) { vec![Act::Mini(2)] }
    else if at(m,1,30) { if _p.full < 50 * P { vec![Act::Drink(Drink::Water), Act::Eat(Food::Apple)] } else { vec![Act::Drink(Drink::Water)] } }
    else if at(m,3,0) { vec![Act::Sleep] } else { vec![] }
}
/// Reactive "reader": 5 check-ins, picks by what the pet shows (bands), no fixed plan.
fn reader(_d: u32, m: u32, p: &Pet) -> Vec<Act> {
    let mut v = vec![];
    if [8*60, 12*60, 16*60, 20*60, 23*60+30].contains(&m) {
        if m == 8*60 { v.push(Act::Wake); }
        if p.full < 50 * P { v.push(Act::Eat(if p.full < 35 * P { Food::Pizza } else { Food::Apple })); }
        if p.stim < 40 * P && p.energy > 35 * P { v.push(Act::Play); }
        if p.dirt > 50 * P { v.push(Act::Wash); }
        if m == 16*60 && p.strain < 20 * P && p.energy > 40 * P && p.full > 40 * P { v.push(Act::Sport); }
        if m == 20*60 && p.load < 15 * P { v.push(Act::Drink(Drink::Beer)); }
        if m == 23*60+30 { if p.full < 55 * P { v.push(Act::Eat(Food::Salad)); } v.push(Act::Sleep); }
    }
    v
}
fn awake_hours(m: u32) -> bool { m >= 8 * 60 }
fn spam_p(m: u32, every: u32, a: Act, p: &Pet) -> Vec<Act> {
    let mut v: Vec<Act> = balanced(0, m, p).into_iter().filter(|x| *x != Act::Sport && !matches!(x, Act::Drink(Drink::Beer))).collect();
    if awake_hours(m) && m % every == 0 { v.push(a); }
    v
}
#[allow(dead_code)]
fn spam(m: u32, every: u32, a: Act) -> Vec<Act> {
    // spammer still feeds and sleeps sensibly so only the spammed action differs
    let mut v = base(m);
    if awake_hours(m) && m % every == 0 { v.push(a); }
    v
}
fn base(m: u32) -> Vec<Act> {     // placeholder, replaced below
    let _ = m; vec![]
}
fn spam_feed(_d: u32, m: u32, _p: &Pet) -> Vec<Act> {
    if at(m,8,0) { vec![Act::Wake] } else if at(m,23,58) { vec![Act::Sleep] }
    else if awake_hours(m) && m % 30 == 0 { vec![Act::Eat(Food::Pizza)] } else { vec![] } }
fn spam_play(_d: u32, m: u32, p: &Pet) -> Vec<Act> { spam_p(m, 30, Act::Play, p) }
fn spam_sport(_d: u32, m: u32, p: &Pet) -> Vec<Act> { spam_p(m, 30, Act::Sport, p) }
fn spam_booze(_d: u32, m: u32, p: &Pet) -> Vec<Act> { spam_p(m, 30, Act::Drink(Drink::Beer), p) }
fn spam_cake(_d: u32, m: u32, p: &Pet) -> Vec<Act> { spam_p(m, 30, Act::Eat(Food::Cake), p) }
fn spam_wash(_d: u32, m: u32, p: &Pet) -> Vec<Act> { spam_p(m, 30, Act::Wash, p) }
fn spam_water(_d: u32, m: u32, p: &Pet) -> Vec<Act> { spam_p(m, 30, Act::Drink(Drink::Water), p) }
fn never_drinks(d: u32, m: u32, p: &Pet) -> Vec<Act> { balanced(d, m, p).into_iter().filter(|a| !matches!(a, Act::Drink(_))).collect() }
fn spam_sleep(_d: u32, m: u32, p: &Pet) -> Vec<Act> { let mut v = spam_p(m, 100000, Act::Relax, p); if awake_hours(m) && m % 30 == 0 && !p.sleeping { v.push(Act::Sleep); } v }
fn spam_med(_d: u32, m: u32, _p: &Pet) -> Vec<Act> { // neglect + medicine only
    if at(m,12,0) { vec![Act::Eat(Food::Pizza)] } else if m % 30 == 0 { vec![Act::Med] } else { vec![] } }
fn old_habits(_d: u32, m: u32, p: &Pet) -> Vec<Act> { // v1 reflex: top every bar up to max at each of 4 check-ins
    let mut v = vec![];
    if [9*60, 13*60, 18*60, 23*60].contains(&m) {
        if m == 9*60 { v.push(Act::Wake); }
        let mut f = p.full; while f < 95 * P { v.push(Act::Eat(Food::Pizza)); f += 40 * P; }
        v.push(Act::Play); v.push(Act::Play); v.push(Act::Sport); v.push(Act::Wash);
        if m == 23*60 { v.push(Act::Sleep); }
    }
    v
}
fn visit(m: u32) -> Vec<Act> { if at(m,15,0) { vec![Act::Station(0), Act::Station(1), Act::Station(2)] } else { vec![] } }
fn balanced_station(d: u32, m: u32, p: &Pet) -> Vec<Act> { let mut v = balanced(d, m, p); v.extend(visit(m)); v }
fn twice_station(d: u32, m: u32, p: &Pet) -> Vec<Act> { let mut v = twice_a_day(d, m, p); v.extend(visit(m)); v }
fn once_station(d: u32, m: u32, p: &Pet) -> Vec<Act> { let mut v = once_a_day(d, m, p); v.extend(visit(m)); v }
fn only_station(_d: u32, m: u32, _p: &Pet) -> Vec<Act> { visit(m) }
fn party_station(d: u32, m: u32, p: &Pet) -> Vec<Act> { let mut v = party(d, m, p); v.extend(visit(m)); v }
fn party_clinic(d: u32, m: u32, p: &Pet) -> Vec<Act> { let mut v = party(d, m, p); if d == 12 && at(m,15,0) { v.push(Act::Station(3)); } v }
fn lazy_clinic(d: u32, m: u32, p: &Pet) -> Vec<Act> { let mut v = lazy_treats(d, m, p); if d == 12 && at(m,15,0) { v.push(Act::Station(3)); } v }
fn once_a_day(_d: u32, m: u32, _p: &Pet) -> Vec<Act> { if at(m,12,0) { vec![Act::Eat(Food::Pizza), Act::Play] } else { vec![] } }
fn twice_a_day(_d: u32, m: u32, _p: &Pet) -> Vec<Act> {
    if at(m,9,0) { vec![Act::Wake, Act::Eat(Food::Pizza), Act::Eat(Food::Apple), Act::Play] } else if at(m,21,0) { vec![Act::Eat(Food::Pizza), Act::Eat(Food::Apple), Act::Wash, Act::Play, Act::Sleep] } else { vec![] } }
fn absent(_d: u32, _m: u32, _p: &Pet) -> Vec<Act> { vec![] }
fn short_sleeper(_d: u32, m: u32, p: &Pet) -> Vec<Act> { // balanced, but woken after five hours every night
    if at(m,4,30) { return vec![Act::Wake]; }
    let mut v = balanced(0, m, p); v.retain(|a| *a != Act::Wake); v }

/// Does the routine have an event at this minute, whatever the pet's state?
fn fires(r: Routine, d: u32, m: u32) -> bool {
    let mut lo = Pet::new(); lo.full = 0; lo.hyd = 0; lo.energy = MAX; lo.stim = 0; lo.dirt = MAX; lo.strain = 0; lo.load = 0; lo.health = 0; lo.fit = 0;
    let mut hi = Pet::new(); hi.full = MAX; hi.hyd = MAX; hi.dirt = 0;
    !r(d, m, &lo).is_empty() || !r(d, m, &hi).is_empty() || !r(d, m, &Pet::new()).is_empty()
}
/// One simulated minute of the harness: bring the pet up to date only when something happens.
fn drive(p: &mut Pet, r: Routine, d: u32, m: u32, water: bool) {
    let now = d as i64 * 1440 + m as i64;
    if !p.delta { p.update_to(now); }
    if !fires(r, d, m) { return; }
    p.update_to(now);
    let mut acts = r(d, m, p);
    if water && !acts.is_empty() && acts != vec![Act::Sleep] && p.hyd < 55 * P {
        let w = if acts.last() == Some(&Act::Sleep) { acts.len() - 1 } else { acts.len() }; acts.insert(w, Act::Drink(Drink::Water)); if p.hyd < 25 * P { acts.insert(w, Act::Drink(Drink::Water)); } }
    // habits of a player who looks at the Body page: eat richer when thin, lighter when heavy (not the junk-food lifestyles); insulin when diabetic
    if p.habits { for a in acts.iter_mut() { if let Act::Eat(f) = a {
        if p.weight < 42 * P && matches!(f, Food::Salad | Food::Apple) { *f = Food::Pizza; }
        else if p.weight > 60 * P && matches!(f, Food::Pizza | Food::Frik) { *f = Food::Apple; }
        else if p.weight > 60 * P && *f == Food::Cake { *f = Food::Salad; } } } }
    if !acts.is_empty() && p.diabetic && p.ins_min <= 120 && !p.demo { acts.insert(0, Act::Insulin); }
    let any = !acts.is_empty();
    for a in acts { p.act(a); }
    if any { p.note_wake(false); p.last_obs = p.disp(); if p.demo { p.demo_policy(); } }
}
struct Res { depd: f32, refused: u32, weight: i32, dep: bool, diab: bool, name: String, alive: bool, gone_day: f32, mood: i32, good: i64, health: i32, minh: i32, fit: i32, presses: f32, inj: u32, groggy: u32, grumpy: u32,
             band: [i64; 5], auto: f32, segs: u64, wakes30: f32, wakes120: f32, obs: f32, leaving: f32 }
fn run(name: &str, r: Routine, days: u32, delta: bool) -> Res {
    let mut p = Pet::new(); p.delta = delta; p.last_obs = p.disp();
    p.demo = name.starts_with("demo"); p.walks = name.contains("walks");
    if let Some(seed) = name.split('#').nth(1) { p.rng = 0x9E37_79B9u32.wrapping_mul(seed.parse::<u32>().unwrap() + 1) | 1; }
    let water = !p.demo && !name.contains("never drinks");
    p.habits = !(name.starts_with("lazy") || name.starts_with("party") || (name.starts_with("spam") && !(name.contains("water") || name.contains("wash") || name.contains("sleep"))) || name.starts_with("old") || name.contains("a day") || name.starts_with("neglect"));
    'o: for d in 0..days {
        for m in (if d == 0 { 480 } else { 0 })..1440u32 { drive(&mut p, r, d, m, water); if p.gone { break 'o; } }
        p.update_to((d as i64 + 1) * 1440);
        if p.gone { break 'o; }
    }
    p.note_wake(false);
    let dl = p.mins as f32 / 1440.0; let t = p.mins.max(1);
    Res { depd: p.dep_min as f32 / 1440.0, refused: p.refused, weight: pc(p.weight), dep: p.dependent, diab: p.diabetic, name: name.into(), alive: !p.gone, gone_day: (p.t as f32) / 1440.0, mood: (p.mood_sum / t) as i32 / P, good: p.good_min * 100 / t, health: pc(p.health), minh: pc(p.min_health), fit: pc(p.fit),
        presses: p.presses as f32 / dl, inj: p.injuries, groggy: p.groggy, grumpy: p.grumpy, band: [p.band[0]*100/t, p.band[1]*100/t, p.band[2]*100/t, p.band[3]*100/t, p.band[4]*100/t],
        auto: p.auto_acts as f32 / dl, segs: p.segs, wakes30: (p.obs_changes + p.auto_acts + p.cap30) as f32 / dl, wakes120: (p.obs_changes + p.auto_acts + p.cap120) as f32 / dl,
        obs: (p.obs_changes + p.auto_acts) as f32 / dl, leaving: p.leaving_total as f32 / 60.0 }
}
fn show(o: &Res, d: &Res) {
    let out = |r: &Res| if r.alive { "alive".to_string() } else { format!("gone d{:.1}", r.gone_day) };
    println!("| {} | {} / {} | {} / {} | {} / {} | {} / {} | {} / {} | {:.1} / {:.1} | {} / {} | {} / {} | {} / {} | {:.0} | {:.0} | {:.0} | {:.0} | {} |",
        o.name, out(o), out(d), o.mood, d.mood, o.health, d.health, o.minh, d.minh, o.fit, d.fit, o.presses, d.presses, o.inj, d.inj, o.groggy, d.groggy, o.grumpy, d.grumpy,
        d.segs as f32 / (d.gone_day - 0.33).max(0.1), d.obs, d.wakes30, d.wakes120, if !o.name.starts_with("demo") { format!("weight {} / {}; deprived days {:.0} / {:.0}; refused {} / {}{}{}", o.weight, d.weight, o.depd, d.depd, o.refused, d.refused, if o.dep || d.dep { " DEPENDENT" } else { "" }, if o.diab || d.diab { " DIABETIC" } else { "" }) } else { format!("{}/{}/{}/{}/{} vs {}/{}/{}/{}/{}; auto {:.1} / {:.1}; leaving {:.0} / {:.0}", o.band[0], o.band[1], o.band[2], o.band[3], o.band[4], d.band[0], d.band[1], d.band[2], d.band[3], d.band[4], o.auto, d.auto, o.leaving, d.leaving) + &format!(" weight {} / {}{}", o.weight, d.weight, if o.dep || d.dep { " DEPENDENT" } else { "" }) });
}
fn main() {
    if std::env::args().any(|a| a == "battle") { battle::matrix(); lifestyle(false); lifestyle(true); money_runs(); return; }
    let days = 30;
    println!("each cell: step oracle / delta engine");
    println!("| routine | outcome | avg mood | end health | min health | end fit | presses/day | injuries | groggy | grumpy | delta segments/day | status changes/day | engine wakes/day cap 30 min | cap 2 h | demo bands |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let all: Vec<(&str, Routine)> = vec![
        ("balanced", balanced), ("sporty", sporty), ("lazy with treats", lazy_treats), ("party animal", party), ("reader (reactive)", reader),
        ("short sleeper (woken after 5 h)", short_sleeper),
        ("spam feed (pizza /30min)", spam_feed), ("spam play", spam_play), ("spam sport", spam_sport), ("spam beer", spam_booze), ("spam cake", spam_cake),
        ("spam water", spam_water), ("never drinks (else balanced)", never_drinks), ("spam sleep", spam_sleep), ("spam wash", spam_wash), ("old habits (max all bars)", old_habits), ("neglect + spam medicine", spam_med),
        ("twice a day", twice_a_day), ("once a day", once_a_day), ("absent", absent),
        ("balanced + stations daily", balanced_station), ("party animal + stations daily", party_station), ("twice a day + stations daily", twice_station), ("once a day + stations daily", once_station), ("stations daily, nothing else", only_station), ("party animal + clinic on day 12", party_clinic), ("lazy with treats + clinic on day 12", lazy_clinic),
        ("demo alone #1", nobody), ("demo alone #2", nobody), ("demo alone #3", nobody), ("demo + daily helper #1", helper), ("demo + daily helper #2", helper), ("demo alone #4", nobody), ("demo alone #5", nobody), ("demo walks #1", nobody), ("demo walks #2", nobody), ("demo walks #3", nobody),
    ];
    for (n, r) in all { let o = run(n, r, days, false); let d = run(n, r, days, true); show(&o, &d); }
}

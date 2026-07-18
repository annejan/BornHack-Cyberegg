//! Game balance simulator — runs all player profiles and outputs a summary
//! table matching the Python simulation format.
//!
//! Usage: `cargo run --bin simulate_game --features simulator`
//! Or:    `make simulate-game`

use bornhack_aegg::game::engine::{FoodKind, GameState, PetKind, Phase};

// ---------------------------------------------------------------------------
// Player policies
// ---------------------------------------------------------------------------

trait Policy {
    fn name(&self) -> &'static str;
    fn act(&self, state: &mut GameState, tick: u32);
    /// Next tick this policy wants to check.  Default: every tick.
    fn next_check(&self, current_tick: u32) -> u32 {
        current_tick + 1
    }
}

/// Shared optimal action logic used by several policies.
fn optimal_act(state: &mut GameState) {
    if state.phase != Phase::Active {
        return;
    }

    if state.is_sleeping {
        if state.tired < 13107 {
            state.wake();
        }
        return;
    }

    if state.tired > 45874 {
        state.sleep();
        return;
    }
    if state.hunger > 32768 {
        state.feed(FoodKind::Apple);
        return;
    }
    if state.sick > 32768 {
        state.heal();
        return;
    }
    if state.miserable > 32768 {
        state.play();
        return;
    }
    if state.hunger > 16384 {
        state.feed(FoodKind::Apple);
    }
}

// ── Perfect: checks every tick ──────────────────────────────────────────

struct PerfectPolicy;
impl Policy for PerfectPolicy {
    fn name(&self) -> &'static str {
        "perfect"
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        optimal_act(state);
    }
}

// ── Perfect minus one action ────────────────────────────────────────────

struct PerfectNoFeed;
impl Policy for PerfectNoFeed {
    fn name(&self) -> &'static str {
        "perfect_no_feed"
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        if state.phase != Phase::Active {
            return;
        }
        if state.is_sleeping {
            if state.tired < 13107 {
                state.wake();
            }
            return;
        }
        if state.tired > 45874 {
            state.sleep();
            return;
        }
        if state.sick > 32768 {
            state.heal();
            return;
        }
        if state.miserable > 32768 {
            state.play();
        }
    }
}

struct PerfectNoHeal;
impl Policy for PerfectNoHeal {
    fn name(&self) -> &'static str {
        "perfect_no_heal"
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        if state.phase != Phase::Active {
            return;
        }
        if state.is_sleeping {
            if state.tired < 13107 {
                state.wake();
            }
            return;
        }
        if state.tired > 45874 {
            state.sleep();
            return;
        }
        if state.hunger > 32768 {
            state.feed(FoodKind::Apple);
            return;
        }
        if state.miserable > 32768 {
            state.play();
            return;
        }
        if state.hunger > 16384 {
            state.feed(FoodKind::Apple);
        }
    }
}

struct PerfectNoRest;
impl Policy for PerfectNoRest {
    fn name(&self) -> &'static str {
        "perfect_no_rest"
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        if state.phase != Phase::Active {
            return;
        }
        if state.is_sleeping {
            state.wake();
            return;
        }
        if state.hunger > 32768 {
            state.feed(FoodKind::Apple);
            return;
        }
        if state.sick > 32768 {
            state.heal();
            return;
        }
        if state.miserable > 32768 {
            state.play();
            return;
        }
        if state.hunger > 16384 {
            state.feed(FoodKind::Apple);
        }
    }
}

// ── Interval-based attentive ────────────────────────────────────────────

struct AttentivePolicy {
    label: &'static str,
    interval: u32,
}

impl Policy for AttentivePolicy {
    fn name(&self) -> &'static str {
        self.label
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        optimal_act(state);
    }
    fn next_check(&self, tick: u32) -> u32 {
        let rem = tick % self.interval;
        if rem == 0 {
            tick + self.interval
        } else {
            tick + self.interval - rem
        }
    }
}

// ── Night owl: never sleeps, checks every minute ────────────────────────

struct NightOwlPolicy;
impl Policy for NightOwlPolicy {
    fn name(&self) -> &'static str {
        "night_owl"
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        if state.phase != Phase::Active {
            return;
        }
        if state.is_sleeping {
            state.wake();
            return;
        }
        if state.hunger > 32768 {
            state.feed(FoodKind::Apple);
            return;
        }
        if state.sick > 32768 {
            state.heal();
            return;
        }
        if state.miserable > 32768 {
            state.play();
        }
    }
    fn next_check(&self, tick: u32) -> u32 {
        let rem = tick % 6;
        if rem == 0 { tick + 6 } else { tick + 6 - rem }
    }
}

// ── Absent: never interacts ─────────────────────────────────────────────

struct AbsentPolicy;
impl Policy for AbsentPolicy {
    fn name(&self) -> &'static str {
        "absent"
    }
    fn act(&self, _state: &mut GameState, _tick: u32) {}
    fn next_check(&self, _tick: u32) -> u32 {
        u32::MAX
    }
}

// ── Feed and sleep only ─────────────────────────────────────────────────

struct FeedAndSleepPolicy;
impl Policy for FeedAndSleepPolicy {
    fn name(&self) -> &'static str {
        "feed_and_sleep"
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        if state.phase != Phase::Active {
            return;
        }
        if state.is_sleeping {
            if state.tired < 13107 {
                state.wake();
            }
            return;
        }
        if state.tired > 45874 {
            state.sleep();
            return;
        }
        if state.hunger > 32768 {
            state.feed(FoodKind::Apple);
        }
    }
    fn next_check(&self, tick: u32) -> u32 {
        let rem = tick % 90;
        if rem == 0 { tick + 90 } else { tick + 90 - rem }
    }
}

// ── Feed, sleep, heal ───────────────────────────────────────────────────

struct FeedSleepHealPolicy;
impl Policy for FeedSleepHealPolicy {
    fn name(&self) -> &'static str {
        "feed_sleep_heal"
    }
    fn act(&self, state: &mut GameState, _tick: u32) {
        if state.phase != Phase::Active {
            return;
        }
        if state.is_sleeping {
            if state.tired < 13107 {
                state.wake();
            }
            return;
        }
        if state.tired > 45874 {
            state.sleep();
            return;
        }
        if state.hunger > 32768 {
            state.feed(FoodKind::Apple);
            return;
        }
        if state.sick > 32768 {
            state.heal();
        }
    }
    fn next_check(&self, tick: u32) -> u32 {
        let rem = tick % 90;
        if rem == 0 { tick + 90 } else { tick + 90 - rem }
    }
}

// ---------------------------------------------------------------------------
// Simulation runner
// ---------------------------------------------------------------------------

struct SimResult {
    name: &'static str,
    left: bool,
    days: f64,
    ticks: u32,
    steps: u32,
    hunger: u16,
    tired: u16,
    miserable: u16,
    sick: u16,
    action_count: u32,
}

fn run_profile(policy: &dyn Policy, seed: u64, max_days: u32) -> SimResult {
    let ticks_per_day: u32 = 8640;
    let max_ticks = max_days * ticks_per_day;

    let mut state = GameState::new_egg(seed, PetKind::Bartholomeus);
    let mut tick: u32 = 0;
    let mut action_count: u32 = 0;
    let mut steps: u32 = 0;

    while tick < max_ticks && state.phase != Phase::Gone {
        state.update(tick);
        let before = state.active_action;
        policy.act(&mut state, tick);
        if state.active_action != before && state.active_action.is_some() {
            action_count += 1;
        }

        let next_engine = state.next_wake_tick();
        let next_policy = policy.next_check(tick);
        tick = next_engine.min(next_policy).max(tick + 1).min(max_ticks);
        steps += 1;
    }

    let days = state.age_ticks as f64 / ticks_per_day as f64;
    SimResult {
        name: policy.name(),
        left: state.phase == Phase::Gone,
        days,
        ticks: state.age_ticks,
        steps,
        hunger: state.hunger,
        tired: state.tired,
        miserable: state.miserable,
        sick: state.sick,
        action_count,
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let seed = 42u64;
    let max_days = 60u32;
    let ticks_per_hour = 360.0f64;

    let policies: Vec<Box<dyn Policy>> = vec![
        Box::new(PerfectPolicy),
        Box::new(PerfectNoFeed),
        Box::new(PerfectNoHeal),
        Box::new(PerfectNoRest),
        Box::new(AttentivePolicy {
            label: "attentive_15min",
            interval: 90,
        }),
        Box::new(AttentivePolicy {
            label: "casual_30min",
            interval: 180,
        }),
        Box::new(AttentivePolicy {
            label: "busy_1hr",
            interval: 360,
        }),
        Box::new(NightOwlPolicy),
        Box::new(AbsentPolicy),
        Box::new(FeedAndSleepPolicy),
        Box::new(FeedSleepHealPolicy),
    ];

    println!(
        "{:<22} {:>4} {:>7} {:>8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6} {:>7}",
        "Profile",
        "Left",
        "Day",
        "Ticks",
        "Steps",
        "Hunger",
        "Tired",
        "Miser",
        "Sick",
        "Attn#",
        "Attn/h"
    );
    println!("{}", "─".repeat(102));

    for policy in &policies {
        let r = run_profile(policy.as_ref(), seed, max_days);
        let day_str = if r.left {
            format!("{:.1}", r.days)
        } else {
            format!("{:.1}+", r.days)
        };
        let attn_per_hour = if r.ticks > 0 {
            r.action_count as f64 / (r.ticks as f64 / ticks_per_hour)
        } else {
            0.0
        };

        println!(
            "{:<22} {:>4} {:>7} {:>8} {:>7} {:>7} {:>7} {:>7} {:>7} {:>6} {:>7.2}",
            r.name,
            if r.left { "YES" } else { "no" },
            day_str,
            r.ticks,
            r.steps,
            r.hunger,
            r.tired,
            r.miserable,
            r.sick,
            r.action_count,
            attn_per_hour,
        );
    }
}

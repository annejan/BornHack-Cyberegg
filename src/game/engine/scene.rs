//! What the background shows behind the pet (spec 14): the sky from the wall
//! clock, the floor items from dirt, the red cross
//! for a poorly one. A pure function of the pet; drawing lives in the UI.
//! Every line read here is a clock boundary or a dirt line, so the wake
//! stays exact: the floor lines are dirt lines and the sky switch is a timer
//! in the solver.

use super::GameState;
use super::Phase;
use super::demo::MINUTES_PER_DAY;
use super::needs::P;
use super::status;
use super::thresholds as t;

/// Things that can lie on the floor: crumbs, peel, can, sock, pizza box.
pub const FLOOR_ITEMS: usize = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sky {
    /// The wall clock is unknown.
    Plain,
    Day,
    Night,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BackgroundState {
    pub sky: Sky,
    /// Items on the floor (0..=`FLOOR_ITEMS`), in slot order.
    pub floor: u8,
    /// The red cross.
    pub poorly: bool,
}

impl BackgroundState {
    /// Bit 7 set (so `Some` never packs to 0), bits 0..1 sky (Plain 0, Day 1,
    /// Night 2), bits 2..4 floor, bit 6 poorly (bit 5 is
    /// unused: the grass bit was dropped, and the value is runtime-only).
    pub fn pack(self) -> u8 {
        let sky = match self.sky {
            Sky::Plain => 0,
            Sky::Day => 1,
            Sky::Night => 2,
        };
        0x80 | sky | (self.floor.min(FLOOR_ITEMS as u8) << 2) | ((self.poorly as u8) << 6)
    }
}

/// Inside the night window, wrapping midnight.
pub fn is_night(local_min: u16) -> bool {
    let (from, to) = (t::BG_NIGHT_FROM_MIN(), t::BG_NIGHT_TO_MIN());
    if from >= to {
        local_min >= from || local_min < to
    } else {
        (from..to).contains(&local_min)
    }
}

pub fn sky(day_min: Option<u16>) -> Sky {
    match day_min {
        None => Sky::Plain,
        Some(m) if is_night(m) => Sky::Night,
        Some(_) => Sky::Day,
    }
}

/// The five dirt lines (percent), in slot order.
pub fn floor_lines() -> [u8; FLOOR_ITEMS] {
    [
        t::FLOOR_CRUMBS_DIRT_PCT(),
        t::FLOOR_PEEL_DIRT_PCT(),
        t::FLOOR_CAN_DIRT_PCT(),
        t::FLOOR_SOCK_DIRT_PCT(),
        t::FLOOR_PIZZA_DIRT_PCT(),
    ]
}

/// How many lines `dirt` has reached (`dirt > line - 1`, the solver's form).
pub fn floor_items(dirt: u16) -> u8 {
    floor_lines()
        .iter()
        .filter(|&&pct| dirt as i32 > pct as i32 * P - 1)
        .count() as u8
}

/// Minutes from `m` to `x` on the 24 h clock; 0 counts as a whole day.
fn until(m: u16, x: u16) -> u32 {
    let day = MINUTES_PER_DAY as u32;
    match (x as u32 % day + day - m as u32 % day) % day {
        0 => day,
        n => n,
    }
}

/// Minutes until `sky(day_min)` next changes (1..=1440); `None` while the
/// clock is unknown.
pub fn minutes_to_switch(day_min: Option<u16>) -> Option<u32> {
    let m = day_min?;
    Some(until(m, t::BG_NIGHT_FROM_MIN()).min(until(m, t::BG_NIGHT_TO_MIN())))
}

/// `None` (draw nothing): not Active or Leaving, or hibernating.
pub fn background_state(s: &GameState) -> Option<BackgroundState> {
    if s.hibernating || !matches!(s.phase, Phase::Active | Phase::Leaving) {
        return None;
    }
    Some(BackgroundState {
        sky: sky(s.day_min),
        floor: floor_items(s.dirt),
        poorly: status::poorly(s) && s.pet_line().red(),
    })
}

/// `background_state(s).map_or(0, BackgroundState::pack)`
pub fn wake_view(s: &GameState) -> u8 {
    background_state(s).map_or(0, BackgroundState::pack)
}

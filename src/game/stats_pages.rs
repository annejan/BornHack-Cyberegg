//! The two pages of the stats view: "Needs" and "Body".
//!
//! Each row is a bar with its comfort band as ticks. The fill is red when
//! the value is out of its band (or past its alarm line, for mood).

use super::engine::PetStats;
use super::engine::thresholds as t;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Needs,
    Body,
}

impl Page {
    /// The other page.
    pub fn other(self) -> Page {
        match self {
            Page::Needs => Page::Body,
            Page::Body => Page::Needs,
        }
    }
}

/// One bar: label, value, the band ticks and whether the fill is red.
/// `cap` is a tick that is never a band edge (the health ceiling).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BarRow {
    pub label: &'static str,
    pub pct: u8,
    pub lo: Option<u8>,
    pub hi: Option<u8>,
    pub cap: Option<u8>,
    pub red: bool,
}

pub fn title(page: Page) -> &'static str {
    match page {
        Page::Needs => "Needs 1/2",
        Page::Body => "Body 2/2",
    }
}

/// A bar with a lower edge only; red below it.
fn floor(label: &'static str, pct: u8, lo: u8) -> BarRow {
    BarRow {
        label,
        pct,
        lo: Some(lo),
        hi: None,
        cap: None,
        red: pct < lo,
    }
}

/// A bar with a band; red outside it.
fn band(label: &'static str, pct: u8, lo: u8, hi: u8) -> BarRow {
    BarRow {
        label,
        pct,
        lo: Some(lo),
        hi: Some(hi),
        cap: None,
        red: pct < lo || pct > hi,
    }
}

/// The five bars of `page`. The numbers are the active thresholds, so the
/// Casual preset moves the ticks.
pub fn bars(page: Page, s: &PetStats) -> [BarRow; 5] {
    match page {
        Page::Needs => [
            band("Full", s.hunger, t::FULL_LO_PCT(), t::FULL_HI_PCT()),
            band("Water", s.water, t::WATER_LO_PCT(), t::WATER_HI_PCT()),
            floor("Energy", s.tired, t::ENERGY_LO_PCT()),
            band("Fun", s.fun, t::FUN_LO_PCT(), t::FUN_HI_PCT()),
            floor("Clean", s.clean, 100 - t::DIRT_HI_PCT()),
        ],
        Page::Body => [
            BarRow {
                cap: Some(s.ceiling),
                ..floor("Health", s.healthy, t::MOOD_HEALTH_FLOOR_PCT())
            },
            floor("Fit", s.fit, t::FIT_LO_PCT()),
            band(
                "Weight",
                s.weight_pct,
                t::WEIGHT_LO_PCT(),
                t::WEIGHT_HI_PCT(),
            ),
            BarRow {
                label: "Booze",
                pct: s.booze,
                lo: None,
                hi: Some(t::BOOZE_HI_PCT()),
                cap: None,
                red: s.booze > t::BOOZE_HI_PCT(),
            },
            BarRow {
                label: "Mood",
                pct: s.happy,
                lo: None,
                hi: None,
                cap: None,
                red: s.happy < t::DMG_MOOD_PCT(),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::engine::{GameState, PetKind, Phase};

    fn pet() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s
    }

    fn labels(rows: &[BarRow; 5]) -> [&'static str; 5] {
        [
            rows[0].label,
            rows[1].label,
            rows[2].label,
            rows[3].label,
            rows[4].label,
        ]
    }

    #[test]
    fn needs_page_rows_and_bands() {
        let mut g = pet();
        let st = g.stats(0);
        let rows = bars(Page::Needs, &st);
        assert_eq!(labels(&rows), ["Full", "Water", "Energy", "Fun", "Clean"]);
        let band = |i: usize| (rows[i].lo, rows[i].hi);
        assert_eq!(band(0), (Some(38), Some(78)));
        assert_eq!(band(1), (Some(38), Some(83)));
        assert_eq!(band(2), (Some(33), None));
        assert_eq!(band(3), (Some(33), Some(73)));
        assert_eq!(band(4), (Some(42), None));
        assert!(rows.iter().all(|r| r.cap.is_none()));
        assert!(rows.iter().all(|r| !r.red), "{rows:?}");
        assert_eq!(title(Page::Needs), "Needs 1/2");
    }

    #[test]
    fn body_page_marks_what_is_out() {
        let pct = |p: i32| (p * crate::game::engine::needs::P) as u16;
        let mut g = pet();
        let mut st = g.stats(0);
        let rows = bars(Page::Body, &st);
        assert_eq!(labels(&rows), ["Health", "Fit", "Weight", "Booze", "Mood"]);
        assert!(rows.iter().all(|r| r.label.len() <= 8));
        assert!(rows.iter().all(|r| !r.red), "{rows:?}");
        assert_eq!(title(Page::Body), "Body 2/2");

        g.weight = pct(70);
        g.booze = pct(45);
        g.fitness = pct(30);
        g.health = pct(45);
        g.mood = pct(15);
        st = g.stats(0);
        let rows = bars(Page::Body, &st);
        assert!(rows.iter().all(|r| r.red), "{rows:?}");
        assert_eq!((rows[0].lo, rows[0].hi), (Some(50), None));
        assert_eq!((rows[1].lo, rows[1].hi), (Some(43), None));
        assert_eq!((rows[2].lo, rows[2].hi), (Some(38), Some(63)));
        assert_eq!((rows[3].lo, rows[3].hi), (None, Some(38)));
        assert_eq!((rows[4].lo, rows[4].hi, rows[4].cap), (None, None, None));

        // Only the weight is out: just that row is red.
        let mut g = pet();
        g.weight = pct(70);
        let rows = bars(Page::Body, &g.stats(0));
        assert_eq!(rows.map(|r| r.red), [false, false, true, false, false]);

        let mut g = pet();
        g.fitness = pct(60);
        let st = g.stats(0);
        assert_eq!(bars(Page::Body, &st)[0].cap, Some(80));
    }

    /// Row `row` of the Needs page after `set` changed the pet.
    fn needs_row(row: usize, set: impl FnOnce(&mut GameState)) -> BarRow {
        let mut g = pet();
        set(&mut g);
        bars(Page::Needs, &g.stats(0))[row]
    }

    fn lvl(p: i32) -> u16 {
        (p * crate::game::engine::needs::P) as u16
    }

    #[test]
    fn needs_rows_turn_red_on_the_right_side() {
        // Classic: Full 38..78, Water 38..83, Energy 33.., Fun 33..73, Clean 42.. (dirt 58).
        // Each row: the band edges are fine, one step past them is red.
        let full = |p| needs_row(0, |g| g.fullness = lvl(p)).red;
        assert_eq!(
            (full(37), full(38), full(78), full(79)),
            (true, false, false, true)
        );
        let water = |p| needs_row(1, |g| g.hydration = lvl(p)).red;
        assert_eq!(
            (water(37), water(38), water(83), water(84)),
            (true, false, false, true)
        );
        // Energy has no upper edge: full energy is fine.
        let energy = |p| needs_row(2, |g| g.energy = lvl(p));
        assert_eq!(
            (energy(32).red, energy(33).red, energy(100).red),
            (true, false, false)
        );
        assert_eq!(energy(100).hi, None);
        let fun = |p| needs_row(3, |g| g.fun = lvl(p)).red;
        assert_eq!(
            (fun(32), fun(33), fun(73), fun(74)),
            (true, false, false, true)
        );
        // Clean is 100 - dirt: dirtier than 58 % is red, spotless is fine.
        let clean = |dirt| needs_row(4, |g| g.dirt = lvl(dirt));
        assert_eq!(
            (clean(59).red, clean(58).red, clean(0).red),
            (true, false, false)
        );
        assert_eq!(clean(59).pct, 41);
    }
}

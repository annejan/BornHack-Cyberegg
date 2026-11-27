//! BornPets v3 needs, tunable band edges and the threshold edge table.
//!
//! Levels are `u16`-scale (`P` = one percent). Band edges come from the
//! runtime [`Thresholds`] table; the other edges are fixed safety lines.

use super::thresholds::{self, Thresholds};

pub const P: i32 = 655;
pub const MAX: i32 = 65535;

// Fixed lines the status word, the display and the wake search read. Each is
// ALSO a threshold edge (needs, `build_edges`) or a signature line (solver,
// `sig`), so a segment never crosses one: one const, three users.
/// Fullness and water below this are in danger.
pub const NEED_DANGER_LO_PCT: u8 = 15;
/// Fullness and water above this are fixed edges; dirt above it is in danger.
pub const NEED_HI_EDGE_PCT: u8 = 85;
/// A fixed edge of the energy need (a segment never crosses it). A pet put to
/// bed at or above it is not tired, so `preview_sleep` warns it will be groggy.
pub const ENERGY_HI_EDGE_PCT: u8 = 85;
/// Dirt above this is in danger.
pub const DIRT_DANGER_PCT: u8 = NEED_HI_EDGE_PCT;
/// Health below this is in danger.
pub const HEALTH_DANGER_PCT: u8 = 30;
/// Fullness below this is hungry (a fixed edge, graded from peckish).
pub const HUNGRY_PCT: u8 = 25;
/// Energy below this is exhausted (a fixed edge, graded from sleepy).
pub const EXHAUSTED_PCT: u8 = 10;
/// Booze at or above this shows as tipsy (a fixed edge).
pub const TIPSY_PCT: u8 = 10;
/// Strain above this is sore (a fixed edge).
pub const SORE_PCT: u8 = 40;
/// A content pet at or above this mood shows as happy.
pub const HAPPY_MOOD_PCT: u8 = 80;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Need {
    Full = 0,
    Water,
    Energy,
    Fun,
    Dirt,
    Booze,
    Strain,
    Fit,
    Weight,
}

pub const NEEDS: usize = 9;
pub const MAX_EDGES: usize = 8;

macro_rules! band {
    ($( $fn:ident => $f:ident ),* $(,)?) => {
        $( #[inline] pub fn $fn() -> i32 { thresholds::$f() as i32 * P } )*
    };
}
band! {
    full_lo => FULL_LO_PCT, full_hi => FULL_HI_PCT,
    water_lo => WATER_LO_PCT, water_hi => WATER_HI_PCT,
    energy_lo => ENERGY_LO_PCT,
    fun_lo => FUN_LO_PCT, fun_hi => FUN_HI_PCT,
    dirt_hi => DIRT_HI_PCT,
    booze_hi => BOOZE_HI_PCT, booze_danger => BOOZE_DANGER_PCT,
    strain_inj => STRAIN_INJ_PCT,
    fit_lo => FIT_LO_PCT,
    weight_lo => WEIGHT_LO_PCT, weight_hi => WEIGHT_HI_PCT,
}

/// Per-need sorted, deduplicated threshold percentages.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct EdgeTable {
    pub edges: [[u8; MAX_EDGES]; NEEDS],
    pub len: [u8; NEEDS],
}

/// Merge `fixed` and `tuned` into one ascending, deduplicated row.
const fn merge(fixed: &[u8], tuned: &[u8]) -> ([u8; MAX_EDGES], u8) {
    let mut buf = [0u8; MAX_EDGES];
    let mut n = 0;
    let mut k = 0;
    while k < fixed.len() + tuned.len() {
        let v = if k < fixed.len() {
            fixed[k]
        } else {
            tuned[k - fixed.len()]
        };
        k += 1;
        // insertion into sorted position, skipping duplicates
        let mut i = 0;
        while i < n && buf[i] < v {
            i += 1;
        }
        if i < n && buf[i] == v {
            continue;
        }
        let mut j = n;
        while j > i {
            buf[j] = buf[j - 1];
            j -= 1;
        }
        buf[i] = v;
        n += 1;
    }
    (buf, n as u8)
}

/// Fixed edges merged with `t`'s band edges.
pub const fn build_edges(t: &Thresholds) -> EdgeTable {
    let rows = [
        merge(
            &[NEED_DANGER_LO_PCT, HUNGRY_PCT, NEED_HI_EDGE_PCT, 95],
            &[t.FULL_LO_PCT, t.FULL_HI_PCT],
        ),
        merge(
            &[NEED_DANGER_LO_PCT, 25, NEED_HI_EDGE_PCT, 90, 95],
            &[t.WATER_LO_PCT, t.WATER_HI_PCT],
        ),
        merge(&[EXHAUSTED_PCT, 50, 70, ENERGY_HI_EDGE_PCT], &[t.ENERGY_LO_PCT]),
        merge(&[40], &[t.FUN_LO_PCT, t.FUN_HI_PCT]),
        merge(&[25, DIRT_DANGER_PCT], &[t.DIRT_HI_PCT]),
        merge(&[TIPSY_PCT, 15, 30], &[t.BOOZE_HI_PCT, t.BOOZE_DANGER_PCT]),
        merge(&[15, SORE_PCT], &[t.STRAIN_INJ_PCT]),
        merge(&[25, 50, 60, 61, 70], &[t.FIT_LO_PCT]),
        merge(&[20, 65, 80], &[t.WEIGHT_LO_PCT, t.WEIGHT_HI_PCT]),
    ];
    let mut out = EdgeTable {
        edges: [[0; MAX_EDGES]; NEEDS],
        len: [0; NEEDS],
    };
    let mut i = 0;
    while i < NEEDS {
        out.edges[i] = rows[i].0;
        out.len[i] = rows[i].1;
        i += 1;
    }
    out
}

// Written once by `thresholds::install` at boot, before any reader.
static mut EDGES: EdgeTable = build_edges(&Thresholds::CLASSIC);

pub(super) fn install_edges(t: &Thresholds) {
    // Safety: boot-time single write, as for `thresholds::ACTIVE`.
    unsafe {
        EDGES = build_edges(t);
    }
}

/// Edge table built by `thresholds::install` (Classic until then).
#[inline]
pub fn edges() -> &'static EdgeTable {
    #[allow(static_mut_refs)]
    unsafe {
        &EDGES
    }
}

/// Level (0..=MAX) as a whole percent.
#[inline]
pub fn pct(level: i32) -> i32 {
    level / P
}

#[inline]
pub fn clamp_level(v: i32) -> i32 {
    v.clamp(0, MAX)
}

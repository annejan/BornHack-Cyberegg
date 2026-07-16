//! Drink choices for the Drink action.
//!
//! Same shape as `FoodKind`: each drink scales the baseline
//! `DRINK_DRUNK_GAIN` / `DRINK_DRAINED_RELIEF` / `DRINK_WEIGHT_GAIN`
//! thresholds by a percentage. `Beer` sits at 100% across the board —
//! the reference point the others scale against. `Water` and `Cola`
//! are non-alcoholic (0% drunk gain); the alcoholic drinks scale up
//! from there, feeding the same overweight/diabetes-style mechanic:
//! staying drunk for a sustained period leads to permanent alcoholism.

/// Which drink the player picked for the current Drink action.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub enum DrinkKind {
    Water,
    Cola,
    Beer,
    Wine,
    Whiskey,
}

impl DrinkKind {
    pub const ALL: [DrinkKind; 5] = [
        DrinkKind::Water,
        DrinkKind::Cola,
        DrinkKind::Beer,
        DrinkKind::Wine,
        DrinkKind::Whiskey,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DrinkKind::Water => "Water",
            DrinkKind::Cola => "Cola",
            DrinkKind::Beer => "Beer",
            DrinkKind::Wine => "Wine",
            DrinkKind::Whiskey => "Whiskey",
        }
    }

    /// (drunk_gain_pct, drained_relief_pct, weight_gain_pct) — applied
    /// as `base * pct / 100` against the DRINK_* thresholds.
    fn multipliers(self) -> (u32, u32, u32) {
        match self {
            DrinkKind::Water => (0, 100, 0),
            DrinkKind::Cola => (0, 150, 80),
            DrinkKind::Beer => (100, 120, 100),
            DrinkKind::Wine => (150, 110, 70),
            DrinkKind::Whiskey => (250, 140, 30),
        }
    }

    pub fn scale_drunk_gain(self, base: u16) -> u16 {
        let (pct, _, _) = self.multipliers();
        ((base as u32 * pct) / 100).min(u16::MAX as u32) as u16
    }

    pub fn scale_drained_relief(self, base: u16) -> u16 {
        let (_, pct, _) = self.multipliers();
        ((base as u32 * pct) / 100).min(u16::MAX as u32) as u16
    }

    pub fn scale_weight_gain(self, base: u16) -> u16 {
        let (_, _, pct) = self.multipliers();
        ((base as u32 * pct) / 100).min(u16::MAX as u32) as u16
    }
}

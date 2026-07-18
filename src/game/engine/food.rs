//! Food choices for the Feed action.
//!
//! Each food scales the baseline `FEED_HUNGER_RELIEF` / `FEED_WEIGHT_GAIN`
//! / `FEED_DRAINED_RELIEF` thresholds by a percentage — `Apple` sits at
//! 100% across the board, so it reproduces exactly what plain `Feed` did
//! before food choice existed. Unhealthier foods trade a bigger hunger
//! payoff for a much bigger weight payoff, tying directly into the
//! overweight → diabetes mechanic: repeatedly picking Pizza/Cake pushes
//! weight up far faster than Salad/Apple ever would.

/// Which food the player picked for the current Feed action.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-base", derive(defmt::Format))]
pub enum FoodKind {
    Salad,
    Apple,
    Burger,
    Pizza,
    Cake,
}

impl FoodKind {
    pub const ALL: [FoodKind; 5] = [
        FoodKind::Salad,
        FoodKind::Apple,
        FoodKind::Burger,
        FoodKind::Pizza,
        FoodKind::Cake,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FoodKind::Salad => "Salad",
            FoodKind::Apple => "Apple",
            FoodKind::Burger => "Burger",
            FoodKind::Pizza => "Pizza",
            FoodKind::Cake => "Cake",
        }
    }

    /// (hunger_relief_pct, weight_gain_pct, drained_relief_pct) — applied
    /// as `base * pct / 100` against the existing FEED_* thresholds.
    fn multipliers(self) -> (u32, u32, u32) {
        match self {
            FoodKind::Salad => (70, 30, 100),
            FoodKind::Apple => (100, 100, 100),
            FoodKind::Burger => (150, 250, 50),
            FoodKind::Pizza => (170, 300, 80),
            FoodKind::Cake => (60, 350, 200),
        }
    }

    /// Scale a base hunger-relief rate by this food's multiplier.
    pub fn scale_hunger_relief(self, base: u16) -> u16 {
        let (pct, _, _) = self.multipliers();
        ((base as u32 * pct) / 100).min(u16::MAX as u32) as u16
    }

    /// Scale a base weight-gain rate by this food's multiplier.
    pub fn scale_weight_gain(self, base: u16) -> u16 {
        let (_, pct, _) = self.multipliers();
        ((base as u32 * pct) / 100).min(u16::MAX as u32) as u16
    }

    /// Scale a base drained-relief rate by this food's multiplier.
    pub fn scale_drained_relief(self, base: u16) -> u16 {
        let (_, _, pct) = self.multipliers();
        ((base as u32 * pct) / 100).min(u16::MAX as u32) as u16
    }

    /// HEX price to feed this food. Healthy costs more than junk.
    pub fn hex_price(self) -> u32 {
        match self {
            FoodKind::Salad | FoodKind::Apple => 15,
            FoodKind::Burger | FoodKind::Pizza | FoodKind::Cake => 10,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Healthy foods (Salad, Apple) cost 15 HEX; unhealthy ones (Burger,
    /// Pizza, Cake) cost 10 — junk food is the cheap trap.
    #[test]
    fn hex_price_matches_health_tier() {
        assert_eq!(FoodKind::Salad.hex_price(), 15);
        assert_eq!(FoodKind::Apple.hex_price(), 15);
        assert_eq!(FoodKind::Burger.hex_price(), 10);
        assert_eq!(FoodKind::Pizza.hex_price(), 10);
        assert_eq!(FoodKind::Cake.hex_price(), 10);
    }
}

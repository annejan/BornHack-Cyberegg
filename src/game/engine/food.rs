//! Food choices for the Feed action. What each food does is in `doses`.

/// Which food the player picked for the current Feed action.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum FoodKind {
    Salad,
    Apple,
    Frikandel,
    Pizza,
    Cake,
}

impl FoodKind {
    pub const ALL: [FoodKind; 5] = [
        FoodKind::Salad,
        FoodKind::Apple,
        FoodKind::Frikandel,
        FoodKind::Pizza,
        FoodKind::Cake,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FoodKind::Salad => "Salad",
            FoodKind::Apple => "Apple",
            FoodKind::Frikandel => "Frikandel spec",
            FoodKind::Pizza => "Pizza",
            FoodKind::Cake => "Cake",
        }
    }

    /// The food's whole name, for the menu title (`label` is the short form
    /// that fits a row with its price).
    pub fn full_name(self) -> &'static str {
        match self {
            FoodKind::Frikandel => "Frikandel speciaal",
            other => other.label(),
        }
    }
}

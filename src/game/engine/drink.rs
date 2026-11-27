//! Drink choices for the Drink action. What each drink does is in `doses`.

/// Which drink the player picked for the current Drink action. The order is
/// the save byte (`DrinkKind::ALL` index): Coffee holds Cola's old slot 1, so
/// a save made mid-Cola loads as a coffee. Never reorder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub enum DrinkKind {
    Water,
    Coffee,
    Beer,
    Wine,
    Whiskey,
}

impl DrinkKind {
    pub const ALL: [DrinkKind; 5] = [
        DrinkKind::Water,
        DrinkKind::Coffee,
        DrinkKind::Beer,
        DrinkKind::Wine,
        DrinkKind::Whiskey,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DrinkKind::Water => "Water",
            DrinkKind::Coffee => "Coffee",
            DrinkKind::Beer => "Beer",
            DrinkKind::Wine => "Wine",
            DrinkKind::Whiskey => "Whiskey",
        }
    }

    /// Whether this drink raises `booze` at all (Water/Coffee never do). Used
    /// to pick an accurate post-drink toast.
    pub fn is_alcoholic(self) -> bool {
        !matches!(self, DrinkKind::Water | DrinkKind::Coffee)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_water_and_coffee_are_non_alcoholic() {
        assert!(!DrinkKind::Water.is_alcoholic());
        assert!(!DrinkKind::Coffee.is_alcoholic());
        assert!(DrinkKind::Beer.is_alcoholic());
        assert!(DrinkKind::Wine.is_alcoholic());
        assert!(DrinkKind::Whiskey.is_alcoholic());
    }
}

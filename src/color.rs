//! Shared EPD pixel colour type used across every panel driver and the simulator.

use embedded_graphics::pixelcolor::raw::RawU2;
use embedded_graphics::prelude::PixelColor;

/// EPD pixel — superset across every supported panel.
///
/// A colour the connected panel cannot reproduce is dropped to [`EpdColor::White`]
/// by that panel's driver (never substituted with a nearer ink). The SSD1675 and
///  are Black/White/Red; [`EpdColor::Yellow`] exists for the future
/// GDEY0154F51 four-colour panel and is mapped to White on today's drivers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpdColor {
    /// Black ink.
    Black,
    /// White background.
    White,
    /// Red chromatic ink.
    Red,
    /// Yellow chromatic ink (four-colour panels only).
    Yellow,
}

impl PixelColor for EpdColor {
    type Raw = RawU2;
}

/// Back-compatible alias: the UI is written against `TriColor`. Keeping the alias
/// lets every `DrawTarget<Color = TriColor>` bound resolve to [`EpdColor`] unchanged.
pub type TriColor = EpdColor;

/// Black pixel constant.
pub const BLACK: EpdColor = EpdColor::Black;
/// White pixel constant.
pub const WHITE: EpdColor = EpdColor::White;
/// Red pixel constant.
pub const RED: EpdColor = EpdColor::Red;

/// Simulator-only mapping to on-screen RGB. Yellow renders as RGB yellow so the
/// simulator can preview four-colour content even though hardware drops it to white.
#[cfg(feature = "simulator")]
impl From<EpdColor> for embedded_graphics::pixelcolor::Rgb888 {
    fn from(c: EpdColor) -> Self {
        use embedded_graphics::pixelcolor::Rgb888;
        match c {
            EpdColor::White => Rgb888::new(255, 255, 255),
            EpdColor::Black => Rgb888::new(0, 0, 0),
            EpdColor::Red => Rgb888::new(255, 0, 0),
            EpdColor::Yellow => Rgb888::new(255, 255, 0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tricolor_is_epdcolor_alias() {
        // The UI is written against `TriColor`; it must be the same type as EpdColor.
        let c: TriColor = EpdColor::Red;
        assert_eq!(c, EpdColor::Red);
    }

    #[test]
    fn consts_match_variants() {
        assert_eq!(BLACK, EpdColor::Black);
        assert_eq!(WHITE, EpdColor::White);
        assert_eq!(RED, EpdColor::Red);
    }

    #[test]
    fn yellow_exists_as_fourth_variant() {
        // Future GDEY0154F51 four-colour panel; no current UI draws it.
        let c = EpdColor::Yellow;
        assert_ne!(c, EpdColor::Red);
    }
}

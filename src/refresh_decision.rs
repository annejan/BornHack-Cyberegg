//! Pure helper for the SSD1675 refresh choice. `no_std`, host-testable.

/// True when the refresh has to move red ink: some pixel in the box
/// `(x, y, w, h)` is for `moves_red(x, y)` true. The caller's probe says a
/// pixel is dirty and red in the committed shadow or the pending frame, so a
/// change in the black plane only (red unchanged under it) gives false.
pub fn touches_red(x: u16, y: u16, w: u16, h: u16, moves_red: impl Fn(u16, u16) -> bool) -> bool {
    (y..y + h).any(|py| (x..x + w).any(|px| moves_red(px, py)))
}

#[cfg(test)]
mod tests {
    use super::touches_red;

    #[test]
    fn black_only_change_does_not_touch_red() {
        // Dirty box over black-only pixels; red sits elsewhere and is clean.
        assert!(!touches_red(0, 0, 8, 8, |_, _| false));
    }

    #[test]
    fn red_change_in_box_is_seen() {
        assert!(touches_red(0, 0, 8, 8, |x, y| (x, y) == (7, 7)));
        // A red pixel outside the box is not looked at.
        assert!(!touches_red(0, 0, 8, 8, |x, y| (x, y) == (8, 8)));
    }
}

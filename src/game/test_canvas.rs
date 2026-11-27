//! A 152 x 152 in-memory target for render tests (both panels are 152 x 152).

use core::convert::Infallible;

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::Rectangle;

use crate::{TriColor, WHITE};

const SIDE: i32 = 152;

/// A white-filled 152 x 152 canvas. Pixels outside it are ignored.
pub struct Canvas {
    px: [TriColor; (SIDE * SIDE) as usize],
}

impl Canvas {
    pub fn new() -> Self {
        Self {
            px: [WHITE; (SIDE * SIDE) as usize],
        }
    }

    /// Fill the whole canvas, standing in for the sprite behind the UI.
    pub fn fill(&mut self, c: TriColor) {
        self.px.fill(c);
    }

    /// The colour at (x, y); white outside the canvas.
    pub fn get(&self, x: i32, y: i32) -> TriColor {
        if (0..SIDE).contains(&x) && (0..SIDE).contains(&y) {
            self.px[(y * SIDE + x) as usize]
        } else {
            WHITE
        }
    }

    /// Does any pixel of `area` have colour `c`?
    pub fn any(&self, area: Rectangle, c: TriColor) -> bool {
        area.points().any(|p| self.get(p.x, p.y) == c)
    }
}

impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(SIDE as u32, SIDE as u32)
    }
}

impl DrawTarget for Canvas {
    type Color = TriColor;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
    where
        I: IntoIterator<Item = Pixel<TriColor>>,
    {
        for Pixel(p, c) in pixels {
            if (0..SIDE).contains(&p.x) && (0..SIDE).contains(&p.y) {
                self.px[(p.y * SIDE + p.x) as usize] = c;
            }
        }
        Ok(())
    }
}

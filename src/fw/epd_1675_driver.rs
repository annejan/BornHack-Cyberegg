//! [`EpdDriver`] implementation for the SSD1675/SSD1675B panel.
//!
//! Adapts the existing tri-colour [`EpdGfx`] (which already owns its B/W + Red +
//! work planes and the OTP-LUT machinery). The driver owns the host-side
//! [`PartialState`] ledger. [`RefreshMode::Full`] runs a temperature-compensated
//! tri-colour refresh (`update_tc`); [`RefreshMode::Fast`] runs the non-blink
//! partial waveform (`update_partial`) over the changed-pixel set. `update_partial`
//! self-promotes to a full ghost-reset waveform internally once the cumulative
//! changed-pixel counter trips `should_force_full()` (vendor `display.rs`), so the
//! adapter never selects that waveform itself. LUT-speed, temperature compensation,
//! variant detection, and the staged drive all stay internal to this adapter — they
//! are SSD1675 specifics, not part of the trait.

use crate::color::EpdColor;
use crate::epd_driver::{EpdDriver, PlaneAccess, RefreshMode};
use crate::fw::epd::{current_lut_speed, panel_temp_c10, EpdGfx};
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::prelude::{OriginDimensions, Size};
use embedded_graphics::primitives::Rectangle;
use embedded_graphics::Pixel;
use ssd1675::graphics::Color;
use ssd1675::partial::PartialState;

/// Map a unified [`EpdColor`] to the SSD1675 tri-colour pixel.
///
/// Yellow is dropped to White (the panel has no yellow ink).
#[inline]
fn to_ssd1675(c: EpdColor) -> Color {
    match c {
        EpdColor::Black => Color::Black,
        EpdColor::White | EpdColor::Yellow => Color::White,
        EpdColor::Red => Color::Red,
    }
}

/// SSD1675 [`EpdDriver`] adapter over [`EpdGfx`].
///
/// Owns the [`PartialState`] delta ledger. The inner [`EpdGfx`] already owns the
/// B/W, Red, and work plane buffers.
pub struct Ssd1675Driver<'a> {
    gfx: EpdGfx<'a>,
    partial_state: PartialState,
}

impl<'a> Ssd1675Driver<'a> {
    /// Build the adapter from an initialised [`EpdGfx`] and the taken
    /// [`PartialState`] ledger (see [`crate::fw::epd::partial_state_take`]).
    ///
    /// # Arguments
    ///
    /// * `gfx`           - Initialised tri-colour graphic display (OTP LUT already loaded).
    /// * `partial_state` - Host-side delta ledger returned by `epd::partial_state_take`.
    pub fn new(gfx: EpdGfx<'a>, partial_state: PartialState) -> Self {
        Self { gfx, partial_state }
    }

    /// Borrow the inner [`EpdGfx`] and [`PartialState`] together for the staged
    /// neighbourhood-erosion B/W drive run from `display_loop`.
    ///
    /// The staged path is an SSD1675-specific quality feature that operates
    /// directly on the panel's plane buffers and the host delta ledger (it is
    /// not expressible through the panel-agnostic [`EpdDriver`] trait). Exposing
    /// both fields through one split borrow lets the loop drive the staged
    /// waveform while still rendering the UI through this adapter's
    /// [`DrawTarget`]/[`EpdDriver`] surface.
    ///
    /// # Returns
    ///
    /// A `(&mut EpdGfx, &mut PartialState)` pair borrowed from this driver.
    pub fn staged_parts_mut(&mut self) -> (&mut EpdGfx<'a>, &mut PartialState) {
        (&mut self.gfx, &mut self.partial_state)
    }
}

impl OriginDimensions for Ssd1675Driver<'_> {
    fn size(&self) -> Size {
        self.gfx.size()
    }
}

impl DrawTarget for Ssd1675Driver<'_> {
    type Color = EpdColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<EpdColor>>,
    {
        // Translate EpdColor → ssd1675 Color and delegate to the inner DrawTarget.
        self.gfx
            .draw_iter(pixels.into_iter().map(|Pixel(p, c)| Pixel(p, to_ssd1675(c))))
    }

    fn fill_solid(&mut self, area: &Rectangle, color: EpdColor) -> Result<(), Self::Error> {
        self.gfx.fill_solid(area, to_ssd1675(color))
    }
}

impl PlaneAccess for Ssd1675Driver<'_> {
    /// Returns mutable references to the B/W and Red planes.
    ///
    /// The work buffer is omitted — it is internal to `EpdGfx` (used for sub-image
    /// scratch during `partial_update`).
    fn planes_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        let (black, red, _work) = self.gfx.all_buffers_mut();
        (black, red)
    }
}

impl EpdDriver for Ssd1675Driver<'_> {
    type Error = core::convert::Infallible;

    /// No-op: the SSD1675 panel is already initialised by `epd::init_epd` (OTP probe +
    /// LUT registration) before the adapter is constructed. The per-frame `reset` is
    /// issued inside `refresh`.
    async fn init(&mut self) -> Result<(), <Self as EpdDriver>::Error> {
        Ok(())
    }

    /// Push the current framebuffer to the panel.
    ///
    /// * [`RefreshMode::Full`] — temperature compensation + `reset` + `update_tc`.
    /// * [`RefreshMode::Fast`] — temperature compensation + `reset` + `update_partial`.
    ///   `update_partial` self-promotes to a full OTP-flash ghost-reset waveform
    ///   internally once [`PartialState::should_force_full`] trips the
    ///   `full_after_screens` threshold (vendor `display.rs`), keeping its own
    ///   changed-pixel/partial-count bookkeeping consistent. The adapter therefore
    ///   never selects that waveform itself — mirroring the real `display_loop`
    ///   partial path.
    ///
    /// `deep_sleep` is issued after every refresh to minimise quiescent power.
    async fn refresh(&mut self, mode: RefreshMode) -> Result<(), <Self as EpdDriver>::Error> {
        let speed = current_lut_speed();
        let panel_c10 = panel_temp_c10(self.gfx.variant());
        if panel_c10 != i16::MIN {
            self.gfx.set_active_temperature(panel_c10);
        }
        let _ = self.gfx.reset().await;
        match mode {
            RefreshMode::Full => {
                let _ = self.gfx.update_tc(speed).await;
            }
            RefreshMode::Fast => {
                let _ = self.gfx.update_partial(&mut self.partial_state, speed).await;
            }
        }
        let _ = self.gfx.deep_sleep().await;
        Ok(())
    }

    /// Put the controller into its lowest-power state.
    async fn deep_sleep(&mut self) -> Result<(), <Self as EpdDriver>::Error> {
        let _ = self.gfx.deep_sleep().await;
        Ok(())
    }
}

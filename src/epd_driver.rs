//! Unified e-paper driver trait.
//!
//! Every panel driver implements [`EpdDriver`]: the application draws through the
//! [`DrawTarget`] super-trait (in [`EpdColor`]), then calls [`EpdDriver::refresh`].
//! Each driver maps the requested [`RefreshMode`] to its panel's best waveform and
//! works around missing hardware features internally (per the design spec). The app
//! never selects a panel-specific waveform.

use crate::color::EpdColor;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::prelude::OriginDimensions;

/// Refresh quality the application requests. The driver maps it to the best the
/// panel supports and degrades internally.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefreshMode {
    /// High-quality full refresh (may flash); drives all ink planes incl. red.
    Full,
    /// Minimal non-blink update of only what changed since the last refresh.
    Fast,
}

/// One panel driver.
///
/// The driver owns its colour-plane framebuffers and the previous frame. The app
/// draws through [`DrawTarget`], then calls [`refresh`](Self::refresh). On
/// [`RefreshMode::Fast`] the driver diffs against the previous frame and drives only
/// the change (its own delta mechanism), choosing the fastest waveform from the
/// colours actually present and patching unused chromatic phases out.
// Single-executor embassy target — opaque futures need not be Send.
#[allow(async_fn_in_trait)]
pub trait EpdDriver: DrawTarget<Color = EpdColor> + OriginDimensions {
    /// Error produced by the underlying SPI/GPIO HAL.
    type Error;

    /// Reset + register init. Call once before the first refresh; safe to re-call to
    /// recover from a known-bad state.
    ///
    /// # Errors
    /// Propagates the underlying bus error.
    async fn init(&mut self) -> Result<(), <Self as EpdDriver>::Error>;

    /// Push the current framebuffer to the panel using `mode`.
    ///
    /// # Errors
    /// Propagates the underlying bus error.
    async fn refresh(&mut self, mode: RefreshMode) -> Result<(), <Self as EpdDriver>::Error>;

    /// Put the controller into its lowest-power state until the next `init`.
    ///
    /// # Errors
    /// Propagates the underlying bus error.
    async fn deep_sleep(&mut self) -> Result<(), <Self as EpdDriver>::Error>;
}

/// Raw access to a driver's B/W and Red planes for the direct-write sprite/PCX
/// blitter (`game::sprite_loader`). Decoupled from [`EpdDriver`] so the blitter does
/// not need a full driver, only mutable plane bytes.
pub trait PlaneAccess {
    /// Mutable `(black_plane, red_plane)`. Each is `ceil(width/8) * height` bytes; a
    /// set bit in `black` = white sub-pixel, a set bit in `red` = red sub-pixel
    /// (matching the SSD16xx plane convention).
    fn planes_mut(&mut self) -> (&mut [u8], &mut [u8]);
}

#[cfg(all(test, feature = "simulator"))]
mod tests {
    use super::*;
    use crate::color::EpdColor;
    use embedded_graphics::prelude::{OriginDimensions, Size};
    use embedded_graphics::draw_target::DrawTarget;
    use embedded_graphics::Pixel;
    use core::convert::Infallible;

    struct MockDriver {
        last_refresh: Option<RefreshMode>,
        init_called: bool,
        slept: bool,
        black: [u8; 8],
        red: [u8; 8],
    }

    impl OriginDimensions for MockDriver {
        fn size(&self) -> Size { Size::new(8, 8) }
    }

    impl DrawTarget for MockDriver {
        type Color = EpdColor;
        type Error = Infallible;
        fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Infallible>
        where I: IntoIterator<Item = Pixel<EpdColor>> {
            for _ in pixels {}
            Ok(())
        }
    }

    impl EpdDriver for MockDriver {
        type Error = Infallible;
        async fn init(&mut self) -> Result<(), Infallible> {
            self.init_called = true;
            Ok(())
        }
        async fn refresh(&mut self, mode: RefreshMode) -> Result<(), Infallible> {
            self.last_refresh = Some(mode);
            Ok(())
        }
        async fn deep_sleep(&mut self) -> Result<(), Infallible> {
            self.slept = true;
            Ok(())
        }
    }

    impl PlaneAccess for MockDriver {
        fn planes_mut(&mut self) -> (&mut [u8], &mut [u8]) {
            (&mut self.black, &mut self.red)
        }
    }

    /// A generic "UI" function: anything bound on the trait must compile.
    async fn ui_render<D: EpdDriver>(d: &mut D) -> Result<(), <D as EpdDriver>::Error> {
        d.init().await?;
        d.refresh(RefreshMode::Fast).await?;
        d.refresh(RefreshMode::Full).await?;
        d.deep_sleep().await
    }

    #[test]
    fn generic_ui_routes_modes() {
        let mut m = MockDriver {
            last_refresh: None, init_called: false, slept: false,
            black: [0; 8], red: [0; 8],
        };
        embassy_futures::block_on(ui_render(&mut m)).unwrap();
        assert!(m.init_called);
        assert_eq!(m.last_refresh, Some(RefreshMode::Full));
        assert!(m.slept);
    }

    #[test]
    fn plane_access_exposes_buffers() {
        let mut m = MockDriver {
            last_refresh: None, init_called: false, slept: false,
            black: [0; 8], red: [0; 8],
        };
        let (b, r) = m.planes_mut();
        b[0] = 0xFF;
        r[1] = 0xAA;
        assert_eq!(m.black[0], 0xFF);
        assert_eq!(m.red[1], 0xAA);
    }
}

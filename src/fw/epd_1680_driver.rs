//! [`EpdDriver`] implementation for the  panel.
//!
//! The shared badge UI is laid out for a 152×152 canvas (the SSD1675 panel
//! size). This driver presents `size() = 152×152` to the UI and owns a 152×152
//! B/W + Red *canvas* that the [`DrawTarget`]/[`PlaneAccess`] writes target. On
//! every [`refresh`](EpdDriver::refresh) it composes that canvas into the real
//! 200×200 panel planes — either centred with a white border, or upscaled to
//! fill the panel — then drives the panel. [`RefreshMode::Full`] uses the full
//! bi-colour OTP waveform (`0xF7`, shows red); [`RefreshMode::Fast`] encodes
//! per-pixel delta codes ([`build_code_planes`]) and drives the custom
//! `DELTA_LUT` (`0xC7`, 200 Hz frame rate, ignore-group L3 zeroed) — unchanged
//! pixels see no voltage, so a partial never flashes and never re-drives red.
//! Red and Yellow are dropped to white on the plane mapping where the panel
//! cannot show them.
//!
//! ## Upscaling
//!
//! With [`set_scale_fill(true)`](Driver::set_scale_fill) the 152×152
//! canvas is nearest-neighbour upscaled by "double every 4th pixel" (×1.25 →
//! 190×190) and centred in the 200×200 panel (a 5 px white border). With
//! `false` the canvas is placed 1:1 centred (a 24 px border). Both B/W and Red
//! planes scale identically.

use crate::color::EpdColor;
use crate::epd_driver::{EpdDriver, PlaneAccess, RefreshMode};
use ssd1680::partial::build_code_planes;
use ssd1680::{BoundingBox, Display, Interface};
use ssd1680::interface::DisplayInterface;
use embassy_nrf::gpio::{Input, Output};
use embassy_nrf::spim::Spim;
use embedded_graphics::Pixel;
use embedded_graphics::draw_target::DrawTarget;
use embedded_graphics::geometry::Dimensions;
use embedded_graphics::prelude::{OriginDimensions, PointsIter, Size};
use embedded_graphics::primitives::Rectangle;
use embedded_hal_bus::spi::ExclusiveDevice;

/// UI canvas width/height in pixels (the shared 152×152 badge layout).
const CANVAS_DIM: u16 = 152;
/// Canvas bytes per row (`152 / 8`).
const CANVAS_STRIDE: usize = (CANVAS_DIM as usize) / 8;
/// Canvas plane byte length (`152 / 8 * 152`).
pub const CANVAS_BYTES: usize = CANVAS_STRIDE * CANVAS_DIM as usize;
/// Upscaled canvas width when `scale_fill` is on (152 × 5 / 4 = 190).
const SCALED_DIM: usize = CANVAS_DIM as usize * 5 / 4;

/// Concrete  interface over SPI3 (mirrors the wiring built in `embassy.rs`).
pub type Iface<'a> = Interface<
    ExclusiveDevice<Spim<'a>, Output<'a>, embassy_time::Delay>,
    Input<'a>,
    Output<'a>,
    Output<'a>,
>;

///  [`EpdDriver`]. Generic over panel plane byte-length `N` (= `width/8 *
/// height`, e.g. 5000 for 200×200). The UI canvas is the fixed 152×152
/// [`CANVAS_BYTES`] size.
pub struct Driver<'a, const N: usize> {
    display: Display<Iface<'a>>,
    /// Panel width in pixels.
    width: u16,
    /// Panel height in pixels.
    height: u16,
    /// UI canvas B/W plane (152×152; set bit = white sub-pixel). DrawTarget here.
    canvas_bw: &'a mut [u8; CANVAS_BYTES],
    /// UI canvas Red plane (152×152; set bit = red sub-pixel).
    canvas_red: &'a mut [u8; CANVAS_BYTES],
    /// Panel B/W plane (composed from the canvas each refresh).
    panel_bw: &'a mut [u8; N],
    /// Panel Red plane.
    panel_red: &'a mut [u8; N],
    /// Previous panel B/W plane, for the `Fast` delta diff.
    prev: &'a mut [u8; N],
    /// Previous panel Red plane, for the `Fast` delta diff / `skip_red`.
    prev_red: &'a mut [u8; N],
    /// Scratch B/W code plane (`0x24` delta codes) for [`build_code_planes`].
    code_bw: &'a mut [u8; N],
    /// Scratch Red code plane (`0x26` delta codes) for [`build_code_planes`].
    code_red: &'a mut [u8; N],
    /// When true, upscale the canvas (×1.25) to fill the panel; else centre 1:1.
    scale_fill: bool,
}

impl<'a, const N: usize> Driver<'a, N> {
    /// Build a driver from a [`Display`], panel dimensions, the 152×152 canvas
    /// planes, the panel planes, and the initial `scale_fill` setting.
    ///
    /// # Arguments
    /// * `display`               - OTP-mode  display (not yet `init`ed).
    /// * `width`/`height`        - Panel dimensions (e.g. 200×200).
    /// * `canvas_bw`/`canvas_red`- 152×152 UI canvas planes ([`CANVAS_BYTES`] each).
    /// * `panel_bw`/`panel_red`/`prev`/`prev_red` - Panel planes, each `N` bytes.
    /// * `code_bw`/`code_red`    - Delta code-plane scratch, each `N` bytes.
    /// * `scale_fill`            - Upscale to fill (true) or centre 1:1 (false).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        display: Display<Iface<'a>>,
        width: u16,
        height: u16,
        canvas_bw: &'a mut [u8; CANVAS_BYTES],
        canvas_red: &'a mut [u8; CANVAS_BYTES],
        panel_bw: &'a mut [u8; N],
        panel_red: &'a mut [u8; N],
        prev: &'a mut [u8; N],
        prev_red: &'a mut [u8; N],
        code_bw: &'a mut [u8; N],
        code_red: &'a mut [u8; N],
        scale_fill: bool,
    ) -> Self {
        canvas_bw.fill(0xFF);
        canvas_red.fill(0x00);
        panel_bw.fill(0xFF);
        panel_red.fill(0x00);
        prev.fill(0xFF);
        prev_red.fill(0x00);
        Self {
            display,
            width,
            height,
            canvas_bw,
            canvas_red,
            panel_bw,
            panel_red,
            prev,
            prev_red,
            code_bw,
            code_red,
            scale_fill,
        }
    }

    /// Enable (`true`) or disable (`false`) upscaling the canvas to fill the
    /// panel. Takes effect on the next [`refresh`](EpdDriver::refresh).
    pub fn set_scale_fill(&mut self, on: bool) {
        self.scale_fill = on;
    }

    /// Panel plane byte length (`ceil(width/8) * height`).
    fn plane_len(&self) -> usize {
        ((self.width as usize).div_ceil(8)) * self.height as usize
    }

    /// Plot one pixel into the 152×152 canvas (shared by `draw_iter`/`fill_solid`).
    /// `EpdColor::Yellow` is dropped to White (panel has no yellow ink).
    fn plot(&mut self, x: u16, y: u16, color: EpdColor) {
        if x >= CANVAS_DIM || y >= CANVAS_DIM {
            return;
        }
        let byte_index = (x as usize / 8) + (y as usize * CANVAS_STRIDE);
        let bit_mask = 1u8 << (7 - (x % 8));
        match color {
            EpdColor::Black => {
                self.canvas_bw[byte_index] &= !bit_mask;
                self.canvas_red[byte_index] &= !bit_mask;
            }
            EpdColor::White | EpdColor::Yellow => {
                self.canvas_bw[byte_index] |= bit_mask;
                self.canvas_red[byte_index] &= !bit_mask;
            }
            EpdColor::Red => {
                self.canvas_bw[byte_index] |= bit_mask;
                self.canvas_red[byte_index] |= bit_mask;
            }
        }
    }

    /// Compose both canvas planes into the panel planes (white background +
    /// scaled-or-centred content) ready to drive.
    fn compose_panel(&mut self) {
        let n = self.plane_len();
        let stride = (self.width as usize).div_ceil(8);
        // White background (set B/W bits, clear Red); the border is whatever is
        // left untouched by the content placement below.
        self.panel_bw[..n].fill(0xFF);
        self.panel_red[..n].fill(0x00);
        let scale = self.scale_fill;
        let content = if scale {
            SCALED_DIM
        } else {
            CANVAS_DIM as usize
        };
        let off = (self.width as usize).saturating_sub(content) / 2;
        Self::compose_plane(self.canvas_bw, &mut self.panel_bw[..n], stride, scale, off);
        Self::compose_plane(
            self.canvas_red,
            &mut self.panel_red[..n],
            stride,
            scale,
            off,
        );
        // Wire convention (datasheet Table 6-4): red must present
        // `(R=1, BW=0)` → LUT2 (row 3, the red waveform). The canvas stores
        // red as `(bw=1, red=1)`, which the controller maps to LUT3 (row 4 —
        // our all-zero ignore group on the delta LUT). Clear the B/W bit
        // under every red bit so BOTH the raw full path and the delta code
        // planes derived from these planes select row 3.
        for i in 0..n {
            self.panel_bw[i] &= !self.panel_red[i];
        }
    }

    /// Copy one 152×152 canvas plane into a panel plane at pixel offset `off`.
    /// `scale` → nearest-neighbour "double every 4th pixel" (×1.25); else 1:1.
    fn compose_plane(
        canvas: &[u8; CANVAS_BYTES],
        panel: &mut [u8],
        stride: usize,
        scale: bool,
        off: usize,
    ) {
        let mut dy = off;
        for sy in 0..CANVAS_DIM as usize {
            let src_row = &canvas[sy * CANVAS_STRIDE..];
            Self::compose_row(src_row, panel, dy * stride, off, scale);
            dy += 1;
            if scale && sy % 4 == 3 {
                Self::compose_row(src_row, panel, dy * stride, off, scale);
                dy += 1;
            }
        }
    }

    /// Write one 152-px source row into `panel` row `row_base`, starting at
    /// destination column `off`. When `scale`, every 4th source column is
    /// doubled (×1.25). 1bpp, MSB-first.
    fn compose_row(src_row: &[u8], panel: &mut [u8], row_base: usize, off: usize, scale: bool) {
        let mut dx = off;
        for sx in 0..CANVAS_DIM as usize {
            let bit = (src_row[sx / 8] >> (7 - (sx & 7))) & 1;
            Self::write_bit(panel, row_base, dx, bit);
            dx += 1;
            if scale && sx % 4 == 3 {
                Self::write_bit(panel, row_base, dx, bit);
                dx += 1;
            }
        }
    }

    /// Set (`bit != 0`) or clear one bit at `(row_base + col/8)`, MSB-first.
    #[inline]
    fn write_bit(panel: &mut [u8], row_base: usize, col: usize, bit: u8) {
        let idx = row_base + col / 8;
        let mask = 0x80u8 >> (col & 7);
        if bit != 0 {
            panel[idx] |= mask;
        } else {
            panel[idx] &= !mask;
        }
    }
}

impl<const N: usize> OriginDimensions for Driver<'_, N> {
    /// The UI draws on the fixed 152×152 canvas; the panel size stays internal.
    fn size(&self) -> Size {
        Size::new(CANVAS_DIM as u32, CANVAS_DIM as u32)
    }
}

impl<const N: usize> DrawTarget for Driver<'_, N> {
    type Color = EpdColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<EpdColor>>,
    {
        for Pixel(p, c) in pixels {
            if p.x < 0 || p.y < 0 {
                continue;
            }
            self.plot(p.x as u16, p.y as u16, c);
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: EpdColor) -> Result<(), Self::Error> {
        let area = area.intersection(&self.bounding_box());
        for p in area.points() {
            if p.x >= 0 && p.y >= 0 {
                self.plot(p.x as u16, p.y as u16, color);
            }
        }
        Ok(())
    }
}

impl<const N: usize> PlaneAccess for Driver<'_, N> {
    /// Returns the 152×152 canvas planes — the sprite/PCX blitter draws here at
    /// canvas resolution; scaling to the panel happens at `refresh`.
    fn planes_mut(&mut self) -> (&mut [u8], &mut [u8]) {
        (&mut self.canvas_bw[..], &mut self.canvas_red[..])
    }
}

impl<'a, const N: usize> EpdDriver for Driver<'a, N> {
    type Error = <Iface<'a> as DisplayInterface>::Error;

    async fn init(&mut self) -> Result<(), <Self as EpdDriver>::Error> {
        self.display.init().await
    }

    async fn refresh(&mut self, mode: RefreshMode) -> Result<(), <Self as EpdDriver>::Error> {
        let n = self.plane_len();
        // Compose the 152 canvas into the 200 panel planes (scaled or centred).
        self.compose_panel();
        // No Fast→Full promotion on red change any more: the fast delta seats
        // red itself via the DELTA_LUT G3 white→neutral→red sort. A B/W-only
        // frame skips that group entirely (DELTA_LUT_NO_RED, selected by
        // `skip_red` below), so red changes stay on the fast path — slower than
        // a pure B/W delta, but no full-screen flash.
        match mode {
            RefreshMode::Full => {
                // Panel planes are already wire-convention (red = `(R=1,
                // BW=0)` → LUT2/row 3; see `compose_panel`) — send raw.
                self.display
                    .full(&self.panel_bw[..n], &self.panel_red[..n])
                    .await?;
            }
            RefreshMode::Fast => {
                // Encode per-pixel delta codes: changed pixels get their target
                // colour's drive group, unchanged pixels the all-zero ignore
                // group (L3) — no voltage, no flash, no red re-drive. The
                // custom DELTA_LUT runs every group at the 200 Hz frame rate
                // (FR = 7, datasheet §6.6).
                let changed = build_code_planes(
                    &self.prev[..n],
                    &self.prev_red[..n],
                    &self.panel_bw[..n],
                    &self.panel_red[..n],
                    &mut self.code_bw[..n],
                    &mut self.code_red[..n],
                );
                if changed {
                    // The delta path never windows — the box is log-only.
                    let bbox = BoundingBox {
                        x0: 0,
                        y0: 0,
                        x1: self.width - 1,
                        y1: self.height - 1,
                    };
                    // Skip the red drive phase entirely when no red changed.
                    let skip_red = self.prev_red[..n] == self.panel_red[..n];
                    self.display
                        .update_delta(&self.code_bw[..n], &self.code_red[..n], bbox, skip_red)
                        .await?;
                }
            }
        }
        self.prev[..n].copy_from_slice(&self.panel_bw[..n]);
        self.prev_red[..n].copy_from_slice(&self.panel_red[..n]);
        Ok(())
    }

    /// Intentionally a no-op on the .
    ///
    /// Deep-sleep mode 1 only exits via a hardware reset, which clears the
    /// controller RAM. The fast [`RefreshMode::Fast`] (`delta`) path relies on
    /// that RAM persisting (the controller diffs against its latched previous
    /// frame). Sleeping between refreshes would force a reset (RAM clear) on the
    /// next refresh and blank the unchanged background. The full/partial
    /// waveforms already leave the panel quiescent (`DisableAnalog |
    /// DisableOsc`), so skipping the explicit deep sleep costs little — and
    /// matches the validated bring-up, which never slept the panel.
    async fn deep_sleep(&mut self) -> Result<(), <Self as EpdDriver>::Error> {
        Ok(())
    }
}

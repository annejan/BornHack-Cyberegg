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
//! ## Preview, settle and charge correction
//!
//! A [`RefreshMode::Fast`] that drives only a few B/W pixels (menu cursor,
//! small sprite step) runs the short preview pulse instead of the 1.16 s
//! delta; [`PreviewScheduler`] makes the call (size, red, run limit) and
//! remembers the previewed pixels. They are settled by the next normal delta
//! or, once the screen is idle for `SETTLE_MS`, by [`EpdDriver::settle`]
//! (also run before [`EpdDriver::deep_sleep`]). After every update the signed
//! [`ChargeLedger`] books the waveform's V·t per pixel and pulses pixels that
//! carry clearly more than one image's worth with the opposite rail. The same
//! post-drive pass gives white pixels next to freshly driven black a short
//! VSL halo kick (`HALO_FRAMES`) against black ink bleeding sideways. All of
//! it is tunable from `EPD.CFG` (see `fw::epd_cfg`).
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
use ssd1680::display::Waveform;
use ssd1680::{
    BoundingBox, ChargeLedger, CorrectionOpts, DeltaState, Display, EpdTuning, Interface,
    LedgerConfig, PreviewScheduler, UpdateKind,
};
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
    /// Confirmed-on-glass shadow planes plus the damage bitmap: every pixel a
    /// waveform has touched but not finished driving. Replaces the old raw
    /// `prev`/`prev_red` diff, which could not survive an interrupted refresh —
    /// a pixel driven toward B, cancelled mid-waveform, then re-targeted back to
    /// its shadow colour diffed as "unchanged" and was left stranded in a
    /// partial state. [`DeltaState`] ORs such pixels into `dirty` and clears the
    /// ledger only on a completed drive, so they are re-driven to the latest
    /// target instead.
    delta: DeltaState,
    /// Signed per-pixel V·t ledger (4 bits/pixel); schedules short
    /// opposite-rail correction pulses for pixels carrying excess charge. See
    /// `ssd1680::ledger`.
    ledger: ChargeLedger,
    /// Preview-vs-delta decision (run counter only: previewed pixels stay in
    /// `delta`'s dirty bitmap until settled).
    preview: PreviewScheduler,
    /// Runtime knobs (from `EPD.CFG`), already clamped by the driver crate.
    tuning: EpdTuning,
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
    /// * `panel_bw`/`panel_red`  - Panel planes, each `N` bytes.
    /// * `shadow_bw`/`shadow_red`- Confirmed-on-glass planes (the old
    ///   `prev`/`prev_red`), handed to [`DeltaState`]. `'static` because
    ///   `DeltaState` stores them for the driver's whole life.
    /// * `dirty`                 - Damage bitmap, 1 bit per panel pixel, so
    ///   also `N` bytes. Zeroed here: nothing has been driven yet.
    /// * `code_bw`/`code_red`    - Delta code-plane scratch, each `N` bytes.
    ///   Reused for the correction planes once the update has run.
    /// * `charge`                - Charge ledger, 4 bits per panel pixel
    ///   ([`ChargeLedger::bytes_for`]); zeroed here.
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
        shadow_bw: &'static mut [u8; N],
        shadow_red: &'static mut [u8; N],
        dirty: &'static mut [u8; N],
        code_bw: &'a mut [u8; N],
        code_red: &'a mut [u8; N],
        charge: &'static mut [u8],
        scale_fill: bool,
    ) -> Self {
        canvas_bw.fill(0xFF);
        canvas_red.fill(0x00);
        panel_bw.fill(0xFF);
        panel_red.fill(0x00);
        shadow_bw.fill(0xFF);
        shadow_red.fill(0x00);
        dirty.fill(0x00);
        code_bw.fill(0xFF);
        code_red.fill(0xFF);
        let delta = DeltaState::new(shadow_bw, shadow_red, dirty, width, height);
        let ledger = ChargeLedger::new(charge, width, height, LedgerConfig::default());
        let preview = PreviewScheduler::new();
        // Keep the analog rails up between back-to-back updates (a delta and
        // its correction pulse, or a burst of button presses). The display loop calls
        // `deep_sleep` once updates stop, which drops them.
        let mut display = display;
        display.set_analog_hold(true);
        Self {
            display,
            width,
            height,
            canvas_bw,
            canvas_red,
            panel_bw,
            panel_red,
            delta,
            ledger,
            preview,
            tuning: EpdTuning::default(),
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

    /// Install runtime tuning (normally from `EPD.CFG`). The driver crate
    /// clamps every field; takes effect on the next update.
    pub fn set_tuning(&mut self, t: EpdTuning) {
        self.display.set_tuning(t);
        self.tuning = self.display.tuning();
        self.ledger.set_config(LedgerConfig {
            pos: self.tuning.ledger_pos,
            neg: self.tuning.ledger_neg,
            decay_shift: self.tuning.ledger_decay,
        });
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

impl<'a, const N: usize> Driver<'a, N> {
    /// [`EpdDriver::refresh`] with preview allowed or not.
    async fn refresh_with(
        &mut self,
        mode: RefreshMode,
        allow_preview: bool,
    ) -> Result<(), <Iface<'a> as DisplayInterface>::Error> {
        let n = self.plane_len();
        // Compose the 152 canvas into the panel planes (scaled or centred).
        self.compose_panel();
        match mode {
            RefreshMode::Full => {
                // Panel planes are already wire-convention (red = `(R=1,
                // BW=0)` → LUT2/row 3; see `compose_panel`) — send raw.
                self.display
                    .full(&self.panel_bw[..n], &self.panel_red[..n])
                    .await?;
                // Past the await, so the waveform ran to completion. A full
                // drives every pixel unconditionally: the shadow advances to
                // the live frame, the damage set (incl. previewed pixels)
                // clears, and the charge ledger restarts from zero (the full
                // is the backstop).
                self.delta
                    .commit_full(&self.panel_bw[..n], &self.panel_red[..n]);
                self.preview.on_delta();
                self.ledger.reset();
            }
            RefreshMode::Fast => {
                // Decide on what changed vs the shadow (dirty-only pixels
                // never preview: they get the full delta or the settle).
                let stats = self
                    .delta
                    .changed_stats(&self.panel_bw[..n], &self.panel_red[..n]);
                let kind = if allow_preview {
                    self.preview.choose(&self.tuning, stats)
                } else {
                    UpdateKind::Delta
                };
                // A correction only ever follows a full-length drive, and
                // only for the pixels that drive moved (see
                // `ChargeLedger::plan_correction`). Never after a preview: its
                // pixels are short of saturation and get their settle first.
                match kind {
                    UpdateKind::Preview => {
                        self.run_preview().await?;
                    }
                    UpdateKind::Delta => {
                        if self.run_delta().await? {
                            self.run_correction().await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Short B/W preview of the changed pixels. `build_preview` ORs them
    /// into the dirty bitmap *before* the await, so a cancelled preview is
    /// re-driven like any interrupted drive; a completed one advances the
    /// shadow but keeps them dirty for the settle. `Ok(false)` = nothing to
    /// drive.
    async fn run_preview(&mut self) -> Result<bool, <Iface<'a> as DisplayInterface>::Error> {
        let n = self.plane_len();
        if self
            .delta
            .build_preview(
                &self.panel_bw[..n],
                &self.panel_red[..n],
                &mut self.code_bw[..n],
                &mut self.code_red[..n],
            )
            .is_none()
        {
            return Ok(false);
        }
        self.preview.on_preview();
        defmt::info!(
            "1680 preview: {} px, run {}",
            self.delta.last_driven_px(),
            self.preview.run()
        );
        self.display
            .update_preview(&self.code_bw[..n], &self.code_red[..n])
            .await?;
        self.delta
            .commit_preview(&self.panel_bw[..n], &self.panel_red[..n]);
        let w = self.display.weights(Waveform::Preview);
        self.ledger
            .after_update(&self.code_bw[..n], &self.code_red[..n], w);
        Ok(true)
    }

    /// Normal delta over (changed vs shadow) ∪ dirty, so it also settles
    /// every previewed pixel and re-drives any left by a cancelled drive.
    /// `Ok(false)` = nothing to drive.
    async fn run_delta(&mut self) -> Result<bool, <Iface<'a> as DisplayInterface>::Error> {
        let n = self.plane_len();
        if self
            .delta
            .build(
                &self.panel_bw[..n],
                &self.panel_red[..n],
                &mut self.code_bw[..n],
                &mut self.code_red[..n],
            )
            .is_none()
        {
            return Ok(false);
        }
        self.drive_delta().await?;
        // Only reached when the waveform completed. If the refresh future is
        // dropped mid-drive the commit never runs, so the shadow stays stale
        // and every pixel this build touched stays in the damage bitmap for
        // the next one.
        self.delta.commit(&self.panel_bw[..n], &self.panel_red[..n]);
        Ok(true)
    }

    /// Run the normal delta waveform over the code planes the last build
    /// wrote, end the preview run and book the charge. The caller commits.
    async fn drive_delta(&mut self) -> Result<(), <Iface<'a> as DisplayInterface>::Error> {
        let n = self.plane_len();
        // The delta path never windows — the box is log-only.
        let bbox = BoundingBox {
            x0: 0,
            y0: 0,
            x1: self.width - 1,
            y1: self.height - 1,
        };
        // Skip the red drive phase entirely when no pixel this build actually
        // drives is red. Asking `DeltaState` rather than diffing the red plane
        // matters once damage is in play: a red pixel left half-driven by a
        // cancelled waveform is re-driven even though the plane compares
        // equal, and skipping its phase would strand it.
        let skip_red = !self.delta.last_had_red();
        self.display
            .update_delta(&self.code_bw[..n], &self.code_red[..n], bbox, skip_red)
            .await?;
        self.preview.on_delta();
        let w = self.display.weights(if skip_red {
            Waveform::DeltaBw
        } else {
            Waveform::DeltaRed
        });
        self.ledger
            .after_update(&self.code_bw[..n], &self.code_red[..n], w);
        Ok(())
    }

    /// Post-drive pass on the still-powered rails: opposite-rail correction
    /// pulses for pixels the ledger flags (`LEDGER`) plus the halo kick on
    /// white pixels next to fresh black (`HALO_FRAMES`), one LUT. Must run
    /// straight after a completed delta/settle: the code planes still hold
    /// that drive's codes; only the pixels it drove are correction candidates
    /// (at most one pulse per drive, so a held pixel is never kicked over and
    /// over) and only the pixels it drove black seed the halo. Rewrites the
    /// code planes in place. If cancelled, nothing is lost: the image is
    /// committed, the ledger is only debited after the pulse ran, and the
    /// next refresh resets the controller.
    async fn run_correction(&mut self) -> Result<(), <Iface<'a> as DisplayInterface>::Error> {
        let opts = CorrectionOpts::after(UpdateKind::Delta, &self.tuning);
        if opts.is_off() {
            return Ok(());
        }
        let n = self.plane_len();
        let plan = self.ledger.plan_correction(
            &self.panel_bw[..n],
            &self.panel_red[..n],
            &mut self.code_bw[..n],
            &mut self.code_red[..n],
            opts,
        );
        if plan.is_empty() {
            return Ok(());
        }
        defmt::info!(
            "1680 correct: vsl={} vsh1={} halo={} max|c|={}",
            plan.vsl,
            plan.vsh1,
            plan.halo,
            self.ledger.max_abs()
        );
        self.display
            .correct(&self.code_bw[..n], &self.code_red[..n])
            .await?;
        let w = self.display.weights(Waveform::Correction);
        self.ledger
            .after_correction(&self.code_bw[..n], &self.code_red[..n], w);
        Ok(())
    }

    /// Settle every dirty pixel (previewed, or left by a cancelled drive)
    /// with the normal delta, toward the panel planes of the last refresh.
    /// Changed-but-clean pixels are not touched (they wait for the next
    /// refresh). No-op when nothing is dirty.
    async fn settle_dirty(&mut self) -> Result<(), <Iface<'a> as DisplayInterface>::Error> {
        if !self.delta.has_dirty() {
            return Ok(());
        }
        let n = self.plane_len();
        if self
            .delta
            .build_settle(
                &self.panel_bw[..n],
                &self.panel_red[..n],
                &mut self.code_bw[..n],
                &mut self.code_red[..n],
            )
            .is_none()
        {
            return Ok(());
        }
        defmt::info!("1680 settle: {} px", self.delta.last_driven_px());
        self.drive_delta().await?;
        self.delta
            .commit_settle(&self.panel_bw[..n], &self.panel_red[..n]);
        self.run_correction().await
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
        self.refresh_with(mode, true).await
    }

    /// Settles any previewed pixels, then drops the analog rails the held
    /// delta/preview/correction path leaves on; does not
    /// enter the controller's deep-sleep mode.
    ///
    /// Deep-sleep mode 1 only exits via a hardware reset, which clears the
    /// controller RAM. The fast [`RefreshMode::Fast`] (`delta`) path relies on
    /// that RAM persisting (the controller diffs against its latched previous
    /// frame). Sleeping between refreshes would force a reset (RAM clear) on the
    /// next refresh and blank the unchanged background. Powering the rails down
    /// (`0x22 = 0x03`) is the lowest state that keeps RAM, and is a no-op when
    /// they are already off.
    async fn deep_sleep(&mut self) -> Result<(), <Self as EpdDriver>::Error> {
        // Never park with previewed pixels short of saturation.
        self.settle_dirty().await?;
        self.display.power_off().await
    }

    fn settle_due_in_ms(&self) -> Option<u32> {
        PreviewScheduler::settle_due_in_ms(&self.tuning, self.delta.has_dirty())
    }

    async fn settle(&mut self) -> Result<(), <Self as EpdDriver>::Error> {
        self.settle_dirty().await
    }
}

/// First-boot factory test on the SSD1680: full clean / ship image through
/// [`RefreshMode::Full`], per-row updates through [`RefreshMode::Fast`] (the
/// same unified-driver calls the UI uses).
impl<const N: usize> crate::fw::factory_test::FactoryDisplay for Driver<'_, N> {
    async fn refresh_full(&mut self) {
        let _ = EpdDriver::refresh(self, RefreshMode::Full).await;
    }

    async fn refresh_fast(&mut self) {
        // No preview: the factory test can halt on its last frame, and a
        // previewed pixel must never be the final state.
        let _ = self.refresh_with(RefreshMode::Fast, false).await;
    }
}

//! "My QR" screen — renders a QR code carrying a `meshcore://contact/add?...`
//! URL so another device's camera can add this badge as a mesh contact
//! without manual key entry.
//!
//! # URL format
//!
//! ```text
//! meshcore://contact/add?name=<NODE_NAME>&public_key=<64-hex>&type=1
//! ```
//!
//! Both fields are read live every redraw:
//! * `<NODE_NAME>`   — current [`crate::NODE_NAME`] value (default ~4-char
//!                     hex device-id label, user-renamable via menu / NUS).
//! * `<64-hex>`      — lowercase hex of the badge's 32-byte Ed25519 public
//!                     key, cached in [`crate::MY_PUB_KEY`] at boot by
//!                     `bin/embassy.rs` once `load_or_create_identity()`
//!                     has populated it.
//!
//! # Rendering
//!
//! Encoded as QR version 6 (41×41 modules) with EC level Low — fits the
//! 4-char-name URL (~115 bytes) comfortably and leaves room if the user
//! sets a longer name.  Rendered at 3 px / module on the 152×152 e-paper
//! with a 6-px quiet zone, occupying 135×135 px.  Header band at top
//! shows "Scan to add me" + battery icon (via [`crate::draw_frame`]).
//!
//! No heap: `qrcodegen-no-heap` works with caller-supplied buffers; we
//! use stack-resident scratch + output arrays sized for V7 (the next
//! step up if a long name forces a bump).

use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use qrcodegen_no_heap::{QrCode, QrCodeEcc, Version};

use crate::TriColor;

/// Lowest supported version — must be high enough for the URL we generate.
const QR_MIN_VERSION: Version = Version::new(1);
/// Cap at V7 (45×45) so a longer NODE_NAME still fits on the panel at
/// 3 px/module with a 4-px quiet zone (135 + 24 ≤ 152).
const QR_MAX_VERSION: Version = Version::new(7);

/// Buffer size: `qrcodegen-no-heap` documents this as the max byte count
/// for the chosen version, used for both the temp and the output buffer.
/// V7 needs 3917 / 8 ≈ 490 bytes — use 600 for headroom.
const QR_BUF_LEN: usize = 600;

/// Pixels per QR module on the e-paper.  Chosen so V7's 45 modules fit
/// within the 152 px panel width with a 4-module quiet zone (135 + 24 =
/// 159, but we offset by half the slack so 8 modules of quiet zone span
/// the actual border).
const PX_PER_MODULE: i32 = 3;

// ---------------------------------------------------------------------------
// URL construction
// ---------------------------------------------------------------------------

/// Build the meshcore contact-add URL into `buf`, returning the slice
/// of bytes actually written.
///
/// Format: `meshcore://contact/add?name=<name>&public_key=<hex64>&type=1`.
/// Returns `None` if the caller's buffer can't hold the result (caller
/// passes a 256-byte buffer; URL maxes out at ~150 bytes).
pub fn build_url<'a>(buf: &'a mut [u8], name: &str, pub_key: &[u8; 32]) -> Option<&'a [u8]> {
    use core::fmt::Write;

    let mut s: heapless::String<200> = heapless::String::new();
    s.push_str("meshcore://contact/add?name=").ok()?;
    s.push_str(name).ok()?;
    s.push_str("&public_key=").ok()?;
    for byte in pub_key {
        write!(s, "{:02x}", byte).ok()?;
    }
    s.push_str("&type=1").ok()?;

    let bytes = s.as_bytes();
    if bytes.len() > buf.len() {
        return None;
    }
    buf[..bytes.len()].copy_from_slice(bytes);
    Some(&buf[..bytes.len()])
}

// ---------------------------------------------------------------------------
// Reading badge state
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-base")]
fn read_name(buf: &mut heapless::String<31>) {
    crate::NODE_NAME.lock(|cell| {
        let _ = buf.push_str(cell.borrow().as_str());
    });
}

#[cfg(feature = "simulator")]
fn read_name(buf: &mut heapless::String<31>) {
    let guard = crate::NODE_NAME.lock().unwrap();
    let _ = buf.push_str(guard.borrow().as_str());
}

#[cfg(feature = "embassy-base")]
fn read_pub_key() -> [u8; 32] {
    crate::MY_PUB_KEY.lock(|cell| *cell.borrow())
}

#[cfg(feature = "simulator")]
fn read_pub_key() -> [u8; 32] {
    *crate::MY_PUB_KEY.lock().unwrap().borrow()
}

// ---------------------------------------------------------------------------
// Draw
// ---------------------------------------------------------------------------

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    #[cfg(feature = "embassy-base")]
    let bat = crate::fw::battery::read_pct();
    #[cfg(not(feature = "embassy-base"))]
    let bat: u8 = 0;

    crate::draw_frame(display, Some(("Scan to add me", &bat)), None)?;

    let mut name: heapless::String<31> = heapless::String::new();
    read_name(&mut name);
    let pub_key = read_pub_key();

    // If the pub key hasn't been populated yet (boot race, or built
    // without `mesh`), show a placeholder instead of a useless QR.
    if pub_key == [0u8; 32] {
        use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
        let centered = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        Text::with_text_style(
            "(key not ready)",
            Point::new(76, 84),
            crate::ui::TEXT_BOLD_BLACK,
            centered,
        )
        .draw(display)?;
        return Ok(());
    }

    let mut url_buf = [0u8; 256];
    let url_bytes = match build_url(&mut url_buf, name.as_str(), &pub_key) {
        Some(b) => b,
        None => return Ok(()),
    };
    let url = match core::str::from_utf8(url_bytes) {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let mut tmp_buf = [0u8; QR_BUF_LEN];
    let mut out_buf = [0u8; QR_BUF_LEN];
    let qr = match QrCode::encode_text(
        url,
        &mut tmp_buf,
        &mut out_buf,
        QrCodeEcc::Low,
        QR_MIN_VERSION,
        QR_MAX_VERSION,
        None,
        true,
    ) {
        Ok(qr) => qr,
        Err(_) => return Ok(()),
    };

    let size = qr.size();
    let total_px = size * PX_PER_MODULE;
    // Centre horizontally; push down past the header band drawn by
    // draw_frame (header occupies the top ~18 px).
    let x_offset = (152 - total_px) / 2;
    let y_offset = 20 + (152 - 20 - total_px) / 2;

    // Each "true" module is filled black; the e-ink's white background
    // already provides the inverse.  Quiet zone is implicit — we don't
    // touch pixels outside the module grid.
    let black = PrimitiveStyle::with_fill(TriColor::Black);
    let _ = black; // silence unused warning if BinaryColor path below changes

    for my in 0..size {
        for mx in 0..size {
            if qr.get_module(mx, my) {
                let x = x_offset + mx * PX_PER_MODULE;
                let y = y_offset + my * PX_PER_MODULE;
                Rectangle::new(
                    Point::new(x, y),
                    Size::new(PX_PER_MODULE as u32, PX_PER_MODULE as u32),
                )
                .into_styled(PrimitiveStyle::with_fill(TriColor::Black))
                .draw(display)?;
            }
        }
    }

    // Suppress unused warning for BinaryColor import in simulator builds
    // that strip the TriColor path.
    let _ = BinaryColor::On;

    Ok(())
}

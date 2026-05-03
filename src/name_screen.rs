//! Classic "Hello my name is" conference-badge screen — sits in the
//! icon grid, takes over the full panel and renders the current
//! `NODE_NAME` as large as it'll fit.  Pure renderer; arrows fall
//! through to screen-nav the same way the Watch face does.
//!
//! Layout (152×152) — mimics the iconic peel-off name tag:
//!
//! ```text
//!  ┌──────────────────────┐
//!  │  HELLO               │  ← red band (top ~40%), centered white
//!  │   my name is         │     PROFONT_24 + PROFONT_14
//!  ├──────────────────────┤
//!  │                      │
//!  │      <node name>     │  ← BIG centered, biggest font that fits
//!  │                      │
//!  │   Cyber Ægg <id>     │  ← small footer (iso-8859-1 for the Æ)
//!  └──────────────────────┘
//! ```
//!
//! The name picks the biggest single-line font that fits.  Short names
//! (≤5 chars) get the u8g2 `fub42_tf` (~42 px tall) for maximum
//! conference-table-readable presence; longer ones step down through
//! profont 24/18 → stock `FONT_10X20` → `FONT_8X13_BOLD` →
//! `FONT_7X13_BOLD`.  Names longer than ~21 chars truncate
//! (renderer doesn't word-wrap; conference name tags are short by
//! convention).
//!
//! When `NODE_NAME` is empty the renderer shows `(no name set)`
//! pointing at the Settings → Set Name flow.

use embedded_graphics::mono_font::ascii::{FONT_7X13_BOLD, FONT_8X13_BOLD, FONT_10X20};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle, iso_8859_1};
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
use profont::{PROFONT_14_POINT, PROFONT_18_POINT, PROFONT_24_POINT};
use u8g2_fonts::FontRenderer;
use u8g2_fonts::fonts::u8g2_font_fub42_tf;
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use crate::{BLACK, RED, TriColor, WHITE};

/// Top-of-panel red band — y < this is filled red, y >= this is white.
const RED_BAND_BOTTOM: i32 = 56;

/// Pick the largest single-line MonoFont whose total width fits within
/// `max_w` for a name of `name_len` chars.  Walks biggest → smallest
/// across profont + stock embedded-graphics ascii.  The huge u8g2
/// `fub42_tf` is handled separately by the caller (different API).
fn pick_name_font(name_len: usize, max_w: u32) -> &'static MonoFont<'static> {
    // (font, per-char width).  Order matters — first match wins.
    let candidates: [(&MonoFont, u32); 5] = [
        (&PROFONT_24_POINT, 16),
        (&PROFONT_18_POINT, 12),
        (&FONT_10X20, 10),
        (&FONT_8X13_BOLD, 8),
        (&FONT_7X13_BOLD, 7),
    ];
    for (font, w) in candidates {
        if (name_len as u32) * w <= max_w {
            return font;
        }
    }
    // Smallest fallback — name will visibly clip beyond ~21 chars.
    &FONT_7X13_BOLD
}

pub fn draw<D>(display: &mut D, bat_prc: &u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let _ = bat_prc; // Battery isn't surfaced on this screen — name stands alone.

    // Clear the panel — bottom half stays white, top is overdrawn red below.
    Rectangle::new(Point::new(0, 0), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // ── Red band: filled rectangle covering the top portion ───────────────
    Rectangle::new(Point::new(0, 0), Size::new(152, RED_BAND_BOTTOM as u32))
        .into_styled(PrimitiveStyle::with_fill(RED))
        .draw(display)?;

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    // "HELLO" — big and centered
    Text::with_text_style(
        "HELLO",
        Point::new(76, 18),
        MonoTextStyle::new(&PROFONT_24_POINT, WHITE),
        centered,
    )
    .draw(display)?;

    // "my name is" — smaller subtitle
    Text::with_text_style(
        "my name is",
        Point::new(76, 42),
        MonoTextStyle::new(&PROFONT_14_POINT, WHITE),
        centered,
    )
    .draw(display)?;

    // ── Big name in the white area below the red band ────────────────────
    // Snapshot the current node name into a local buffer — keep the
    // mutex hold short and let the renderer work without holding any
    // locks.  Strip non-printable bytes so the chosen font (ASCII-only
    // for the bigger sizes) doesn't render replacement glyphs.
    let mut name_buf: heapless::String<31> = heapless::String::new();
    snapshot_name(&mut name_buf);

    if name_buf.is_empty() {
        // Use the smaller iso-8859 font so the parens render in either build.
        Text::with_text_style(
            "(no name set)",
            Point::new(76, 100),
            MonoTextStyle::new(&iso_8859_1::FONT_8X13_BOLD, BLACK),
            centered,
        )
        .draw(display)?;
    } else {
        let name_str = name_buf.as_str();
        let len = name_str.chars().count();

        // Huge u8g2 font for very short names — `fub42_tf` glyphs are
        // ~26 px wide, so 5 chars = ~130 px and clears the 152 px panel
        // with margin.  Falls back to the MonoFont chain for longer
        // names so we still single-line up to ~21 chars.
        if len <= 5 {
            let renderer = FontRenderer::new::<u8g2_font_fub42_tf>();
            renderer
                .render_aligned(
                    name_str,
                    Point::new(76, 100),
                    VerticalPosition::Center,
                    HorizontalAlignment::Center,
                    FontColor::Transparent(BLACK),
                    display,
                )
                .map_err(|e| match e {
                    u8g2_fonts::Error::DisplayError(d) => d,
                    // Other variants (e.g. `GlyphNotFound`) — name was
                    // pre-filtered to ASCII printable upstream so this
                    // shouldn't be reachable.  Treat as unreachable.
                    _ => panic!("u8g2 glyph render failure"),
                })?;
        } else {
            let font = pick_name_font(len, 148);
            Text::with_text_style(
                name_str,
                Point::new(76, 100),
                MonoTextStyle::new(font, BLACK),
                centered,
            )
            .draw(display)?;
        }
    }

    // ── Bottom footer — device ID ─────────────────────────────────────────
    // Uses the iso-8859-1 font so the `Æ` (U+00C6) renders properly —
    // the ascii font would emit the fallback glyph here.
    let mut id_buf: heapless::String<16> = heapless::String::new();
    write_device_id(&mut id_buf);
    Text::with_text_style(
        id_buf.as_str(),
        Point::new(76, 142),
        MonoTextStyle::new(&iso_8859_1::FONT_6X10, BLACK),
        centered,
    )
    .draw(display)?;

    Ok(())
}

/// Copy the current `NODE_NAME` into `out`, dropping any non-printable
/// bytes so the ASCII-only big fonts don't render `?` replacements.
/// The full UTF-8 name is preserved in the source mutex — this is just
/// for the on-screen big-text rendering.
fn snapshot_name(out: &mut heapless::String<31>) {
    out.clear();
    #[cfg(feature = "embassy-base")]
    {
        crate::NODE_NAME.lock(|cell| {
            let name = cell.borrow();
            for b in name.as_bytes() {
                if (0x20..=0x7e).contains(b) && out.push(*b as char).is_err() {
                    break;
                }
            }
        });
    }
    #[cfg(feature = "simulator")]
    {
        let guard = crate::NODE_NAME.lock().unwrap();
        let name = guard.borrow();
        for b in name.as_bytes() {
            if (0x20..=0x7e).contains(b) && out.push(*b as char).is_err() {
                break;
            }
        }
    }
}

/// Build the "Cyber Ægg <id>" footer string.  Mirrors the title-bar
/// builder in `lib::draw_screen_main`.
fn write_device_id(out: &mut heapless::String<16>) {
    let _ = out.push_str("Cyber ");
    let _ = out.push('\u{00C6}');
    let _ = out.push_str("gg ");
    #[cfg(feature = "embassy-base")]
    {
        let id = crate::fw::device_id::get_bytes();
        let _ = out.push_str(core::str::from_utf8(&id).unwrap_or("????"));
    }
    #[cfg(feature = "simulator")]
    {
        let _ = out.push_str("A3F7");
    }
}

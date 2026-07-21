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
/// across profont + embedded-graphics `iso_8859_1`.  All candidates carry
/// full Latin-1 glyphs so accented names (issue #111) render at every size.
/// The huge u8g2 `fub42_tf` is handled separately by the caller (different
/// API) and is also Latin-1 capable.
fn pick_name_font(name_len: usize, max_w: u32) -> &'static MonoFont<'static> {
    // (font, per-char width).  Order matters — first match wins.
    let candidates: [(&MonoFont, u32); 5] = [
        (&PROFONT_24_POINT, 16),
        (&PROFONT_18_POINT, 12),
        (&iso_8859_1::FONT_10X20, 10),
        (&iso_8859_1::FONT_8X13_BOLD, 8),
        (&iso_8859_1::FONT_7X13_BOLD, 7),
    ];
    for (font, w) in candidates {
        if (name_len as u32) * w <= max_w {
            return font;
        }
    }
    // Smallest fallback — name will visibly clip beyond ~21 chars.
    &iso_8859_1::FONT_7X13_BOLD
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
    // locks.  Keeps printable ASCII + Latin-1 (all fonts below are Latin-1
    // capable) and drops anything else.
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

        // Render the name with the MonoFont chain (profont + iso_8859_1, all
        // full Latin-1).  Also the fallback if the huge u8g2 font is missing a
        // glyph.  Non-renderable chars were already mapped to `?` in the
        // snapshot, so both font paths render them at the name's own size.
        let draw_mono = |display: &mut D| -> Result<(), D::Error> {
            let font = pick_name_font(len, 148);
            Text::with_text_style(
                name_str,
                Point::new(76, 100),
                MonoTextStyle::new(font, BLACK),
                centered,
            )
            .draw(display)
            .map(|_| ())
        };

        // Huge u8g2 font for very short names — `fub42_tf` glyphs are
        // ~26 px wide, so 5 chars = ~130 px and clears the 152 px panel
        // with margin.  Falls back to the MonoFont chain for longer
        // names so we still single-line up to ~21 chars.
        if len <= 5 {
            let renderer = FontRenderer::new::<u8g2_font_fub42_tf>();
            match renderer.render_aligned(
                name_str,
                Point::new(76, 100),
                VerticalPosition::Center,
                HorizontalAlignment::Center,
                FontColor::Transparent(BLACK),
                display,
            ) {
                Ok(_) => {}
                Err(u8g2_fonts::Error::DisplayError(d)) => return Err(d),
                // Missing glyph (`fub42_tf` lacks this Latin-1 code point, or a
                // non-Latin-1 char slipped through the filter): fall back to the
                // MonoFont chain — which covers full Latin-1 — instead of
                // panicking.
                Err(_) => draw_mono(display)?,
            }
        } else {
            draw_mono(display)?;
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

/// Copy the current `NODE_NAME` into `out`, keeping printable ASCII and
/// Latin-1 characters (so accented names and `Æ` render — issue #111) and
/// dropping everything else (control chars, and anything beyond U+00FF like
/// CJK/emoji that the fonts have no glyph for — dropping bounds the rendered
/// width and avoids missing-glyph fallbacks).  Iterates over `char`s of the
/// already-UTF-8-validated `NODE_NAME`, so multi-byte sequences are preserved
/// intact.  The full name is still kept in the source mutex — this is just the
/// on-screen snapshot.
fn snapshot_name(out: &mut heapless::String<31>) {
    out.clear();
    #[cfg(feature = "embassy-base")]
    {
        crate::NODE_NAME.lock(|cell| {
            let name = cell.borrow();
            push_renderable(out, name.as_str());
        });
    }
    #[cfg(feature = "simulator")]
    {
        let guard = crate::NODE_NAME.lock().unwrap();
        let name = guard.borrow();
        push_renderable(out, name.as_str());
    }
}

/// Push the drawable characters of `name` into `out`, stopping if the buffer
/// fills.  Printable ASCII (`0x20..=0x7e`) and Latin-1 (`U+00A0..=U+00FF`) pass
/// through unchanged; any higher codepoint (CJK/emoji — no font glyph) is
/// substituted with a plain `?` so it renders at the name's own (large) font
/// size rather than a small `�` glyph amid big text, and so a name made only of
/// such characters still shows (as `?`) rather than collapsing to "(no name
/// set)".  C0/C1 control characters are dropped.
fn push_renderable(out: &mut heapless::String<31>, name: &str) {
    for c in name.chars() {
        let mapped = if matches!(c, ' '..='~') || matches!(c, '\u{A0}'..='\u{FF}') {
            c
        } else if c.is_control() {
            continue;
        } else {
            '?'
        };
        if out.push(mapped).is_err() {
            break;
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

#[cfg(test)]
mod tests {
    use super::push_renderable;

    fn filter(s: &str) -> heapless::String<31> {
        let mut out = heapless::String::new();
        push_renderable(&mut out, s);
        out
    }

    #[test]
    fn keeps_ascii_and_latin1() {
        // Issue #111: Æ and accented letters must survive to the renderer.
        assert_eq!(filter("Ægg").as_str(), "Ægg");
        assert_eq!(filter("José").as_str(), "José");
        assert_eq!(filter("Zöe Müller").as_str(), "Zöe Müller");
        assert_eq!(filter("Hello").as_str(), "Hello");
    }

    #[test]
    fn drops_control_and_beyond_latin1() {
        assert_eq!(filter("a\u{7f}b").as_str(), "ab"); // DEL
        assert_eq!(filter("x\ty").as_str(), "xy"); // tab (control)
        assert_eq!(filter("q\u{85}w").as_str(), "qw"); // C1 control (< U+00A0)
        assert_eq!(filter("日本語").as_str(), ""); // CJK beyond Latin-1
        assert_eq!(filter("hi🎉").as_str(), "hi"); // emoji beyond Latin-1
    }
}

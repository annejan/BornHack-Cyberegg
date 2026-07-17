//! Token screen — collects tokens received over MeshCore or NFC and
//! shows them as a scrollable list that persists until reboot.
//!
//! Tokens are event/CTF handouts. Each distinct token is kept in a
//! fixed-size in-RAM list ([`MAX_TOKENS`] entries); duplicates are
//! ignored. The list survives screen changes and stays until the badge
//! is power-cycled. There is no visibility timer.
//!
//! # Triggering
//!
//! * **MeshCore**: any received message (channel or direct) whose
//!   plaintext starts with `"token:"` collects the substring after the
//!   colon.
//! * **NFC**: any NDEF text record written to the badge whose text
//!   starts with `"token:"` does the same.
//!
//! Both paths are intentionally **unauthenticated** — anyone can hand
//! you a token. The worst case is a spoofed/spammed list entry; no game
//! or badge state is affected.
//!
//! # Buttons
//!
//! On the token screen Up/Down scroll the list; Left/Right switch
//! screens as usual (see [`dispatch`]).

use core::cell::RefCell;
use core::sync::atomic::AtomicUsize;

// ---------------------------------------------------------------------------
// Mutex — embassy vs simulator (mirrors NODE_NAME in lib.rs)
// ---------------------------------------------------------------------------
#[cfg(feature = "embassy-base")]
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{TriColor, draw_frame};

/// Maximum byte length of a single stored token string.
pub const TOKEN_MAX_LEN: usize = 64;

/// Maximum number of distinct tokens retained (fixed static allocation:
/// `MAX_TOKENS * TOKEN_MAX_LEN` ≈ 1 KiB in `.bss`). When full, collecting
/// a new distinct token drops the oldest.
pub const MAX_TOKENS: usize = 16;

/// The collected tokens, newest first.
#[cfg(feature = "embassy-base")]
pub static TOKEN_LIST: Mutex<
    CriticalSectionRawMutex,
    RefCell<heapless::Vec<heapless::String<TOKEN_MAX_LEN>, MAX_TOKENS>>,
> = Mutex::new(RefCell::new(heapless::Vec::new()));

#[cfg(feature = "simulator")]
pub static TOKEN_LIST: std::sync::Mutex<
    RefCell<heapless::Vec<heapless::String<TOKEN_MAX_LEN>, MAX_TOKENS>>,
> = std::sync::Mutex::new(RefCell::new(heapless::Vec::new()));

/// Index of the top visible row in the list.
pub static TOKEN_SCROLL: AtomicUsize = AtomicUsize::new(0);

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Rows of the list visible at once (body 18..140 px, ~17 px pitch).
const VISIBLE_ROWS: usize = 6;
/// Baseline of the first list row.
const FIRST_ROW_Y: i32 = 34;
/// Vertical pitch between rows.
const ROW_PITCH: i32 = 17;
/// Characters per row that fit at 7 px/char on the 152 px panel with a
/// small left margin.
const CHARS_PER_ROW: usize = 20;
/// Continuation (wrapped) lines are indented by this many spaces so a
/// long token reads clearly as one entry spanning several rows.
const CONT_INDENT: usize = 2;
/// Characters of token text on a continuation line (after the indent).
const CONT_CHARS: usize = CHARS_PER_ROW - CONT_INDENT;

/// Number of wrapped display lines a token of `chars` characters needs:
/// the first line holds [`CHARS_PER_ROW`], each continuation line holds
/// [`CONT_CHARS`].
fn line_count(chars: usize) -> usize {
    if chars <= CHARS_PER_ROW {
        1
    } else {
        1 + (chars - CHARS_PER_ROW).div_ceil(CONT_CHARS)
    }
}

/// Byte index of the `n`-th char boundary in `s` (or `s.len()`).
fn char_byte(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map_or(s.len(), |(i, _)| i)
}

/// Slice of `s` shown on wrapped line `line` (0-based), plus whether it
/// is an indented continuation line. `None` past the end of the token.
fn wrapped_line(s: &str, line: usize) -> Option<(bool, &str)> {
    let total = s.chars().count();
    if line == 0 {
        Some((false, &s[..char_byte(s, CHARS_PER_ROW.min(total))]))
    } else {
        let start = CHARS_PER_ROW + (line - 1) * CONT_CHARS;
        if start >= total {
            return None;
        }
        let end = (start + CONT_CHARS).min(total);
        Some((true, &s[char_byte(s, start)..char_byte(s, end)]))
    }
}

// ---------------------------------------------------------------------------
// List access — embassy vs simulator
// ---------------------------------------------------------------------------

#[cfg(feature = "embassy-base")]
fn with_list<F, R>(f: F) -> R
where
    F: FnOnce(&mut heapless::Vec<heapless::String<TOKEN_MAX_LEN>, MAX_TOKENS>) -> R,
{
    TOKEN_LIST.lock(|cell| f(&mut cell.borrow_mut()))
}

#[cfg(feature = "simulator")]
fn with_list<F, R>(f: F) -> R
where
    F: FnOnce(&mut heapless::Vec<heapless::String<TOKEN_MAX_LEN>, MAX_TOKENS>) -> R,
{
    let guard = TOKEN_LIST.lock().unwrap();
    f(&mut guard.borrow_mut())
}

// ---------------------------------------------------------------------------
// Public API called by MeshCore / NFC handlers
// ---------------------------------------------------------------------------

/// Sanitize `value` into `out`: keep printable ASCII (0x20..=0x7E) only,
/// dropping control characters and non-ASCII bytes, then trim surrounding
/// spaces. Truncates at [`TOKEN_MAX_LEN`]. Returns the trimmed slice.
fn sanitize(value: &str, out: &mut heapless::String<TOKEN_MAX_LEN>) {
    out.clear();
    for b in value.bytes() {
        if (0x20..=0x7E).contains(&b) {
            // `out` is capacity TOKEN_MAX_LEN; push a single ASCII byte.
            if out.push(b as char).is_err() {
                break;
            }
        }
    }
}

/// Collect `value` as a token. No-op if, after sanitizing and trimming,
/// the token is empty or already present. When the list is full the
/// oldest entry is dropped to make room. Newest tokens sort to the front.
#[cfg(any(feature = "embassy-base", feature = "simulator"))]
pub fn set_token(value: &str) {
    let mut buf: heapless::String<TOKEN_MAX_LEN> = heapless::String::new();
    sanitize(value, &mut buf);
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        return;
    }
    let mut token: heapless::String<TOKEN_MAX_LEN> = heapless::String::new();
    let _ = token.push_str(trimmed);

    let inserted = with_list(|list| {
        if list.contains(&token) {
            return false;
        }
        // Drop the oldest (back) entry if we are at capacity.
        if list.is_full() {
            list.pop();
        }
        // Shift everything down one and place the newcomer at the front.
        // `insert` on heapless::Vec is O(n) but n ≤ 16.
        let _ = list.insert(0, token);
        true
    });

    if inserted {
        // Show the freshly collected token: reset the scroll to the top.
        TOKEN_SCROLL.store(0, core::sync::atomic::Ordering::Relaxed);
        signal_redraw();
    }
}

// ---------------------------------------------------------------------------
// Button handling
// ---------------------------------------------------------------------------

/// Wake the display loop so the token screen redraws. No-op off-target.
fn signal_redraw() {
    #[cfg(feature = "embassy-base")]
    crate::TOKEN_SIGNAL.signal(());
}

/// Total wrapped display lines across all collected tokens — the extent
/// the vertical scroll ranges over.
fn total_lines() -> usize {
    with_list(|list| list.iter().map(|t| line_count(t.chars().count())).sum())
}

/// Dispatch a button press on the token screen. Returns `true` when the
/// press was consumed (Up/Down scrolling) so the caller stops here;
/// `false` for anything else so screen navigation still works.
#[cfg(any(feature = "embassy-base", feature = "simulator"))]
pub fn dispatch(btn: crate::menu::ButtonId) -> bool {
    use crate::menu::ButtonId;
    use core::sync::atomic::Ordering::Relaxed;
    match btn {
        ButtonId::Up => {
            let cur = TOKEN_SCROLL.load(Relaxed);
            TOKEN_SCROLL.store(cur.saturating_sub(1), Relaxed);
            signal_redraw();
            true
        }
        ButtonId::Down => {
            let cur = TOKEN_SCROLL.load(Relaxed);
            // Last valid top line so the final row still fills the screen.
            let max_top = total_lines().saturating_sub(VISIBLE_ROWS);
            if cur < max_top {
                TOKEN_SCROLL.store(cur + 1, Relaxed);
                signal_redraw();
            }
            true
        }
        _ => false,
    }
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

    let len = with_list(|list| list.len());
    let n_lines = total_lines();

    // Scroll ranges over wrapped lines, not tokens. Clamp defensively.
    let max_top = n_lines.saturating_sub(VISIBLE_ROWS);
    let mut top = TOKEN_SCROLL.load(core::sync::atomic::Ordering::Relaxed);
    if top > max_top {
        top = max_top;
    }

    // Header: "Tokens: N" (distinct tokens, not wrapped lines).
    let mut header: heapless::String<16> = heapless::String::new();
    let _ = header.push_str("Tokens: ");
    push_usize(&mut header, len);

    // Footer: line-position indicator only when the content overflows.
    let mut footer: heapless::String<16> = heapless::String::new();
    if n_lines > VISIBLE_ROWS {
        let first = top + 1;
        let last = (top + VISIBLE_ROWS).min(n_lines);
        push_usize(&mut footer, first);
        let _ = footer.push('-');
        push_usize(&mut footer, last);
        let _ = footer.push_str(" / ");
        push_usize(&mut footer, n_lines);
    }
    let footer_ref = if footer.is_empty() {
        None
    } else {
        Some(footer.as_str())
    };

    draw_frame(display, Some((header.as_str(), &bat)), footer_ref)?;

    if len == 0 {
        // Empty state — centered placeholder.
        let centered = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        Text::with_text_style(
            "No tokens yet",
            Point::new(76, 84),
            crate::ui::TEXT_BOLD_BLACK,
            centered,
        )
        .draw(display)?;
        return Ok(());
    }

    // Collect the visible window of wrapped lines under one short lock —
    // long tokens wrap across several rows, continuation lines indented —
    // so the mutex is not held across the (slow) EPD draw calls.
    let mut rows: heapless::Vec<heapless::String<CHARS_PER_ROW>, VISIBLE_ROWS> =
        heapless::Vec::new();
    with_list(|list| {
        let mut gline = 0usize;
        'outer: for tok in list.iter() {
            let lc = line_count(tok.chars().count());
            for l in 0..lc {
                if gline >= top {
                    if gline >= top + VISIBLE_ROWS {
                        break 'outer;
                    }
                    if let Some((indent, text)) = wrapped_line(tok.as_str(), l) {
                        let mut s: heapless::String<CHARS_PER_ROW> = heapless::String::new();
                        if indent {
                            for _ in 0..CONT_INDENT {
                                let _ = s.push(' ');
                            }
                        }
                        let _ = s.push_str(text);
                        let _ = rows.push(s);
                    }
                }
                gline += 1;
            }
        }
    });

    let left = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Left)
        .build();
    for (row, line) in rows.iter().enumerate() {
        let y = FIRST_ROW_Y + row as i32 * ROW_PITCH;
        Text::with_text_style(
            line.as_str(),
            Point::new(4, y),
            crate::ui::TEXT_BOLD_BLACK,
            left,
        )
        .draw(display)?;
    }

    Ok(())
}

/// Append a small unsigned decimal to a heapless string. No-op on overflow.
fn push_usize<const N: usize>(s: &mut heapless::String<N>, mut v: usize) {
    if v == 0 {
        let _ = s.push('0');
        return;
    }
    let mut digits = [0u8; 20];
    let mut i = 0;
    while v > 0 {
        digits[i] = b'0' + (v % 10) as u8;
        v /= 10;
        i += 1;
    }
    while i > 0 {
        i -= 1;
        if s.push(digits[i] as char).is_err() {
            return;
        }
    }
}

#[cfg(all(test, feature = "simulator"))]
mod tests {
    use super::*;
    use crate::menu::ButtonId;
    use core::sync::atomic::Ordering::Relaxed;

    fn reset() {
        with_list(|l| l.clear());
        TOKEN_SCROLL.store(0, Relaxed);
    }

    fn snapshot() -> std::vec::Vec<std::string::String> {
        with_list(|l| l.iter().map(|s| s.as_str().to_string()).collect())
    }

    #[test]
    fn wrapping_line_math() {
        assert_eq!(line_count(0), 1);
        assert_eq!(line_count(CHARS_PER_ROW), 1); // exactly one row
        assert_eq!(line_count(CHARS_PER_ROW + 1), 2); // spills to a second
        assert_eq!(line_count(CHARS_PER_ROW + CONT_CHARS), 2); // fills 2nd
        assert_eq!(line_count(CHARS_PER_ROW + CONT_CHARS + 1), 3);

        // 30-char token: line 0 = first 20, line 1 = next up-to-18, no line 2.
        let s: std::string::String = "a".repeat(30);
        let (i0, l0) = wrapped_line(&s, 0).unwrap();
        assert!(!i0);
        assert_eq!(l0.len(), CHARS_PER_ROW);
        let (i1, l1) = wrapped_line(&s, 1).unwrap();
        assert!(i1); // continuation is indented
        assert_eq!(l1.len(), 30 - CHARS_PER_ROW);
        assert!(wrapped_line(&s, 2).is_none());
    }

    #[test]
    fn sanitize_strips_and_trims() {
        let mut out = heapless::String::<TOKEN_MAX_LEN>::new();
        sanitize("  a\tb\nc\u{00e9}d  ", &mut out);
        // Tab/newline/non-ASCII dropped; leading/trailing space trimmed later.
        assert_eq!(out.as_str(), "  abcd  ");
        assert_eq!(out.trim(), "abcd");
    }

    #[test]
    fn list_semantics() {
        // NB: TOKEN_LIST/TOKEN_SCROLL are process-global; keep every
        // global-touching assertion in this single serial test.
        reset();

        // Empty / whitespace-only / control-only inputs are ignored.
        set_token("");
        set_token("   ");
        set_token("\u{0001}\u{0002}");
        assert_eq!(snapshot().len(), 0);

        // Newest sorts to the front.
        set_token("alpha");
        set_token("bravo");
        assert_eq!(snapshot(), ["bravo", "alpha"]);

        // Duplicates are ignored (no reorder, no growth).
        set_token("alpha");
        assert_eq!(snapshot(), ["bravo", "alpha"]);

        // Sanitized before compare: "bravo\n" == existing "bravo".
        set_token("bravo\n");
        assert_eq!(snapshot(), ["bravo", "alpha"]);

        // Overflow drops the oldest (back) entry, keeps newest at front.
        reset();
        for i in 0..(MAX_TOKENS + 4) {
            let mut s = heapless::String::<TOKEN_MAX_LEN>::new();
            let _ = s.push_str("t");
            push_usize(&mut s, i);
            set_token(s.as_str());
        }
        let snap = snapshot();
        assert_eq!(snap.len(), MAX_TOKENS);
        assert_eq!(snap[0], "t19"); // last inserted
        assert_eq!(snap[MAX_TOKENS - 1], "t4"); // t0..t3 dropped

        // Scroll clamps to [0, len-VISIBLE_ROWS]; Up/Down consumed.
        reset();
        for i in 0..10 {
            let mut s = heapless::String::<TOKEN_MAX_LEN>::new();
            push_usize(&mut s, i);
            set_token(s.as_str());
        }
        // 10 entries, 6 visible → max top index = 4.
        assert!(dispatch(ButtonId::Down));
        assert!(dispatch(ButtonId::Down));
        assert_eq!(TOKEN_SCROLL.load(Relaxed), 2);
        for _ in 0..20 {
            dispatch(ButtonId::Down);
        }
        assert_eq!(TOKEN_SCROLL.load(Relaxed), 4); // clamped
        for _ in 0..20 {
            dispatch(ButtonId::Up);
        }
        assert_eq!(TOKEN_SCROLL.load(Relaxed), 0); // clamped
        assert!(!dispatch(ButtonId::Left)); // nav keys not consumed
        assert!(!dispatch(ButtonId::Fire));
    }
}

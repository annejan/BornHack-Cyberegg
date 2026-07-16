//! Word-aware line breaker shared across the on-device text surfaces.
//!
//! [`fw::mesh::pm_inbox`] (per-peer DM threads) and
//! [`fw::mesh::channel_browser`] (group-channel messages) need to lay
//! out wrapped text on the 152-px-wide tri-color e-paper; the game's
//! Friends screen (`game::friends_view`) does too, for friend names
//! that don't fit one line. Before this module the two mesh surfaces
//! had divergent implementations: PM had a polished word-wrap with
//! newline / leading-whitespace handling; channels sliced the byte
//! buffer at fixed `chars_per_line` boundaries and rendered embedded
//! `\n` as junk glyphs (no `FONT_7X13` codepoint).
//!
//! Lives at the crate root (not under `fw::mesh`) so it's usable from
//! `game`-only builds that don't enable the `mesh` feature at all.
//!
//! This file exposes one free function — no generics, no display
//! target, no closures — so the abstraction is flash-neutral
//! (calling it from N sites instantiates one body, not N).
//!
//! [`fw::mesh::pm_inbox`]: crate::fw::mesh::pm_inbox
//! [`fw::mesh::channel_browser`]: crate::fw::mesh::channel_browser

/// Word-aware line breaker — walks `bytes` and produces a list of
/// `(start, end)` byte ranges, each yielding ≤ `max_chars` printable
/// chars when sliced from `bytes`.  Soft-wraps at space boundaries
/// when possible; hard-breaks on `\n` / `\r` / `\r\n` (rendered as
/// line breaks rather than junk glyphs); falls back to a hard cut
/// for words longer than `max_chars`.
///
/// Leading whitespace is preserved on the first line and on lines
/// that follow a hard break — important for indented content (e.g.
/// pasted code in a PM).  Only the space that caused a soft wrap is
/// consumed, so word-wrap continuation lines start at the next word.
///
/// Cap of 32 lines is enough for a 130-byte message at 14 chars/line
/// (≈ 10 lines) plus several explicit newlines.  Messages longer
/// than that get truncated at the wrap level — the caller's storage
/// limits already cap text length well below this.
pub fn word_wrap(bytes: &[u8], max_chars: usize) -> heapless::Vec<(u8, u8), 32> {
    // Validate UTF-8 once up front; we walk by `char_indices` below so
    // every cut lands on a codepoint boundary.  Non-UTF-8 input produces
    // an empty layout — callers already handle that via `unwrap_or("")`.
    let Ok(s) = core::str::from_utf8(bytes) else {
        return heapless::Vec::new();
    };

    /// Display columns occupied by a single codepoint in the badge's
    /// monospaced text grid: emoji = 2 (renders as a 14-px tile, two
    /// `FONT_7X13` cells), variation selectors = 0, everything else = 1.
    fn columns_for(c: char) -> usize {
        let cp = c as u32;
        if cp == 0xFE0E || cp == 0xFE0F {
            0
        } else if crate::fw::emoji::atlas_index(cp).is_some() {
            crate::fw::emoji::EMOJI_COLUMNS
        } else {
            1
        }
    }

    let mut lines: heapless::Vec<(u8, u8), 32> = heapless::Vec::new();
    let mut pos = 0usize;
    let mut after_soft_wrap = false;

    while pos < s.len() {
        // Only swallow leading spaces when the previous iteration
        // ended with a soft wrap — that space was the wrap point and
        // shouldn't appear at the start of the continuation.  After a
        // hard break (or on the first line, or after a hard cut),
        // leading whitespace is intentional content.
        if after_soft_wrap {
            while pos < s.len() && s.as_bytes()[pos] == b' ' {
                pos += 1;
            }
            if pos >= s.len() {
                break;
            }
        }
        let line_start = pos;

        // Walk codepoints, accumulating display columns, until we hit
        // `max_chars` columns or a hard-break (`\n` / `\r`).  `end` is
        // a byte index — char_indices guarantees codepoint boundaries.
        let mut column = 0usize;
        let mut end = s.len();
        let mut stopped_at_break = false;
        for (i, c) in s[line_start..].char_indices() {
            let abs_i = line_start + i;
            if c == '\n' || c == '\r' {
                end = abs_i;
                stopped_at_break = true;
                break;
            }
            let w = columns_for(c);
            if column + w > max_chars {
                end = abs_i;
                break;
            }
            column += w;
        }

        let bytes_slice = s.as_bytes();
        let mut soft_wrapped = false;

        // Soft-wrap: back up to a space so the line ends at a word
        // boundary.  If the only space in the line is part of the
        // leading-whitespace cluster (visible content would be empty),
        // fall through to a hard cut at the window edge.
        if !stopped_at_break && end < bytes_slice.len() && bytes_slice[end] != b' ' {
            let mut back = end;
            while back > line_start && bytes_slice[back - 1] != b' ' {
                back -= 1;
            }
            if back > line_start {
                let mut probe = back;
                while probe > line_start && bytes_slice[probe - 1] == b' ' {
                    probe -= 1;
                }
                if probe > line_start {
                    end = back;
                    soft_wrapped = true;
                }
            }
        } else if !stopped_at_break && end < bytes_slice.len() && bytes_slice[end] == b' ' {
            soft_wrapped = true;
        }

        // Trim trailing spaces from the visible slice.  Spaces are
        // ASCII, never part of a multi-byte UTF-8 sequence, so this
        // never violates codepoint boundaries.
        let mut visible_end = end;
        while visible_end > line_start && bytes_slice[visible_end - 1] == b' ' {
            visible_end -= 1;
        }
        let _ = lines.push((line_start as u8, visible_end as u8));
        if lines.is_full() {
            break;
        }

        pos = end;
        if stopped_at_break {
            if pos < bytes_slice.len() && bytes_slice[pos] == b'\r' {
                pos += 1;
            }
            if pos < bytes_slice.len() && bytes_slice[pos] == b'\n' {
                pos += 1;
            }
            after_soft_wrap = false;
        } else {
            after_soft_wrap = soft_wrapped;
        }
    }
    lines
}

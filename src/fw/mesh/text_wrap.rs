//! Word-aware line breaker shared by the on-device chat surfaces.
//!
//! Both [`pm_inbox`] (per-peer DM threads) and [`channel_browser`]
//! (group-channel messages) need to lay out wrapped text on the
//! 152-px-wide tri-color e-paper.  Before this module the two
//! surfaces had divergent implementations: PM had a polished
//! word-wrap with newline / leading-whitespace handling; channels
//! sliced the byte buffer at fixed `chars_per_line` boundaries and
//! rendered embedded `\n` as junk glyphs (no `FONT_7X13` codepoint).
//!
//! This file exposes one free function — no generics, no display
//! target, no closures — so the abstraction is flash-neutral
//! (calling it from N sites instantiates one body, not N).
//!
//! [`pm_inbox`]: super::pm_inbox
//! [`channel_browser`]: super::channel_browser

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
    let mut lines: heapless::Vec<(u8, u8), 32> = heapless::Vec::new();
    let is_break = |b: u8| b == b'\n' || b == b'\r';
    let mut pos = 0usize;
    let mut after_soft_wrap = false;
    while pos < bytes.len() {
        // Only swallow leading spaces when the previous iteration
        // ended with a soft wrap — that space was the wrap point and
        // shouldn't appear at the start of the continuation.  After a
        // hard break (or on the first line, or after a hard cut),
        // leading whitespace is intentional content.
        if after_soft_wrap {
            while pos < bytes.len() && bytes[pos] == b' ' {
                pos += 1;
            }
            if pos >= bytes.len() {
                break;
            }
        }
        let line_start = pos;

        // Scan up to max_chars or first hard-break char.
        let limit_window = (line_start + max_chars).min(bytes.len());
        let mut end = limit_window;
        for i in line_start..limit_window {
            if is_break(bytes[i]) {
                end = i;
                break;
            }
        }
        let stopped_at_break = end < bytes.len() && is_break(bytes[end]);
        let mut soft_wrapped = false;

        // Soft-wrap: try to back up to a space so the line ends at a
        // word boundary.  If the only space within the line is part
        // of the leading-whitespace cluster (visible content would be
        // empty), fall through to a hard cut at the window edge — a
        // mid-word break is uglier than indented content collapsing.
        if !stopped_at_break && end < bytes.len() && bytes[end] != b' ' {
            let mut back = end;
            while back > line_start && bytes[back - 1] != b' ' {
                back -= 1;
            }
            if back > line_start {
                let mut probe = back;
                while probe > line_start && bytes[probe - 1] == b' ' {
                    probe -= 1;
                }
                if probe > line_start {
                    end = back;
                    soft_wrapped = true;
                }
            }
        } else if !stopped_at_break && end < bytes.len() && bytes[end] == b' ' {
            // Cleanly stopped at a space at the window edge.
            soft_wrapped = true;
        }

        // Trim trailing spaces from the visible slice (so a line
        // ending in soft-wrap whitespace doesn't leave dangling
        // glyphs at the right edge).
        let mut visible_end = end;
        while visible_end > line_start && bytes[visible_end - 1] == b' ' {
            visible_end -= 1;
        }
        let _ = lines.push((line_start as u8, visible_end as u8));
        if lines.is_full() {
            break;
        }

        pos = end;
        if stopped_at_break {
            if pos < bytes.len() && bytes[pos] == b'\r' {
                pos += 1;
            }
            if pos < bytes.len() && bytes[pos] == b'\n' {
                pos += 1;
            }
            after_soft_wrap = false;
        } else {
            after_soft_wrap = soft_wrapped;
        }
    }
    lines
}

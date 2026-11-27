//! Read `BPETS_V3.CFG` from the USB-MSC FAT12 partition and apply its
//! overrides on top of the preset selected via the on-badge menu.
//!
//! File format — one `KEY=VALUE` per line, `#` introduces a comment.
//! Whitespace around the `=` is ignored.  Unknown keys are logged and
//! skipped, so adding a new tunable later doesn't break older files.
//!
//! ```text
//! # make food fill more, halve the starvation damage
//! MOOD_STEP=40
//! DMG_FULL_LOW_PM=20
//! ```
//!
//! Call [`load_and_install`] exactly once during boot, after `kv::init()`
//! and after `fat12::format_if_needed()`, but **before** the game engine
//! creates any [`crate::game::engine::GameState`].

#[cfg(feature = "embassy-core")]
use crate::game::engine::thresholds;
use crate::game::engine::thresholds::{Mode, Thresholds};

/// The v3 balance file.  The old firmware keeps its own `BORNPETS.CFG`,
/// which v3 never reads, writes or deletes.  The defmt log literals below
/// must match this name (defmt needs literals); keep them in sync.
#[cfg(feature = "embassy-core")]
const CFG_FILE: &str = "BPETS_V3.CFG";

#[cfg(feature = "embassy-core")]
const READ_CHUNK_LEN: usize = 512;

/// Longest config line kept whole (the longest template line is ~60).
#[cfg(feature = "embassy-core")]
const LINE_BUF_LEN: usize = 160;

/// Pick the active preset, optionally overlay `BPETS_V3.CFG`, install
/// the result into the threshold accessor.  Returns the [`Mode`] used
/// and whether the installed values differ from the bare preset (custom).
///
/// The file is streamed in small chunks, so every row is seen whatever the
/// file size; a line too long for the line buffer is skipped whole.
#[cfg(feature = "embassy-core")]
pub async fn load_and_install(mode: Mode) -> (Mode, bool) {
    use crate::cfg_text::LineAssembler;
    use crate::fw::fat12;

    let mut values = mode.preset();

    let file = match fat12::to_8_3(CFG_FILE) {
        Some(name) => fat12::find_file(&name).await.ok(),
        None => None,
    };
    let Some(file) = file else {
        thresholds::install(values, mode, false);
        return (mode, false);
    };

    let mut lines = LineAssembler::<LINE_BUF_LEN>::new();
    let mut chunk = [0u8; READ_CHUNK_LEN];
    let mut offset = 0u32;
    let mut applied = 0u32;
    let mut read_failed = false;
    loop {
        let n = match fat12::read_file(&file, offset, &mut chunk).await {
            Ok(n) => n,
            // A failed read is not EOF: stop here and do NOT run `finish`,
            // which would apply a partial last line (a cut-off value).
            // Only the lines that were complete have been applied.
            Err(_) => {
                read_failed = true;
                0
            }
        };
        if n == 0 {
            break;
        }
        offset = offset.saturating_add(n as u32);
        lines.push(&chunk[..n], &mut |l| {
            applied += apply_line(&mut values, l) as u32
        });
    }
    if !read_failed {
        lines.finish(&mut |l| applied += apply_line(&mut values, l) as u32);
    }

    let values = thresholds::validate(&values, &mode.preset());
    defmt::info!(
        "{=str}: applied {} override(s) on top of {}",
        CFG_FILE,
        applied,
        mode.label()
    );

    let custom = is_custom_set(&values, mode);
    thresholds::install(values, mode, custom);
    (mode, custom)
}

/// `true` when `values` differ from the bare preset of `mode`.
pub fn is_custom_set(values: &Thresholds, mode: Mode) -> bool {
    *values != mode.preset()
}

/// Apply one config line (`KEY=VALUE`, `#` comments) to `values`.  Returns
/// `true` when a known key got a value.  The per-line core of both the
/// badge's streaming loader and [`apply_overrides`].
pub fn apply_line(values: &mut Thresholds, line: &[u8]) -> bool {
    let Some((key, value)) = crate::cfg_text::kv_of_line(line, true) else {
        return false;
    };
    if key.is_empty() || value.is_empty() {
        return false;
    }
    let Ok(key_str) = core::str::from_utf8(key) else {
        return false;
    };
    let Some(value_num) = crate::cfg_text::parse_dec_u32(value) else {
        #[cfg(feature = "embassy-core")]
        defmt::warn!("BPETS_V3.CFG: cannot parse value for key");
        return false;
    };

    if let Some(setter) = crate::game::engine::thresholds::cfg_setter(key_str) {
        setter(values, value_num);
        return true;
    }
    #[cfg(feature = "embassy-core")]
    defmt::info!("BPETS_V3.CFG: unknown key, ignored");
    false
}

/// Parse `data` as one KEY=VALUE per line and apply each known key to
/// `values`.  Returns the number of rows that produced an actual edit.
///
/// Visible for tests in pure host builds.
pub fn apply_overrides(values: &mut Thresholds, data: &[u8]) -> u32 {
    data.split(|&b| b == b'\n')
        .map(|line| apply_line(values, line) as u32)
        .sum()
}

#[cfg(all(test, not(feature = "embassy-core")))]
mod tests {
    use super::*;

    #[test]
    fn applies_known_keys_and_ignores_unknown() {
        let mut t = Thresholds::CLASSIC;
        let n = apply_overrides(
            &mut t,
            b"# comment\nMOOD_STEP = 40\nUNKNOWN = 99\nDMG_FULL_LOW_PM=2\n",
        );
        assert_eq!(n, 2);
        assert_eq!(t.MOOD_STEP, 40);
        assert_eq!(t.DMG_FULL_LOW_PM, 2);
    }

    #[test]
    fn clamps_u16_overflow() {
        let mut t = Thresholds::CLASSIC;
        apply_overrides(&mut t, b"MOOD_STEP=999999\n");
        assert_eq!(t.MOOD_STEP, u16::MAX);
    }

    #[test]
    fn ignores_blank_and_malformed() {
        let mut t = Thresholds::CLASSIC;
        let n = apply_overrides(&mut t, b"\n  \n=\nkey_without_value=\n=value_without_key\n");
        assert_eq!(n, 0);
    }

    /// Feed `data` through the same assembler the badge uses, in `chunk`-byte
    /// pieces.
    fn apply_chunked(values: &mut Thresholds, data: &[u8], chunk: usize) -> u32 {
        let mut lines = crate::cfg_text::LineAssembler::<160>::new();
        let mut applied = 0u32;
        for piece in data.chunks(chunk) {
            lines.push(piece, &mut |l| applied += apply_line(values, l) as u32);
        }
        lines.finish(&mut |l| applied += apply_line(values, l) as u32);
        applied
    }

    #[test]
    fn chunked_read_applies_rows_straddling_chunks_and_the_last_row() {
        let template = crate::game::engine::thresholds::cfg_template();
        // Uncomment the first row, one in the middle and the very last one.
        let mut lines: std::vec::Vec<std::string::String> =
            template.lines().map(|l| l.to_string()).collect();
        let n = lines.len();
        for i in [1, n / 2, n - 1] {
            lines[i] = lines[i].strip_prefix("# ").unwrap().to_string();
        }
        // Values far from every preset so each row is a real edit.
        lines[1] = "HATCHING_TICKS=77".into();
        lines[n - 1] = "DOSE_FRIEND_SMALL_FUN_PCT=33".into();
        let text = lines.join("\n") + "\n";
        for chunk in [1, 7, 61, 100, 509] {
            let mut t = Thresholds::CLASSIC;
            let applied = apply_chunked(&mut t, text.as_bytes(), chunk);
            assert_eq!(applied, 3, "chunk {chunk}");
            assert_eq!(t.HATCHING_TICKS, 77, "chunk {chunk}");
            assert_eq!(t.DOSE_FRIEND_SMALL_FUN_PCT, 33, "chunk {chunk}");
        }
    }

    #[test]
    fn chunked_read_matches_whole_buffer_and_keeps_full_values() {
        let data = b"MOOD_STEP=40\nDMG_FULL_LOW_PM=20\nHATCHING_TICKS=55";
        for chunk in 1..=data.len() {
            let mut a = Thresholds::CLASSIC;
            let mut b = Thresholds::CLASSIC;
            apply_chunked(&mut a, data, chunk);
            apply_overrides(&mut b, data);
            assert_eq!(a, b, "chunk {chunk}");
            assert_eq!(
                a.HATCHING_TICKS, 55,
                "unterminated last line, chunk {chunk}"
            );
            assert_eq!(a.DMG_FULL_LOW_PM, 20);
        }
    }

    #[test]
    fn over_long_line_is_skipped_whole() {
        let mut data = std::vec::Vec::new();
        data.extend_from_slice(b"MOOD_STEP=");
        data.extend(core::iter::repeat(b'1').take(300)); // longer than the buffer
        data.extend_from_slice(b"\nHATCHING_TICKS=9\n");
        let mut t = Thresholds::CLASSIC;
        let applied = apply_chunked(&mut t, &data, 50);
        assert_eq!(applied, 1);
        assert_eq!(t.MOOD_STEP, Thresholds::CLASSIC.MOOD_STEP);
        assert_eq!(t.HATCHING_TICKS, 9);
    }
}

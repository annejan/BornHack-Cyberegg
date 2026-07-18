//! Read `BORNPETS.CFG` from the USB-MSC FAT12 partition and apply its
//! overrides on top of the preset selected via the on-badge menu.
//!
//! File format — one `KEY=VALUE` per line, `#` introduces a comment.
//! Whitespace around the `=` is ignored.  Unknown keys are logged and
//! skipped, so adding a new tunable later doesn't break older files.
//!
//! ```text
//! # speed up hunger decay, slow down sickness
//! HUNGER_RATE=4
//! SICK_RATE=2
//! ```
//!
//! Call [`load_and_install`] exactly once during boot, after `kv::init()`
//! and after `fat12::format_if_needed()`, but **before** the game engine
//! creates any [`crate::game::engine::GameState`].

use crate::game::engine::thresholds::{self, BORNPETS_CFG_KEYS, Mode, Thresholds};

const READ_BUF_LEN: usize = 2048;

/// Pick the active preset, optionally overlay `BORNPETS.CFG`, install
/// the result into the threshold accessor.  Returns the [`Mode`] used
/// and whether at least one valid override row was applied.
#[cfg(feature = "embassy-base")]
pub async fn load_and_install(mode: Mode) -> (Mode, bool) {
    use crate::fw::fat12;

    let mut values = mode.preset();

    let Some(name) = fat12::to_8_3("BORNPETS.CFG") else {
        thresholds::install(values, mode, false);
        return (mode, false);
    };
    let Ok(file) = fat12::find_file(&name).await else {
        thresholds::install(values, mode, false);
        return (mode, false);
    };

    let mut buf = [0u8; READ_BUF_LEN];
    let n = match fat12::read_file(&file, 0, &mut buf).await {
        Ok(n) => n,
        Err(_) => {
            thresholds::install(values, mode, false);
            return (mode, false);
        }
    };

    let applied = apply_overrides(&mut values, &buf[..n]);
    defmt::info!(
        "BORNPETS.CFG: applied {} override(s) on top of {}",
        applied,
        mode.label()
    );

    thresholds::install(values, mode, applied > 0);
    (mode, applied > 0)
}

/// Parse `data` as one KEY=VALUE per line and apply each known key to
/// `values`.  Returns the number of rows that produced an actual edit.
///
/// Visible for tests in pure host builds.
pub fn apply_overrides(values: &mut Thresholds, data: &[u8]) -> u32 {
    let mut applied = 0u32;

    for line in data.split(|&b| b == b'\n') {
        let line = strip_cr(line);
        let line = strip_comment(line);
        let line = trim_ascii(line);
        if line.is_empty() {
            continue;
        }

        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            continue;
        };
        let (key, rest) = line.split_at(eq);
        let key = trim_ascii(key);
        let value = trim_ascii(&rest[1..]);
        if key.is_empty() || value.is_empty() {
            continue;
        }

        let Ok(key_str) = core::str::from_utf8(key) else {
            continue;
        };
        let Some(value_num) = parse_u32(value) else {
            #[cfg(feature = "embassy-base")]
            defmt::warn!("BORNPETS.CFG: cannot parse value for key");
            continue;
        };

        let mut handled = false;
        for (name, setter) in BORNPETS_CFG_KEYS {
            if key_str.eq_ignore_ascii_case(name) {
                setter(values, value_num);
                applied += 1;
                handled = true;
                break;
            }
        }
        if !handled {
            #[cfg(feature = "embassy-base")]
            defmt::info!("BORNPETS.CFG: unknown key, ignored");
        }
    }

    applied
}

fn strip_cr(line: &[u8]) -> &[u8] {
    if line.last() == Some(&b'\r') {
        &line[..line.len() - 1]
    } else {
        line
    }
}

fn strip_comment(line: &[u8]) -> &[u8] {
    match line.iter().position(|&b| b == b'#') {
        Some(i) => &line[..i],
        None => line,
    }
}

fn trim_ascii(s: &[u8]) -> &[u8] {
    let mut start = 0;
    let mut end = s.len();
    while start < end && (s[start] == b' ' || s[start] == b'\t') {
        start += 1;
    }
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\t') {
        end -= 1;
    }
    &s[start..end]
}

fn parse_u32(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(acc)
}

#[cfg(all(test, not(feature = "embassy-base")))]
mod tests {
    use super::*;

    #[test]
    fn applies_known_keys_and_ignores_unknown() {
        let mut t = Thresholds::CLASSIC;
        let n = apply_overrides(
            &mut t,
            b"# comment\nHUNGER_RATE = 4\nUNKNOWN = 99\nSICK_RATE=2\n",
        );
        assert_eq!(n, 2);
        assert_eq!(t.HUNGER_RATE, 4);
        assert_eq!(t.SICK_RATE, 2);
    }

    #[test]
    fn clamps_u16_overflow() {
        let mut t = Thresholds::CLASSIC;
        apply_overrides(&mut t, b"HUNGER_RATE=999999\n");
        assert_eq!(t.HUNGER_RATE, u16::MAX);
    }

    #[test]
    fn ignores_blank_and_malformed() {
        let mut t = Thresholds::CLASSIC;
        let n = apply_overrides(&mut t, b"\n  \n=\nkey_without_value=\n=value_without_key\n");
        assert_eq!(n, 0);
    }
}

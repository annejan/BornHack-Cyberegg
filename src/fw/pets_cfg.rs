//! Read `PETS.CFG` from the USB-MSC FAT12 partition and register any custom
//! pets it declares, so new pets can be added without a firmware reflash.
//!
//! Format — one `PREFIX=NAME` per line, `#` introduces a comment.  `PREFIX`
//! is the pet's sprite-prefix byte in decimal.  `0`/`1`/`2` are the built-in
//! pets — listing them just renames them; `3` and `4` are reserved (sponsors,
//! menu icons) and ignored; new pets use `5..7`.  The pet's sprites are the
//! matching `PPAAFF.PCX` files (e.g. `050100.PCX` for pet 5's idle-frame 0).
//! Names longer than [`NAME_CAP`] chars are truncated.
//!
//! ```text
//! # rename a built-in, add two new pets (drop 05xxxx.PCX / 06xxxx.PCX too)
//! 0=Bartho
//! 5=Dragon
//! 6=Ghost
//! ```
//!
//! Call [`load_and_install`] exactly once at boot, after
//! `fat12::format_if_needed()` and before the game reads the roster.

use crate::game::pet_registry::{self, MAX_PETS, NAME_CAP, PetDef};

const READ_BUF_LEN: usize = 1024;

/// Load `PETS.CFG` (if present) and install the resulting roster.  Always
/// calls [`pet_registry::install`] — with an empty custom list when the file
/// is missing or unreadable — so the built-in roster is set up either way.
#[cfg(feature = "embassy-base")]
pub async fn load_and_install() {
    use crate::fw::fat12;

    let mut customs: heapless::Vec<PetDef, MAX_PETS> = heapless::Vec::new();

    if let Some(name) = fat12::to_8_3("PETS.CFG")
        && let Ok(file) = fat12::find_file(&name).await
    {
        let mut buf = [0u8; READ_BUF_LEN];
        if let Ok(n) = fat12::read_file(&file, 0, &mut buf).await {
            let parsed = parse(&buf[..n], &mut customs);
            defmt::info!("PETS.CFG: {} custom pet(s) parsed", parsed);
        }
    }

    pet_registry::install(&customs);
}

/// Parse `data` as one `PREFIX=NAME` per line into `out`.  Returns the number
/// of rows accepted.  Reserved prefixes and malformed rows are skipped;
/// over-long names are truncated at [`NAME_CAP`].  Visible for host tests.
pub fn parse(data: &[u8], out: &mut heapless::Vec<PetDef, MAX_PETS>) -> u32 {
    let mut added = 0u32;

    for line in data.split(|&b| b == b'\n') {
        let line = trim(strip_comment(strip_cr(line)));
        if line.is_empty() {
            continue;
        }

        let Some(eq) = line.iter().position(|&b| b == b'=') else {
            continue;
        };
        let (key, rest) = line.split_at(eq);
        let key = trim(key);
        let value = trim(&rest[1..]);
        if key.is_empty() || value.is_empty() {
            continue;
        }

        let Some(id) = parse_u8(key) else {
            continue;
        };
        if !pet_registry::is_pet_prefix(id) {
            continue; // reserved (3 sponsors, 4 menu) or out of range
        }

        let mut name: heapless::String<NAME_CAP> = heapless::String::new();
        for &b in value {
            // ASCII only; drop anything else so names stay renderable.
            if !b.is_ascii() || name.push(b as char).is_err() {
                break; // truncate at cap / first non-ascii
            }
        }
        if name.is_empty() {
            continue;
        }

        if out.push(PetDef { id, name }).is_err() {
            break; // roster full
        }
        added += 1;
    }

    added
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

fn trim(s: &[u8]) -> &[u8] {
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

fn parse_u8(s: &[u8]) -> Option<u8> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u16 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((b - b'0') as u16)?;
        if acc > 255 {
            return None;
        }
    }
    Some(acc as u8)
}

#[cfg(all(test, not(feature = "embassy-base")))]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_rows_and_skips_reserved() {
        let mut out = heapless::Vec::new();
        // 3 is reserved (sponsors); 5 and 6 are valid.
        let n = parse(b"# pets\n3=Nope\n5=Dragon\n6 = Ghost \n", &mut out);
        assert_eq!(n, 2);
        assert_eq!(out[0].id, 5);
        assert_eq!(out[0].name.as_str(), "Dragon");
        assert_eq!(out[1].id, 6);
        assert_eq!(out[1].name.as_str(), "Ghost");
    }

    #[test]
    fn truncates_long_names_and_ignores_malformed() {
        let mut out = heapless::Vec::new();
        let n = parse(
            b"5=ThisNameIsWayTooLongToFit\n=noident\n7=\nxx=bad\n",
            &mut out,
        );
        assert_eq!(n, 1);
        assert_eq!(out[0].name.as_str().len(), NAME_CAP);
    }
}

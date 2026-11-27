//! Read `PETS.CFG` from the USB-MSC FAT12 partition and register the pets it
//! declares.  This file is the *only* source of the roster — no pet is baked
//! into the firmware, so pets are added, renamed and removed without a
//! reflash, and a badge with no `PETS.CFG` has no selectable pet at all.
//!
//! Format — one `PREFIX=NAME` per line, `#` introduces a comment.  `PREFIX`
//! is the pet's sprite-prefix byte in decimal; `3` and `4` are reserved
//! (sponsors, menu icons) and ignored, anything else below
//! [`pet_registry::MAX_PET_PREFIX`] is a pet slot.  The pet's sprites are the
//! matching `PPAAFF.PCX` files (e.g. `050100.PCX` for pet 5's idle-frame 0).
//! Names longer than [`NAME_CAP`] chars are truncated.  Rows are listed in
//! selection-screen order.
//!
//! ```text
//! # the shipped roster — each line needs matching PPxxxx.PCX sprites
//! 0=Bartholomeus
//! 1=Cat
//! 5=Panda
//! ```
//!
//! Old-format files: before 2026-11-09 Bartholomeus (0), Cat (1) and Slug (2)
//! were built into the firmware and `PETS.CFG` listed only the extra pets.  A
//! file that declares at least one pet but none of the prefixes 0, 1, 2 is
//! taken for that old format, and [`migrate_old_format`] puts those three in
//! front of its pets (in memory only; the file on the badge is never
//! rewritten).  They then pass the same `pet_registry::drop_without_sprites`
//! pass as every other pet, so a missing sprite set still hides one.  A file
//! with any of 0, 1, 2 is new-format and left alone, so a pet removed from
//! it stays removed; an empty file stays an empty roster.
//!
//! Call [`load_and_install`] exactly once at boot, after
//! `fat12::format_if_needed()` and before the game reads the roster.

use crate::game::pet_registry::{self, MAX_PETS, NAME_CAP, PetDef};

/// Only the FAT12 read path needs this; host builds (simulator, unit tests)
/// pull in just [`parse`].
#[cfg(feature = "embassy-core")]
const READ_BUF_LEN: usize = 1024;

/// Load `PETS.CFG` (if present) and install the resulting roster.  Always
/// calls [`pet_registry::install`], so the registry is marked installed even
/// when the file is missing or unreadable — in that case with an empty list,
/// leaving the badge with no selectable pet.
#[cfg(feature = "embassy-core")]
pub async fn load_and_install() {
    use crate::fw::fat12;

    let mut pets: heapless::Vec<PetDef, MAX_PETS> = heapless::Vec::new();

    let mut buf = [0u8; READ_BUF_LEN];
    if let Ok(n) = fat12::read_named("PETS.CFG", &mut buf).await {
        let parsed = parse(&buf[..n], &mut pets);
        defmt::info!("PETS.CFG: {} pet(s) parsed", parsed);
        if migrate_old_format(&mut pets) {
            defmt::info!("PETS.CFG: old format, built-in pets 0-2 added");
        }
    }

    if pets.is_empty() {
        defmt::warn!("PETS.CFG: no pets declared — none will be selectable");
    }
    pet_registry::install(&pets);
}

/// Parse `data` as one `PREFIX=NAME` per line into `out`.  Returns the number
/// of rows accepted.  Reserved prefixes and malformed rows are skipped;
/// over-long names are truncated at [`NAME_CAP`].  Visible for host tests.
pub fn parse(data: &[u8], out: &mut heapless::Vec<PetDef, MAX_PETS>) -> u32 {
    let mut added = 0u32;

    for (key, value) in crate::cfg_text::kv_lines(data, true) {
        let Some(id) = crate::cfg_text::parse_dec_u32(key).and_then(|v| u8::try_from(v).ok()) else {
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

/// The pets that used to be built into the firmware, in roster order.
const LEGACY_PETS: [(u8, &str); 3] = [(0, "Bartholomeus"), (1, "Cat"), (2, "Slug")];

/// Treat `pets` as an old-format roster when it is non-empty and holds none
/// of the prefixes 0, 1, 2: put [`LEGACY_PETS`] in front of its entries.
/// The roster never exceeds [`MAX_PETS`]; when it would, the three legacy pets
/// are kept and entries are dropped from the end.  Returns whether it migrated.
/// The sprite check is not done here: `drop_without_sprites` runs later.
pub fn migrate_old_format(pets: &mut heapless::Vec<PetDef, MAX_PETS>) -> bool {
    if pets.is_empty() || pets.iter().any(|p| LEGACY_PETS.iter().any(|&(id, _)| id == p.id)) {
        return false;
    }
    let mut merged: heapless::Vec<PetDef, MAX_PETS> = heapless::Vec::new();
    for &(id, name) in &LEGACY_PETS {
        let mut n: heapless::String<NAME_CAP> = heapless::String::new();
        let _ = n.push_str(name);
        let _ = merged.push(PetDef { id, name: n });
    }
    for p in pets.iter() {
        if merged.push(p.clone()).is_err() {
            break; // roster full
        }
    }
    *pets = merged;
    true
}

#[cfg(all(test, not(feature = "embassy-core")))]
mod tests {
    use super::*;

    fn roster_of(data: &[u8]) -> std::vec::Vec<(u8, std::string::String)> {
        let mut out = heapless::Vec::new();
        parse(data, &mut out);
        migrate_old_format(&mut out);
        out.iter().map(|p| (p.id, p.name.as_str().into())).collect()
    }

    fn ids(data: &[u8]) -> std::vec::Vec<u8> {
        roster_of(data).iter().map(|p| p.0).collect()
    }

    #[test]
    fn old_file_gets_the_three_builtins_in_front() {
        let r = roster_of(b"5=Panda\n6=Badger\n7=Seastar\n8=Jellyfish\n");
        let ids: std::vec::Vec<u8> = r.iter().map(|p| p.0).collect();
        assert_eq!(ids, [0, 1, 2, 5, 6, 7, 8]);
        assert_eq!(r[0].1, "Bartholomeus");
        assert_eq!(r[1].1, "Cat");
        assert_eq!(r[2].1, "Slug");
        assert_eq!(r[3].1, "Panda");
    }

    #[test]
    fn new_format_files_are_unchanged() {
        assert_eq!(ids(b"0=A\n1=B\n2=C\n5=D\n6=E\n7=F\n"), [0, 1, 2, 5, 6, 7]);
        assert_eq!(ids(b"1=Cat\n5=Panda\n"), [1, 5]);
        assert_eq!(ids(b"2=Slug\n"), [2]);
    }

    #[test]
    fn empty_or_comment_only_file_stays_empty() {
        assert!(ids(b"").is_empty());
        assert!(ids(b"# nothing here\n# 5=Panda\n").is_empty());
        assert!(ids(b"3=Reserved\n").is_empty());
    }

    #[test]
    fn overflow_keeps_the_builtins_and_drops_from_the_end() {
        let mut out: heapless::Vec<PetDef, MAX_PETS> = heapless::Vec::new();
        // Distinct ids are limited to 9, so repeat prefixes: the parser keeps
        // duplicates as separate rows, which fills the roster.
        let mut text = std::string::String::new();
        for i in 0..MAX_PETS {
            text.push_str(&std::format!("{}=P{}\n", 5 + (i % 4), i));
        }
        assert_eq!(parse(text.as_bytes(), &mut out) as usize, MAX_PETS);
        assert!(migrate_old_format(&mut out));
        assert_eq!(out.len(), MAX_PETS);
        assert_eq!(out[0].name.as_str(), "Bartholomeus");
        assert_eq!(out[1].name.as_str(), "Cat");
        assert_eq!(out[2].name.as_str(), "Slug");
        assert_eq!(out[3].name.as_str(), "P0");
        assert_eq!(out[MAX_PETS - 1].name.as_str(), std::format!("P{}", MAX_PETS - 4));
    }

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

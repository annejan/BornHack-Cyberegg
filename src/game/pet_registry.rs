//! Runtime pet roster.
//!
//! The roster comes entirely from the `PETS.CFG` manifest on the USB-MSC
//! partition — there are no built-in pets.  See [`crate::game::pets_cfg`].
//! One exception to "no built-ins" in effect: an old-format `PETS.CFG` (pets
//! declared, none of the prefixes 0, 1, 2) is migrated in memory to start with
//! Bartholomeus, Cat and Slug before [`install`] sees it.
//! A badge whose `PETS.CFG` is missing, empty or unreadable has an empty
//! roster and no pet can be selected until the file is restored.
//!
//! A pet is a pure cosmetic skin: a sprite-prefix byte plus a display name.
//! All game behaviour (stats, decay, traits, animations) is identical across
//! pets, so a pet needs only its `PPAAFF.PCX` sprites at the matching prefix
//! and one manifest line.  Frame counts are auto-discovered from the FAT
//! directory catalogue, so no per-pet code is required.

use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use super::engine::PetKind;

/// Maximum total pets in the roster (3 built-in + custom).
pub const MAX_PETS: usize = 12;
/// Maximum display-name length (bytes).
pub const NAME_CAP: usize = 16;
/// Sprite prefixes reserved for non-pet assets: `3` sponsor slideshow,
/// `4` menu icons.  Everything else `< MAX_PET_PREFIX` is a pet.
pub const RESERVED_PREFIXES: [u8; 2] = [3, 4];

/// Exclusive upper bound on a pet's sprite prefix.  The shipped roster uses
/// `0..=2` and `5..=7`; `3`/`4` are reserved.  Must stay in sync with
/// `sprite_loader::PP_MAX` so the frame catalogue covers every pet.  Kept
/// small deliberately — every extra prefix grows the catalogue table, and
/// the debug build is RAM-tight.
pub const MAX_PET_PREFIX: u8 = 9;

/// Whether `id` is usable as a pet's sprite prefix (in range, not reserved).
pub fn is_pet_prefix(id: u8) -> bool {
    id < MAX_PET_PREFIX && !RESERVED_PREFIXES.contains(&id)
}

/// One roster entry: sprite-prefix byte + display name.
#[derive(Clone)]
pub struct PetDef {
    pub id: u8,
    pub name: heapless::String<NAME_CAP>,
}

struct Registry {
    defs: heapless::Vec<PetDef, MAX_PETS>,
    ids: heapless::Vec<PetKind, MAX_PETS>,
    /// Bit `id` set: pet `id` was dropped from `ids` for having no idle sprite.
    dropped: u16,
}

/// Shown for an id that isn't in the roster — a save whose pet was dropped
/// from `PETS.CFG`, or any lookup before the manifest has been installed.
const UNKNOWN_NAME: &str = "Pet";

// Single global roster, populated once at boot.  `heapless` collections are
// const-constructible, so this needs no allocator or `StaticCell` (keeping
// the module dependency-free for the simulator build).  Mirrors the
// install-once pattern in `engine::thresholds`.  Two boot-time writes, both
// before any reader: `install`, then `drop_without_sprites`.
static mut REGISTRY: Registry = Registry {
    defs: heapless::Vec::new(),
    ids: heapless::Vec::new(),
    dropped: 0,
};
/// The ids [`drop_without_sprites`] dropped, readable without touching `REGISTRY`.
static DROPPED: AtomicU16 = AtomicU16::new(0);
static INSTALLED: AtomicBool = AtomicBool::new(false);

fn current() -> Option<&'static Registry> {
    if INSTALLED.load(Ordering::Acquire) {
        // SAFETY: `REGISTRY` is written at boot only: once in `install` (before
        // the `INSTALLED` Release store) and once in `drop_without_sprites`
        // (`ids` and `dropped`, before any task reads the roster).  Boot is
        // single-threaded, so no concurrent access occurs.
        Some(unsafe { &*core::ptr::addr_of!(REGISTRY) })
    } else {
        None
    }
}

/// Install the roster from the manifest `entries`, in file order.  Reserved /
/// out-of-range ids, duplicates and overflow past [`MAX_PETS`] are dropped; a
/// repeated id renames the earlier entry rather than listing the pet twice.
///
/// Call once at boot (see [`crate::game::pets_cfg::load_and_install`]).  A
/// second call is ignored.  There is no built-in roster: installing an empty
/// slice — which is what a missing or unreadable `PETS.CFG` produces — leaves
/// the badge with no selectable pets.
pub fn install(entries: &[PetDef]) {
    if INSTALLED.load(Ordering::Acquire) {
        return; // already installed — ignore
    }

    // SAFETY: exclusive access — install runs once at boot before any reader,
    // and the `INSTALLED` guard above prevents a second populate.
    let reg = unsafe { &mut *core::ptr::addr_of_mut!(REGISTRY) };

    for e in entries {
        if !is_pet_prefix(e.id) {
            continue;
        }
        if let Some(d) = reg.defs.iter_mut().find(|d| d.id == e.id) {
            d.name = e.name.clone(); // duplicate id — last name wins
            continue;
        }
        if reg.defs.push(e.clone()).is_err() {
            break; // roster full
        }
        let _ = reg.ids.push(PetKind(e.id));
    }

    INSTALLED.store(true, Ordering::Release);
}

/// Display name for a pet id, or [`UNKNOWN_NAME`] when the id isn't in the
/// installed roster (or nothing has been installed yet).
pub fn name_of(id: u8) -> &'static str {
    if let Some(reg) = current() {
        for d in &reg.defs {
            if d.id == id {
                return d.name.as_str();
            }
        }
    }
    UNKNOWN_NAME
}

/// Selectable pet kinds, in `PETS.CFG` order.  Empty before an install and
/// when the manifest declared no usable pet.
pub fn roster() -> &'static [PetKind] {
    match current() {
        Some(reg) => reg.ids.as_slice(),
        None => &[],
    }
}

/// Whether `id` resolves to a pet that actually exists in the roster.
pub fn is_known(id: u8) -> bool {
    roster().iter().any(|k| k.0 == id)
}

/// Whether `kind` counts as hidden: dropped for having no sprites, while the
/// roster still has other pets (with none left there is nothing to move to).
pub fn is_hidden(kind: PetKind) -> bool {
    hidden_in(roster(), DROPPED.load(Ordering::Relaxed), kind)
}

pub(crate) fn hidden_in(roster: &[PetKind], dropped: u16, kind: PetKind) -> bool {
    !roster.is_empty() && kind.0 < 16 && dropped & (1 << kind.0) != 0
}

/// The pure part of [`drop_without_sprites`]: remove every pet with no sprites
/// from the selectable ids and record it in `dropped`.  The name stays in
/// `defs`.  Does nothing unless `catalogue_complete`: a flash read glitch must
/// never make a living pet look hidden.
fn drop_from(reg: &mut Registry, catalogue_complete: bool, has_sprites: impl Fn(u8) -> bool) {
    if !catalogue_complete {
        return;
    }
    let mut mask = 0u16;
    for k in reg.ids.iter().filter(|k| !has_sprites(k.0)) {
        mask |= 1 << k.0;
    }
    reg.ids.retain(|k| mask & (1 << k.0) == 0);
    reg.dropped |= mask;
}

/// Once, after the sprite catalogue: drop from the selectable roster every pet with no idle frame.
/// Its PETS.CFG name stays known to `name_of`, so Unicorn Realm records keep it.
/// Does nothing when the catalogue walk did not reach its end.
pub fn drop_without_sprites(has_sprites: impl Fn(u8) -> bool) {
    if !INSTALLED.load(Ordering::Acquire) {
        return;
    }
    // SAFETY: boot-time write before any reader of `ids`, single-threaded.
    let reg = unsafe { &mut *core::ptr::addr_of_mut!(REGISTRY) };
    drop_from(reg, super::sprite_loader::catalogue_complete(), has_sprites);
    DROPPED.store(reg.dropped, Ordering::Relaxed);
}

/// Test stand-in for the boot-time `PETS.CFG` load: ids 0, 1, 2.  `install`
/// is once-only, so calling this from each test is safe in any order.
#[cfg(test)]
pub(crate) fn install_test_roster() {
    let mut defs: heapless::Vec<PetDef, 3> = heapless::Vec::new();
    for (id, name) in [(0u8, "One"), (1, "Two"), (2, "Three")] {
        let mut n = heapless::String::new();
        let _ = n.push_str(name);
        let _ = defs.push(PetDef { id, name: n });
    }
    install(&defs);
}

/// Test hook: set the dropped mask directly.  The registry installs once per
/// process, so tests cannot reach `drop_without_sprites` with a roster that
/// holds kind 8; this marks kinds as dropped whatever the test order.
#[cfg(test)]
pub(crate) fn set_dropped_for_test(mask: u16) {
    DROPPED.store(mask, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(id: u8, name: &str) -> PetDef {
        let mut n = heapless::String::new();
        let _ = n.push_str(name);
        PetDef { id, name: n }
    }

    fn local() -> Registry {
        let mut reg = Registry { defs: heapless::Vec::new(), ids: heapless::Vec::new(), dropped: 0 };
        for (id, name) in [(0u8, "A"), (7, "B"), (8, "Jellyfish")] {
            let _ = reg.defs.push(def(id, name));
            let _ = reg.ids.push(PetKind(id));
        }
        reg
    }

    #[test]
    fn pets_without_sprites_leave_the_roster_but_keep_their_name() {
        let mut reg = local();
        drop_from(&mut reg, true, |id| id != 8);
        let ids: std::vec::Vec<u8> = reg.ids.iter().map(|k| k.0).collect();
        assert_eq!(ids, [0, 7]);
        assert_eq!(reg.dropped, 1 << 8);
        assert!(reg.defs.iter().any(|d| d.id == 8 && d.name.as_str() == "Jellyfish"));
        assert!(!reg.ids.iter().any(|k| k.0 == 8));
    }

    #[test]
    fn migrated_pets_survive_only_with_their_sprites() {
        let mut parsed: heapless::Vec<PetDef, MAX_PETS> = heapless::Vec::new();
        crate::game::pets_cfg::parse(b"5=Panda\n6=Badger\n7=Seastar\n8=Jellyfish\n", &mut parsed);
        assert!(crate::game::pets_cfg::migrate_old_format(&mut parsed));
        let mut reg = Registry { defs: heapless::Vec::new(), ids: heapless::Vec::new(), dropped: 0 };
        for d in parsed.iter() {
            let _ = reg.defs.push(d.clone());
            let _ = reg.ids.push(PetKind(d.id));
        }
        // Slug (2) and Jellyfish (8) have no sprites.
        drop_from(&mut reg, true, |id| id != 2 && id != 8);
        let ids: std::vec::Vec<u8> = reg.ids.iter().map(|k| k.0).collect();
        assert_eq!(ids, [0, 1, 5, 6, 7]);
        // With every sprite set present nothing goes.
        let mut all = Registry { defs: heapless::Vec::new(), ids: heapless::Vec::new(), dropped: 0 };
        for d in parsed.iter() {
            let _ = all.ids.push(PetKind(d.id));
        }
        drop_from(&mut all, true, |_| true);
        assert_eq!(all.ids.len(), 7);
    }

    #[test]
    fn an_incomplete_catalogue_drops_nothing() {
        let mut reg = local();
        drop_from(&mut reg, false, |_| false);
        assert_eq!(reg.ids.len(), 3);
        assert_eq!(reg.dropped, 0);
    }
}

//! Runtime pet roster.
//!
//! The three built-in pets (Bartholomeus, Cat, Slug) are always present;
//! additional pets can be installed at boot from a `PETS.CFG` manifest on the
//! USB-MSC partition — no firmware reflash.  See [`crate::fw::pets_cfg`].
//!
//! A pet is a pure cosmetic skin: a sprite-prefix byte plus a display name.
//! All game behaviour (stats, decay, traits, animations) is identical across
//! pets, so a new pet needs only its `PPAAFF.PCX` sprites at the matching
//! prefix and one manifest line.  Frame counts are auto-discovered from the
//! FAT directory catalogue, so no per-pet code is required.

use core::sync::atomic::{AtomicBool, Ordering};

use super::engine::PetKind;

/// Maximum total pets in the roster (3 built-in + custom).
pub const MAX_PETS: usize = 12;
/// Maximum display-name length (bytes).
pub const NAME_CAP: usize = 16;
/// Sprite prefixes reserved for non-pet assets: `3` sponsor slideshow,
/// `4` menu icons.  Everything else `< MAX_PET_PREFIX` is a pet.
pub const RESERVED_PREFIXES: [u8; 2] = [3, 4];

/// Exclusive upper bound on a pet's sprite prefix.  Built-ins live at
/// `0..=2`; custom pets use `5..7`.  Must stay in sync with
/// `sprite_loader::PP_MAX` so the frame catalogue covers every pet.  Kept
/// small deliberately — every extra prefix grows the catalogue table, and
/// the debug build is RAM-tight.
pub const MAX_PET_PREFIX: u8 = 8;

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
}

/// Built-in roster — used before (or instead of) an install.
static BUILTIN_IDS: [PetKind; 3] = [PetKind::Bartholomeus, PetKind::Cat, PetKind::Slug];

fn builtin_name(id: u8) -> &'static str {
    match id {
        0 => "Bartholomeus",
        1 => "Cat",
        2 => "Slug",
        _ => "Pet",
    }
}

// Single global roster, populated once at boot.  `heapless` collections are
// const-constructible, so this needs no allocator or `StaticCell` (keeping
// the module dependency-free for the simulator build).  Mirrors the
// install-once pattern in `engine::thresholds`.
static mut REGISTRY: Registry = Registry {
    defs: heapless::Vec::new(),
    ids: heapless::Vec::new(),
};
static INSTALLED: AtomicBool = AtomicBool::new(false);

fn current() -> Option<&'static Registry> {
    if INSTALLED.load(Ordering::Acquire) {
        // SAFETY: `REGISTRY` is written exactly once inside `install` (before
        // the `INSTALLED` Release store) and only ever read afterwards.  Boot
        // is single-threaded, so no concurrent access occurs.
        Some(unsafe { &*core::ptr::addr_of!(REGISTRY) })
    } else {
        None
    }
}

/// Install the roster: the three built-ins first (always present), then any
/// manifest `entries` — a built-in id renames that pet, a new valid prefix
/// appends a pet.  Reserved / out-of-range / overflow entries are dropped.
///
/// Call once at boot (see [`crate::fw::pets_cfg::load_and_install`]).  A
/// second call is ignored, so the built-in roster stays stable if the
/// manifest is absent.
pub fn install(entries: &[PetDef]) {
    if INSTALLED.load(Ordering::Acquire) {
        return; // already installed — ignore
    }

    // SAFETY: exclusive access — install runs once at boot before any reader,
    // and the `INSTALLED` guard above prevents a second populate.
    let reg = unsafe { &mut *core::ptr::addr_of_mut!(REGISTRY) };

    // Built-ins first, in canonical order.
    for &k in &BUILTIN_IDS {
        let mut name = heapless::String::new();
        let _ = name.push_str(builtin_name(k.0));
        let _ = reg.defs.push(PetDef { id: k.0, name });
        let _ = reg.ids.push(k);
    }

    // Apply manifest entries: an id matching a built-in renames it in place;
    // a new valid pet prefix is appended.  Reserved / out-of-range ids and
    // roster overflow are dropped.
    for e in entries {
        if !is_pet_prefix(e.id) {
            continue;
        }
        if let Some(d) = reg.defs.iter_mut().find(|d| d.id == e.id) {
            d.name = e.name.clone(); // rename existing (built-in or duplicate)
            continue;
        }
        if reg.defs.push(e.clone()).is_err() {
            break; // roster full
        }
        let _ = reg.ids.push(PetKind(e.id));
    }

    INSTALLED.store(true, Ordering::Release);
}

/// Display name for a pet id.  Falls back to the built-in names (and
/// `"Pet"` for an unknown id) before an install or for a missing entry.
pub fn name_of(id: u8) -> &'static str {
    if let Some(reg) = current() {
        for d in &reg.defs {
            if d.id == id {
                return d.name.as_str();
            }
        }
    }
    builtin_name(id)
}

/// Selectable pet kinds, in roster order — built-ins plus installed customs.
/// Falls back to the three built-ins before an install.
pub fn roster() -> &'static [PetKind] {
    match current() {
        Some(reg) => reg.ids.as_slice(),
        None => &BUILTIN_IDS,
    }
}

/// Whether `id` resolves to a pet that actually exists in the roster.
pub fn is_known(id: u8) -> bool {
    roster().iter().any(|k| k.0 == id)
}

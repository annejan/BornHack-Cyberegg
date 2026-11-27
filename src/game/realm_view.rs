//! Unicorn Realm — display past pets.
//!
//! A full-screen overlay showing the last 10 pets that have left.
//! Activated from the BornPets settings menu.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Text};

use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK};
use crate::{BLACK, TriColor};

const NO_PETS: &str = "No past pets yet";

/// Fixed texts of the Realm screen (host-only, for the guide's word list).
#[cfg(any(test, feature = "simulator"))]
pub fn words() -> [&'static str; 5] {
    [
        NO_PETS,
        "<name> (<kind>) - <d>d <h>h",
        "<kind> Gen <n> - <d>d <h>h",
        "V:<n>% C:<n>% R:<n>%",
        "<n>/<m>",
    ]
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static SCROLL: AtomicU8 = AtomicU8::new(0);

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    SCROLL.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

pub fn scroll_up() {
    let count = super::lifecycle::realm_count();
    let s = SCROLL.load(Ordering::Relaxed);
    SCROLL.store(ui::wrap_prev(s, count), Ordering::Relaxed);
}

pub fn scroll_down() {
    let count = super::lifecycle::realm_count();
    let s = SCROLL.load(Ordering::Relaxed);
    SCROLL.store(ui::wrap_next(s, count), Ordering::Relaxed);
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let count = super::lifecycle::realm_count();
    let scroll = SCROLL.load(Ordering::Relaxed) as usize;

    // Background.
    crate::ui::clear_screen(display)?;

    // Title bar.
    ui::draw_title_bar(display, "Unicorn Realm", Point::zero(), 152)?;

    if count == 0 {
        ui::draw_centered_message(display, NO_PETS, Point::new(76, 85))?;
        return Ok(());
    }

    // Show up to 4 pets per screen.
    let left = crate::ui::TOP_LEFT;

    let visible = 4usize.min(count as usize - scroll);
    for i in 0..visible {
        let idx = scroll + i;
        let Some(pet) = super::lifecycle::realm_pet(idx) else {
            break;
        };

        let y = 22 + i as i32 * 32;

        // Name / kind header.
        let mut line: heapless::String<28> = heapless::String::new();
        let name = pet.name_str();
        let kind_name = pet.pet_kind.name();
        if !name.is_empty() {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!("{} ({}) - {}", name, kind_name, pet.age_str()),
            );
        } else {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!("{} Gen {} - {}", kind_name, pet.generation, pet.age_str()),
            );
        }
        crate::ui::draw_text(display, line.as_str(), Point::new(4, y), TEXT_BOLD_BLACK, left)?;

        // Traits line.
        let mut traits: heapless::String<32> = heapless::String::new();
        let vit_pct = super::engine::trait_pct(pet.vitality);
        let cur_pct = super::engine::trait_pct(pet.curiosity);
        let res_pct = super::engine::trait_pct(pet.resilience);
        let _ = core::fmt::Write::write_fmt(
            &mut traits,
            format_args!("V:{}% C:{}% R:{}%", vit_pct, cur_pct, res_pct),
        );
        Text::with_text_style(traits.as_str(), Point::new(4, y + 14), TEXT_BLACK, left)
            .draw(display)?;

        // Separator.
        if i + 1 < visible {
            Rectangle::new(Point::new(4, y + 29), Size::new(144, 1))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
        }
    }

    // Scroll indicator.
    if count as usize > 4 {
        let mut indicator: heapless::String<8> = heapless::String::new();
        let _ =
            core::fmt::Write::write_fmt(&mut indicator, format_args!("{}/{}", scroll + 1, count));
        let right = crate::ui::BOTTOM_RIGHT;
        Text::with_text_style(indicator.as_str(), Point::new(148, 150), TEXT_BLACK, right)
            .draw(display)?;
    }

    Ok(())
}

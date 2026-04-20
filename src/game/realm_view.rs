//! Unicorn Realm — display past pets.
//!
//! A full-screen overlay showing the last 10 pets that have left.
//! Activated from the BornPets settings menu.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::{
    mono_font::{ascii::{FONT_7X13, FONT_7X13_BOLD}, MonoTextStyle},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Baseline, Text, TextStyleBuilder},
};

use crate::{BLACK, TriColor, WHITE};

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
    let s = SCROLL.load(Ordering::Relaxed);
    if s > 0 { SCROLL.store(s - 1, Ordering::Relaxed); }
}

pub fn scroll_down() {
    let count = super::lifecycle::realm_count();
    let s = SCROLL.load(Ordering::Relaxed);
    if s + 1 < count { SCROLL.store(s + 1, Ordering::Relaxed); }
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let count = super::lifecycle::realm_count();
    let scroll = SCROLL.load(Ordering::Relaxed) as usize;

    // Background.
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // Title bar.
    Rectangle::new(Point::zero(), Size::new(152, 18))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let title_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .build();
    Text::with_text_style(
        " Unicorn Realm",
        Point::new(2, 9),
        MonoTextStyle::new(&FONT_7X13_BOLD, WHITE),
        title_style,
    )
    .draw(display)?;

    if count == 0 {
        let centered = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(embedded_graphics::text::Alignment::Center)
            .build();
        Text::with_text_style(
            "No past pets yet",
            Point::new(76, 85),
            MonoTextStyle::new(&FONT_7X13, BLACK),
            centered,
        )
        .draw(display)?;
        return Ok(());
    }

    // Show up to 4 pets per screen.
    let font = MonoTextStyle::new(&FONT_7X13, BLACK);
    let font_bold = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();

    let visible = 4usize.min(count as usize - scroll);
    for i in 0..visible {
        let idx = scroll + i;
        let Some(pet) = super::lifecycle::realm_pet(idx) else { break };

        let y = 22 + i as i32 * 32;

        // Name / generation header.
        let mut line: heapless::String<24> = heapless::String::new();
        let name = pet.name_str();
        if !name.is_empty() {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!("{} - {}", name, pet.age_str()),
            );
        } else {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!("Gen {} - {}", pet.generation, pet.age_str()),
            );
        }
        Text::with_text_style(line.as_str(), Point::new(4, y), font_bold, left)
            .draw(display)?;

        // Traits line.
        let mut traits: heapless::String<32> = heapless::String::new();
        let vit_pct = pet.vitality as u32 * 100 / 65535;
        let cur_pct = pet.curiosity as u32 * 100 / 65535;
        let res_pct = pet.resilience as u32 * 100 / 65535;
        let _ = core::fmt::Write::write_fmt(
            &mut traits,
            format_args!("V:{}% C:{}% R:{}%", vit_pct, cur_pct, res_pct),
        );
        Text::with_text_style(traits.as_str(), Point::new(4, y + 14), font, left)
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
        let _ = core::fmt::Write::write_fmt(
            &mut indicator,
            format_args!("{}/{}", scroll + 1, count),
        );
        let right = TextStyleBuilder::new()
            .baseline(Baseline::Bottom)
            .alignment(embedded_graphics::text::Alignment::Right)
            .build();
        Text::with_text_style(indicator.as_str(), Point::new(148, 150), font, right)
            .draw(display)?;
    }

    Ok(())
}

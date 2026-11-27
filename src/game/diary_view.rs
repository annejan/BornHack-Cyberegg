//! Diary view: the last cause events, three per page, two pages.
//!
//! A full-screen overlay opened from the Stats menu. Left / Right flip the
//! page, any other button closes it. The ages change by themselves, so an
//! open diary is a live view (see `game::live_view_open`).

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;

use super::diary::{DIARY_LEN, entry_lines};
use crate::TriColor;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static PAGE: AtomicU8 = AtomicU8::new(0);

/// Entries per page.
const PER_PAGE: usize = 3;
const PAGES: u8 = DIARY_LEN.div_ceil(PER_PAGE) as u8;
/// Y of the first entry's first line, and the distance between entries.
const FIRST_ROW_Y: i32 = 22;
const ROW_PITCH: i32 = 40;
/// How far the second line sits below the first.
const LINE2_DY: i32 = 13;
const LEFT_X: i32 = 4;

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    PAGE.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
    super::note_live_view_drawn();
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

/// Show the other page (wraps); a diary of one page has no other page.
pub fn flip() {
    if super::lifecycle::diary().len() <= PER_PAGE {
        return;
    }
    let next = (PAGE.load(Ordering::Relaxed) + 1) % PAGES;
    PAGE.store(next, Ordering::Relaxed);
}

pub fn page() -> u8 {
    PAGE.load(Ordering::Relaxed)
}

/// Shown instead of the entries while the diary is empty.
const NOTHING_YET: &str = "Nothing yet";

/// Every word this screen draws (host-only, for the guide's word list).
#[cfg(any(test, feature = "simulator"))]
pub fn words() -> std::vec::Vec<std::string::String> {
    use std::string::ToString;
    std::vec![
        title(0, 0).to_string(),
        title(DIARY_LEN, 0).to_string(),
        title(DIARY_LEN, 1).to_string(),
        NOTHING_YET.to_string(),
    ]
}

/// "Diary" alone while everything fits one page, else "Diary 1/2" (the page
/// is never flipped to a second one then, see [`flip`]).
fn title(entries: usize, page: u8) -> heapless::String<12> {
    let mut title: heapless::String<12> = heapless::String::new();
    let _ = if entries <= PER_PAGE {
        core::fmt::Write::write_str(&mut title, "Diary")
    } else {
        core::fmt::Write::write_fmt(&mut title, format_args!("Diary {}/{}", page + 1, PAGES))
    };
    title
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    super::note_live_view_drawn();
    ui::clear_screen(display)?;

    let page = page();
    let diary = super::lifecycle::diary();
    ui::draw_title_bar(display, title(diary.len(), page).as_str(), Point::zero(), 152)?;
    if diary.is_empty() {
        return ui::draw_centered_message(display, NOTHING_YET, Point::new(76, 76));
    }

    let now_min = super::lifecycle::pet_age_min();
    let shown = diary
        .newest_first()
        .skip(page as usize * PER_PAGE)
        .take(PER_PAGE);
    for (i, entry) in shown.enumerate() {
        let y = FIRST_ROW_Y + ROW_PITCH * i as i32;
        let (what, why) = entry_lines(entry, now_min);
        ui::draw_text(
            display,
            what.as_str(),
            Point::new(LEFT_X, y),
            TEXT_BOLD_BLACK,
            ui::TOP_LEFT,
        )?;
        ui::draw_text(
            display,
            why.as_str(),
            Point::new(LEFT_X, y + LINE2_DY),
            TEXT_BLACK,
            ui::TOP_LEFT,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::game::engine::{GameState, PetKind, Phase};
    use crate::menu::ButtonId;

    #[test]
    fn the_title_shows_pages_only_when_there_are_two() {
        assert_eq!(super::title(0, 0).as_str(), "Diary");
        assert_eq!(super::title(3, 0).as_str(), "Diary");
        assert_eq!(super::title(4, 0).as_str(), "Diary 1/2");
        assert_eq!(super::title(6, 1).as_str(), "Diary 2/2");
    }

    #[test]
    fn diary_pages_flip_with_left_right() {
        let _guard = crate::game::lock_test_state();
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        crate::game::lifecycle::set_state_for_test(Some(s));
        for i in 0..4 {
            crate::game::lifecycle::log_cause(crate::game::engine::causes::Cause::Groggy, i);
        }
        super::open();
        assert!(super::is_active());
        assert_eq!(super::page(), 0);
        assert!(crate::game::input::dispatch(ButtonId::Right));
        assert_eq!(super::page(), 1);
        assert!(crate::game::input::dispatch(ButtonId::Right));
        assert_eq!(super::page(), 0);
        assert!(crate::game::input::dispatch(ButtonId::Left));
        assert_eq!(super::page(), 1);
        assert!(crate::game::input::dispatch(ButtonId::Fire));
        assert!(!super::is_active());
        super::open();
        assert_eq!(super::page(), 0, "reopening starts at page 1");
        super::close();
        crate::game::lifecycle::set_state_for_test(None);
    }

    #[test]
    fn a_short_diary_has_no_second_page() {
        use crate::game::engine::causes::Cause;
        let _guard = crate::game::lock_test_state();
        crate::game::lifecycle::start_new_game(PetKind(0), false, false);
        for _ in 0..3 {
            crate::game::lifecycle::log_cause(Cause::Groggy, 0);
        }
        super::open();
        super::flip();
        assert_eq!(super::page(), 0);
        crate::game::lifecycle::log_cause(Cause::Groggy, 0);
        super::flip();
        assert_eq!(super::page(), 1);
        super::close();
    }

    #[test]
    fn a_cause_behind_a_full_screen_view_is_logged_but_not_toasted() {
        use crate::game::engine::causes::Cause;
        let _guard = crate::game::lock_test_state();
        crate::game::lifecycle::start_new_game(PetKind(0), false, false);
        crate::game::lifecycle::with_state(|s| {
            s.phase = Phase::Active;
            s.causes.push(Cause::FoodComa);
            true
        });
        super::open();
        assert!(crate::game::full_screen_view_up());
        let before = crate::game::next_ui_deadline_tick();
        crate::game::lifecycle::cycle();
        assert_eq!(crate::game::lifecycle::diary().len(), 1);
        assert_eq!(crate::game::current_toast().unwrap_or(crate::game::Toast::None), crate::game::Toast::None);
        let now = crate::game::lifecycle::now_tick();
        assert!(crate::game::next_ui_deadline_tick() > now + 1);
        assert_eq!(crate::game::next_ui_deadline_tick(), before);
        super::close();
        assert!(!crate::game::full_screen_view_up());
    }

    #[test]
    fn an_open_diary_refreshes_within_an_idle_frame() {
        let _guard = crate::game::lock_test_state();
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert_eq!(crate::game::next_ui_deadline_tick(), u32::MAX);
        super::open();
        let now = crate::game::lifecycle::now_tick();
        assert!(crate::game::live_view_open());
        assert!(crate::game::next_ui_deadline_tick() <= now + 30);
        super::close();
        assert_eq!(crate::game::next_ui_deadline_tick(), u32::MAX);
        crate::game::lifecycle::set_state_for_test(None);
    }
}

//! Battle screen — pick a friend from the mesh Friends list and fight.
//!
//! Opened from the top of the Play menu. Two internal states:
//! - **Picking**: scrollable friend list (mirrors `friends_view`), Up/Down
//!   moves the cursor, Fire challenges the highlighted friend, Cancel/any
//!   other button closes the screen.
//! - **Result**: a static report card (no live turn animation — e-paper
//!   refreshes are too slow for that) showing both pets' names, HP-left
//!   bars, and a WIN/LOSE banner. Any button returns to the game screen.

use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use super::battle::BattleOutcome;
use super::engine::PET_NAME_MAX;
use crate::menu::ButtonId;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, RED, TriColor, WHITE};

const STATE_PICKING: u8 = 0;
const STATE_RESULT: u8 = 1;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STATE: AtomicU8 = AtomicU8::new(STATE_PICKING);
static CURSOR: AtomicU8 = AtomicU8::new(0);

/// Result of the most recently resolved battle, stashed for the Result
/// screen to render. `None` until a battle has actually been fought.
struct ResultDisplay {
    friend_name: [u8; PET_NAME_MAX],
    friend_name_len: u8,
    outcome: BattleOutcome,
}

struct SyncCell<T>(core::cell::UnsafeCell<T>);
unsafe impl<T> Sync for SyncCell<T> {}
impl<T> SyncCell<T> {
    const fn new(v: T) -> Self {
        Self(core::cell::UnsafeCell::new(v))
    }
    fn get(&self) -> *mut T {
        self.0.get()
    }
}

static RESULT: SyncCell<Option<ResultDisplay>> = SyncCell::new(None);

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

pub fn open() {
    STATE.store(STATE_PICKING, Ordering::Relaxed);
    CURSOR.store(0, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

fn cursor_up() {
    let c = CURSOR.load(Ordering::Relaxed);
    if c > 0 {
        CURSOR.store(c - 1, Ordering::Relaxed);
    }
}

fn cursor_down() {
    let count = super::friends::count();
    let c = CURSOR.load(Ordering::Relaxed);
    if count > 0 && c + 1 < count {
        CURSOR.store(c + 1, Ordering::Relaxed);
    }
}

/// Challenge the highlighted friend, if any. No-op (stays on the picker)
/// if there are no friends yet or no pet is active.
fn try_challenge() {
    let idx = CURSOR.load(Ordering::Relaxed) as usize;
    let Some(friend) = super::friends::get(idx) else {
        return;
    };
    let Some(outcome) = super::battle::challenge(&friend) else {
        return;
    };

    unsafe {
        *RESULT.get() = Some(ResultDisplay {
            friend_name: friend.name,
            friend_name_len: friend.name_len,
            outcome,
        });
    }
    STATE.store(STATE_RESULT, Ordering::Relaxed);
}

/// Route a button press while the Battle screen is active. Owns its own
/// input logic across the two sub-states (mirrors `pet_select`).
pub fn handle_input(btn: ButtonId) {
    if STATE.load(Ordering::Relaxed) == STATE_RESULT {
        // Any button dismisses the result and closes the whole screen.
        close();
        return;
    }

    match btn {
        ButtonId::Up => cursor_up(),
        ButtonId::Down => cursor_down(),
        ButtonId::Fire => try_challenge(),
        _ => close(),
    }
}

pub fn draw<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::zero(), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    if STATE.load(Ordering::Relaxed) == STATE_RESULT {
        draw_result(display)
    } else {
        draw_picker(display)
    }
}

fn draw_picker<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    ui::draw_title_bar(display, "Battle a Friend", Point::zero(), 152)?;

    let count = super::friends::count();
    if count == 0 {
        ui::draw_centered_message(display, "No friends to battle yet", Point::new(76, 85))?;
        return Ok(());
    }

    let cursor = CURSOR.load(Ordering::Relaxed) as usize;
    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();
    let row_h = 24i32;
    let start_y = 22;

    // Fixed 4-row page window; the visible page follows the cursor.
    const PAGE: usize = 4;
    let viewport_start = (cursor / PAGE) * PAGE;
    let visible = PAGE.min(count as usize - viewport_start);

    for i in 0..visible {
        let idx = viewport_start + i;
        let Some(friend) = super::friends::get(idx) else {
            break;
        };
        let is_selected = idx == cursor;
        let y = start_y + i as i32 * row_h;

        if is_selected {
            Rectangle::new(Point::new(2, y - 1), Size::new(148, row_h as u32 - 2))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
        }

        let name = friend.name_str();
        let kind_name = super::engine::PetKind::from_u8(friend.pet_kind).name();
        let mut line: heapless::String<28> = heapless::String::new();
        if !name.is_empty() {
            let _ =
                core::fmt::Write::write_fmt(&mut line, format_args!("{} ({})", name, kind_name));
        } else {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("{}", kind_name));
        }
        let style = if is_selected {
            TEXT_BOLD_WHITE
        } else {
            TEXT_BOLD_BLACK
        };
        Text::with_text_style(line.as_str(), Point::new(6, y + 3), style, left).draw(display)?;
    }

    let hint = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style("Fire to challenge", Point::new(76, 150), TEXT_BLACK, hint)
        .draw(display)?;

    Ok(())
}

fn draw_result<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    ui::draw_title_bar(display, "Battle Result", Point::zero(), 152)?;

    let result = unsafe { &*RESULT.get() };
    let Some(result) = result else {
        ui::draw_centered_message(display, "No result yet", Point::new(76, 85))?;
        return Ok(());
    };

    let left = TextStyleBuilder::new().baseline(Baseline::Top).build();
    let n = (result.friend_name_len as usize).min(PET_NAME_MAX);
    let friend_name = core::str::from_utf8(&result.friend_name[..n]).unwrap_or("Friend");
    let friend_label = if friend_name.is_empty() {
        "Friend"
    } else {
        friend_name
    };

    Text::with_text_style("You", Point::new(6, 26), TEXT_BOLD_BLACK, left).draw(display)?;
    super::stat_bar::draw_stat_bar(
        display,
        "HP",
        result.outcome.challenger_hp_pct,
        Point::new(6, 42),
        Point::new(30, 40),
        Size::new(116, 16),
        if result.outcome.challenger_hp_pct == 0 {
            RED
        } else {
            BLACK
        },
    )?;

    Text::with_text_style(friend_label, Point::new(6, 66), TEXT_BOLD_BLACK, left).draw(display)?;
    super::stat_bar::draw_stat_bar(
        display,
        "HP",
        result.outcome.target_hp_pct,
        Point::new(6, 82),
        Point::new(30, 80),
        Size::new(116, 16),
        if result.outcome.target_hp_pct == 0 {
            RED
        } else {
            BLACK
        },
    )?;

    let banner = if result.outcome.challenger_won {
        "YOU WON!"
    } else {
        "YOU LOST"
    };
    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let banner_color = if result.outcome.challenger_won {
        TEXT_BOLD_BLACK
    } else {
        embedded_graphics::mono_font::MonoTextStyle::new(
            &embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD,
            RED,
        )
    };
    Text::with_text_style(banner, Point::new(76, 112), banner_color, centered).draw(display)?;

    let hint = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style("Any button to close", Point::new(76, 148), TEXT_BLACK, hint)
        .draw(display)?;

    Ok(())
}

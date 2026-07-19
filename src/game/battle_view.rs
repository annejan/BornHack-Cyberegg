//! Battle screen — pick a friend from the mesh Friends list and fight.
//!
//! Opened from the top of the Play menu. Two internal states:
//! - **Picking**: scrollable friend list (mirrors `friends_view`), Up/Down
//!   moves the cursor, Fire challenges the highlighted friend, Cancel/any
//!   other button closes the screen.
//! - **Result**: a static report card (no live turn animation — e-paper
//!   refreshes are too slow for that) showing both pets' names, HP-left
//!   bars, and a WIN/LOSE banner. Any button returns to the game screen.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering};

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

/// Transient picker feedback: shown when a challenge can't proceed, so a
/// Fire press always tells the player *why* nothing happened instead of
/// silently no-op'ing. Cleared on cursor move / (re)open / a real battle.
const MSG_NONE: u8 = 0;
const MSG_COOLDOWN: u8 = 1; // Battle on cooldown; remaining secs in PICKER_MSG_SECS.
const MSG_BUSY: u8 = 2; // Pet asleep or mid-action.
const MSG_NOT_READY: u8 = 3; // Friend hasn't broadcast combat stats yet.
static PICKER_MSG: AtomicU8 = AtomicU8::new(MSG_NONE);
static PICKER_MSG_SECS: AtomicU16 = AtomicU16::new(0);

fn set_picker_msg(msg: u8, secs: u16) {
    PICKER_MSG.store(msg, Ordering::Relaxed);
    PICKER_MSG_SECS.store(secs, Ordering::Relaxed);
}

fn clear_picker_msg() {
    PICKER_MSG.store(MSG_NONE, Ordering::Relaxed);
}

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

/// The result screen (HP bars + WON/LOST + reward) needs a full tri-color
/// refresh — the red "YOU LOST" banner under-drives on the fast delta LUT,
/// and the whole-screen picker→result change ghosts otherwise.
pub fn wants_full_refresh() -> bool {
    ACTIVE.load(Ordering::Relaxed) && STATE.load(Ordering::Relaxed) == STATE_RESULT
}

/// Low 32 bits of uptime in ms (firmware); 0 on the simulator, where the
/// dismiss guard below is a no-op.
#[cfg(feature = "embassy-base")]
fn now_ms() -> u32 {
    embassy_time::Instant::now().as_millis() as u32
}
#[cfg(not(feature = "embassy-base"))]
fn now_ms() -> u32 {
    0
}

/// When the result screen was shown, so the same button action that
/// resolved the battle (or a joystick bounce right after) can't instantly
/// dismiss it before the player sees it.
static RESULT_SHOWN_MS: AtomicU32 = AtomicU32::new(0);
const RESULT_DISMISS_GUARD_MS: u32 = 600;

// ── Battle animation ─────────────────────────────────────────────────────────
//
// Full-screen two-stage result (see the battle-animation spec). Own pet on the
// left (art as-is), opponent on the right (same art mirrored in software). The
// takeover flag + timing live in `game::mod` (`battle_anim_*`); this module
// only renders a frame for the current stage.

use super::BattleStage;
use super::engine::anim_files::{BATTLE_AA_LOST, BATTLE_AA_STAND, BATTLE_AA_WON, battle_filename};

/// Left pet origin (own pet).
const ANIM_LEFT_X: i32 = 4;
/// Right pet origin (opponent, mirrored).
const ANIM_RIGHT_X: i32 = 84;
/// Top of both pet sprites.
const ANIM_PET_Y: i32 = 34;

/// Pick the pose animation code for one combatant. Stage 1 is always standing;
/// stage 2 gives the viewer's own pet the won/lost pose and the opponent the
/// opposite. `is_own` selects which side; `viewer_won` is from the viewer's
/// perspective.
fn pose_aa(stage: BattleStage, viewer_won: bool, is_own: bool) -> u8 {
    match stage {
        BattleStage::Standing => BATTLE_AA_STAND,
        BattleStage::Result => {
            let won = if is_own { viewer_won } else { !viewer_won };
            if won { BATTLE_AA_WON } else { BATTLE_AA_LOST }
        }
        // Not drawn — the takeover ends at Done.
        BattleStage::Done => BATTLE_AA_STAND,
    }
}

/// Draw the stage-2 win/lose banner (red, centred near the bottom).
fn draw_anim_banner<D>(display: &mut D, viewer_won: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;

    let text = if viewer_won { "YOU WON!" } else { "YOU LOST" };
    let ts = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    // White strip behind the banner so it reads over either pet.
    Rectangle::new(Point::new(0, 122), Size::new(152, 22))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    let style = MonoTextStyle::new(&FONT_7X13_BOLD, RED);
    Text::with_text_style(text, Point::new(76, 133), style, ts).draw(display)?;
    Ok(())
}

/// Power-bar geometry (Street-Fighter style: top-left own, top-right opponent).
const HP_BAR_Y: i32 = 4;
const HP_BAR_H: u32 = 8;
const HP_BAR_W: u32 = 66;
const HP_LEFT_X: i32 = 4;
const HP_RIGHT_X: i32 = 82;

/// Draw one power bar: a black outline filled proportionally to `pct`. The own
/// bar (left) fills from the left edge; the opponent bar (right, `anchor_right`)
/// depletes toward the centre, mirroring the fighting-game convention.
fn draw_hp_bar<D>(display: &mut D, x: i32, pct: u8, anchor_right: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::new(x, HP_BAR_Y), Size::new(HP_BAR_W, HP_BAR_H))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;
    let fw = HP_BAR_W * pct.min(100) as u32 / 100;
    if fw > 0 {
        let fx = if anchor_right {
            x + (HP_BAR_W - fw) as i32
        } else {
            x
        };
        Rectangle::new(Point::new(fx, HP_BAR_Y), Size::new(fw, HP_BAR_H))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
    }
    Ok(())
}

/// Draw both power bars for `stage`: 100/100 while standing, the outcome HP
/// once the result stage begins.
fn draw_hp_bars<D>(display: &mut D, stage: BattleStage) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let (own_hp, opp_hp) = if stage == BattleStage::Standing {
        (100, 100)
    } else {
        super::battle_anim_hp()
    };
    draw_hp_bar(display, HP_LEFT_X, own_hp, false)?;
    draw_hp_bar(display, HP_RIGHT_X, opp_hp, true)?;
    Ok(())
}

/// Firmware render of the current battle-animation frame. Blits both pets from
/// FAT12 (own left, opponent right-mirrored) and the stage-2 banner. The caller
/// clears the display and drives the refresh.
#[cfg(feature = "embassy-base")]
pub async fn render_anim(display: &mut crate::fw::epd::EpdGfx<'_>, stage: BattleStage) {
    use crate::fw::fat12;

    let (own_kind, opp_kind, viewer_won) = super::battle_anim_ctx();

    let own_name = battle_filename(own_kind, pose_aa(stage, viewer_won, true));
    if let Ok(file) = fat12::find_file(&own_name).await {
        super::sprite_loader::blit_file(display, &file, ANIM_LEFT_X, ANIM_PET_Y, false).await;
    }
    let opp_name = battle_filename(opp_kind, pose_aa(stage, viewer_won, false));
    if let Ok(file) = fat12::find_file(&opp_name).await {
        super::sprite_loader::blit_file(display, &file, ANIM_RIGHT_X, ANIM_PET_Y, true).await;
    }
    let _ = draw_hp_bars(display, stage);
    if stage == BattleStage::Result {
        let _ = draw_anim_banner(display, viewer_won);
    }
}

/// Simulator render of the current battle-animation frame (host build).
#[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
pub fn draw_anim_sim<D>(display: &mut D)
where
    D: DrawTarget<Color = TriColor>,
{
    let (own_kind, opp_kind, viewer_won) = super::battle_anim_ctx();
    let stage = super::battle_anim_stage();

    let own_name = battle_filename(own_kind, pose_aa(stage, viewer_won, true));
    super::sprite_loader::blit_pcx_sim(display, &own_name, ANIM_LEFT_X, ANIM_PET_Y, false);
    let opp_name = battle_filename(opp_kind, pose_aa(stage, viewer_won, false));
    super::sprite_loader::blit_pcx_sim(display, &opp_name, ANIM_RIGHT_X, ANIM_PET_Y, true);
    let _ = draw_hp_bars(display, stage);
    if stage == BattleStage::Result {
        let _ = draw_anim_banner(display, viewer_won);
    }
}

pub fn open() {
    STATE.store(STATE_PICKING, Ordering::Relaxed);
    CURSOR.store(0, Ordering::Relaxed);
    clear_picker_msg();
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn close() {
    ACTIVE.store(false, Ordering::Relaxed);
}

fn cursor_up() {
    clear_picker_msg();
    let c = CURSOR.load(Ordering::Relaxed);
    if c > 0 {
        CURSOR.store(c - 1, Ordering::Relaxed);
    } else {
        // Wrap to the last friend instead of doing nothing at the top.
        let count = super::friends::count();
        if count > 0 {
            CURSOR.store(count - 1, Ordering::Relaxed);
        }
    }
}

fn cursor_down() {
    clear_picker_msg();
    let count = super::friends::count();
    let c = CURSOR.load(Ordering::Relaxed);
    if count > 0 && c + 1 < count {
        CURSOR.store(c + 1, Ordering::Relaxed);
    } else if count > 0 {
        // Wrap to the top instead of doing nothing at the bottom.
        CURSOR.store(0, Ordering::Relaxed);
    }
}

/// Challenge the highlighted friend, if any. No-op (stays on the picker)
/// if there are no friends yet, no pet is active, or Battle is still on
/// cooldown. That last check is normally already enforced by the Play
/// menu (a disabled `Item::Battle` can't be activated to get here at
/// all), but re-checking it here means this screen can never trigger a
/// second challenge while already open, regardless of how it was
/// reached.
fn try_challenge() {
    let Some(stats) = super::lifecycle::cycle() else {
        return;
    };
    if !stats.can_battle {
        // Give feedback instead of silently ignoring the tap.
        if stats.cooldown_battle > 0 {
            // cooldown is in ticks (1 tick = 10 s).
            set_picker_msg(MSG_COOLDOWN, stats.cooldown_battle.saturating_mul(10));
        } else {
            set_picker_msg(MSG_BUSY, 0);
        }
        return;
    }

    let idx = CURSOR.load(Ordering::Relaxed) as usize;
    let Some(friend) = super::friends::get(idx) else {
        return;
    };
    let Some(outcome) = super::battle::challenge(&friend) else {
        // Friend hasn't broadcast combat stats yet (or no local stats).
        set_picker_msg(MSG_NOT_READY, 0);
        return;
    };

    clear_picker_msg();
    unsafe {
        *RESULT.get() = Some(ResultDisplay {
            friend_name: friend.name,
            friend_name_len: friend.name_len,
            outcome,
        });
    }
    RESULT_SHOWN_MS.store(now_ms(), Ordering::Relaxed);
    STATE.store(STATE_RESULT, Ordering::Relaxed);
}

/// Route a button press while the Battle screen is active. Owns its own
/// input logic across the two sub-states (mirrors `pet_select`).
pub fn handle_input(btn: ButtonId) {
    if STATE.load(Ordering::Relaxed) == STATE_RESULT {
        // Any button dismisses the result — but not for a brief guard
        // window after it appears, so the Fire that resolved the battle
        // (or a joystick bounce) can't flick it away before it's seen.
        let elapsed = now_ms().wrapping_sub(RESULT_SHOWN_MS.load(Ordering::Relaxed));
        if elapsed >= RESULT_DISMISS_GUARD_MS {
            close();
        }
        return;
    }

    match btn {
        ButtonId::Up => cursor_up(),
        ButtonId::Down => cursor_down(),
        // Both the joystick Fire and the Execute button confirm — players
        // reach for either, and only accepting Fire made Execute silently
        // close the screen ("no result").
        ButtonId::Fire | ButtonId::Execute => try_challenge(),
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

    // Feedback line when a previous Fire couldn't start a battle.
    match PICKER_MSG.load(Ordering::Relaxed) {
        MSG_COOLDOWN => {
            let secs = PICKER_MSG_SECS.load(Ordering::Relaxed);
            let mut m: heapless::String<24> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut m, format_args!("On cooldown {}s", secs));
            ui::draw_centered_message(display, m.as_str(), Point::new(76, 132))?;
        }
        MSG_BUSY => ui::draw_centered_message(display, "Can't battle now", Point::new(76, 132))?,
        MSG_NOT_READY => {
            ui::draw_centered_message(display, "Friend not ready", Point::new(76, 132))?
        }
        _ => {}
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

    // On a win with money mode on, show the HAX reward that was credited.
    if result.outcome.challenger_won && super::lifecycle::money_enabled() {
        let mut reward: heapless::String<16> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(
            &mut reward,
            format_args!("+{} HAX", super::engine::BATTLE_HAX_REWARD),
        );
        Text::with_text_style(reward.as_str(), Point::new(76, 130), TEXT_BOLD_BLACK, centered)
            .draw(display)?;
    }

    let hint = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style("Any button to close", Point::new(76, 148), TEXT_BLACK, hint)
        .draw(display)?;

    Ok(())
}

#[cfg(test)]
mod anim_tests {
    use super::*;

    #[test]
    fn pose_selection() {
        // Stage 1: both standing regardless of outcome/side.
        assert_eq!(pose_aa(BattleStage::Standing, true, true), BATTLE_AA_STAND);
        assert_eq!(pose_aa(BattleStage::Standing, false, false), BATTLE_AA_STAND);
        // Stage 2, viewer won: own pet WON, opponent LOST.
        assert_eq!(pose_aa(BattleStage::Result, true, true), BATTLE_AA_WON);
        assert_eq!(pose_aa(BattleStage::Result, true, false), BATTLE_AA_LOST);
        // Stage 2, viewer lost: own pet LOST, opponent WON.
        assert_eq!(pose_aa(BattleStage::Result, false, true), BATTLE_AA_LOST);
        assert_eq!(pose_aa(BattleStage::Result, false, false), BATTLE_AA_WON);
    }
}

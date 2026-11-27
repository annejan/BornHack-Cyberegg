//! Battle screens: pick a friend, the pre-battle screen, the result card.
//!
//! Opened from the top of the Play menu. Three internal states:
//! - **Picking**: scrollable friend list (mirrors `friends_view`), Up/Down
//!   moves the cursor, Fire challenges the highlighted friend, any other
//!   button closes the screen. Each row shows the chance (`~n%`, an estimate
//!   from the friend's last beacon) and the pet's own form word sits below.
//! - **Pre-battle** (`STATE_PRE`): both pets, the chance, who waits for whom.
//!   The target presses Fire to fight or Cancel to decline; the challenger
//!   waits (no cancel) and any button closes a note. It is drawn from
//!   `battle_link::view()` and redrawn only when something happens (an event
//!   or the answer timer), never on a clock: a per-second countdown would cost
//!   an e-paper refresh a second.
//! - **Result**: a static report card (no live turn animation — e-paper
//!   refreshes are too slow for that). Any button returns to the game screen.

use crate::sync::SyncCell;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_6X10;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::Text;

use super::battle::{Fight, Note, PreBattle, Role};
use super::engine::combat::{
    Decisive, Form, Snapshot, WIRE_FOCUS, WIRE_GUARD, WIRE_POWER, WIRE_SPEED, WIRE_STAMINA,
    factors, win_chance,
};
use super::engine::doses::Reason;
use super::engine::{PET_NAME_MAX, PetKind};
use super::friends::FriendRecord;
use crate::menu::ButtonId;
use crate::ui::{self, TEXT_BLACK, TEXT_BOLD_BLACK, TEXT_BOLD_WHITE};
use crate::{BLACK, RED, TriColor, WHITE};

const BANNER_WON: &str = "YOU WON!";
const BANNER_LOST: &str = "YOU LOST";
const NO_FRIENDS_1: &str = "No friends";
const NO_FRIENDS_2: &str = "to battle yet";
const MSG_NOT_READY_TEXT: &str = "Friend not ready";
const MSG_NO_RADIO_TEXT: &str = "No radio on badge";
const PICKER_HINT: &str = "Fire to challenge";
const RESULT_TITLE: &str = "Battle Result";
const NO_RESULT: &str = "No result yet";
const CLOSE_HINT: &str = "Any button to close";
const TARGET_HINT: &str = "Fire: fight Cancel: no";
const STAT_LABELS: [&str; 5] = ["Pow", "Grd", "Spd", "Sta", "Foc"];
const NOTE_DECLINED: &str = "declined";
const NOTE_NO_ANSWER: &str = "no answer";
const LEAGUE_DEMO: &str = "demo";
const LEAGUE_PLAYER: &str = "player";

/// Fixed texts of the Battle screens (host-only, for the guide's word list).
/// Every entry is a literal or a `&'static str` the engine owns.
#[cfg(any(test, feature = "simulator"))]
pub fn words() -> std::vec::Vec<&'static str> {
    let mut out = std::vec::Vec::from([
        BANNER_WON,
        BANNER_LOST,
        NO_FRIENDS_1,
        NO_FRIENDS_2,
        MSG_NOT_READY_TEXT,
        MSG_NO_RADIO_TEXT,
        PICKER_HINT,
        RESULT_TITLE,
        NO_RESULT,
        CLOSE_HINT,
        TARGET_HINT,
        NOTE_DECLINED,
        NOTE_NO_ANSWER,
        LEAGUE_DEMO,
        LEAGUE_PLAYER,
        "Battle <w>W-<l>L",
        "Demo <w>W-<l>L",
        "+<n> HAX",
        "You",
        "Friend",
        "HP",
        "<n>/<m>",
        "HP <n>",
        "<n>",
        "<name>",
        "<name> ~<n>% <age>",
        "<name> demo",
        "<name> player",
        "<n>m",
        "<n>h",
        "<n>d",
        "now",
        "<n>% to win",
        "~<n>% to win",
        "waiting for <name>",
        "<name> challenges you!",
        "challenges you!",
        "chance was <n>%",
        "+<word>",
        "-<word>",
        "You were <word>",
        "They were <word>",
        "You ran out of steam",
        "They ran out of steam",
    ]);
    out.extend(STAT_LABELS);
    out.extend(Form::ALL.map(Form::text));
    out.extend(super::engine::combat::Factor::ALL.map(super::engine::combat::Factor::text));
    out
}

const STATE_PICKING: u8 = 0;
const STATE_RESULT: u8 = 1;
const STATE_PRE: u8 = 2;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STATE: AtomicU8 = AtomicU8::new(STATE_PICKING);
static CURSOR: AtomicU8 = AtomicU8::new(0);

/// Transient picker feedback: shown when a challenge can't proceed, so a
/// Fire press always tells the player *why* nothing happened instead of
/// silently no-op'ing. Cleared on cursor move / (re)open / a real battle.
const MSG_NONE: u8 = 0;
const MSG_NOT_READY: u8 = 3; // Friend hasn't broadcast combat stats yet.
const MSG_NO_RADIO: u8 = 4; // This build has no mesh, so nobody can be challenged.
const MSG_REASON: u8 = 5; // A `Reason` (its packed kind and seconds below).
static PICKER_MSG: AtomicU8 = AtomicU8::new(MSG_NONE);
static PICKER_REASON_KIND: AtomicU8 = AtomicU8::new(0);
static PICKER_REASON_SECS: AtomicU32 = AtomicU32::new(0);

fn set_picker_msg(msg: u8) {
    PICKER_MSG.store(msg, Ordering::Relaxed);
}

/// The challenge was refused: the line shows the refusal's own text.
#[cfg_attr(all(feature = "embassy-core", not(feature = "mesh")), allow(dead_code))]
fn set_picker_reason(why: Reason) {
    let (kind, secs) = why.pack();
    PICKER_REASON_KIND.store(kind, Ordering::Relaxed);
    PICKER_REASON_SECS.store(secs, Ordering::Relaxed);
    set_picker_msg(MSG_REASON);
}

fn clear_picker_msg() {
    PICKER_MSG.store(MSG_NONE, Ordering::Relaxed);
}

/// The feedback line under the picker rows; empty when there is none.
fn picker_message() -> heapless::String<24> {
    let mut m: heapless::String<24> = heapless::String::new();
    match PICKER_MSG.load(Ordering::Relaxed) {
        MSG_NOT_READY => {
            let _ = m.push_str(MSG_NOT_READY_TEXT);
        }
        MSG_NO_RADIO => {
            let _ = m.push_str(MSG_NO_RADIO_TEXT);
        }
        MSG_REASON => {
            let why = Reason::unpack(
                PICKER_REASON_KIND.load(Ordering::Relaxed),
                PICKER_REASON_SECS.load(Ordering::Relaxed),
            );
            let _ = m.push_str(why.text().as_str());
        }
        _ => {}
    }
    m
}

/// Result of the most recently resolved battle, stashed for the Result
/// screen to render. `None` until a battle has actually been fought.
struct ResultDisplay {
    friend_name: [u8; PET_NAME_MAX],
    friend_name_len: u8,
    fight: Fight,
}

static RESULT: SyncCell<Option<ResultDisplay>> = SyncCell::new(None);

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// The result screen (HP bars + WON/LOST + reward) needs a full tri-color
/// refresh — the red BANNER_LOST banner under-drives on the fast delta LUT,
/// and the whole-screen picker→result change ghosts otherwise.
pub fn wants_full_refresh() -> bool {
    ACTIVE.load(Ordering::Relaxed) && STATE.load(Ordering::Relaxed) == STATE_RESULT
}

/// Test hook: back to the idle, closed state.
#[cfg(test)]
pub(crate) fn reset_for_test() {
    PRE_SHOT_DUE.store(false, Ordering::Relaxed);
    ACTIVE.store(false, Ordering::Relaxed);
    STATE.store(STATE_PICKING, Ordering::Relaxed);
}

/// Test hook: show the result card as a finished battle would.
#[cfg(test)]
pub(crate) fn show_result_for_test() {
    ACTIVE.store(true, Ordering::Relaxed);
    STATE.store(STATE_RESULT, Ordering::Relaxed);
}

/// When the result screen was shown, so the same button action that
/// resolved the battle (or a joystick bounce right after) can't instantly
/// dismiss it before the player sees it.
static RESULT_SHOWN_MS: AtomicU32 = AtomicU32::new(0);
const RESULT_DISMISS_GUARD_MS: u32 = 600;

/// When the pre-battle screen opened: the e-paper shows it only after its
/// redraw, so a press already in flight must not answer the challenge.
static PRE_SHOWN_MS: AtomicU32 = AtomicU32::new(0);
/// One partial redraw (0.5 to 2 s on this panel, ~1 s typical) plus a moment
/// to read the screen; the 15 s answer window leaves ample room.
const PRE_ANSWER_GUARD_MS: u32 = 1000;

/// Stamp `shown` with the current uptime: a screen just appeared.
fn mark_shown(shown: &AtomicU32) {
    shown.store(crate::platform::uptime_ms_u32(), Ordering::Relaxed);
}

/// The guard after a screen appeared (`mark_shown`) has run out.
fn guard_over(shown: &AtomicU32, guard_ms: u32) -> bool {
    crate::platform::uptime_ms_u32().wrapping_sub(shown.load(Ordering::Relaxed)) >= guard_ms
}

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
    let text = if viewer_won { BANNER_WON } else { BANNER_LOST };
    let ts = crate::ui::CENTERED;
    // White strip behind the banner so it reads over either pet.
    Rectangle::new(Point::new(0, 122), Size::new(152, 22))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;
    let style = crate::ui::TEXT_BOLD_RED;
    Text::with_text_style(text, Point::new(76, 133), style, ts).draw(display)?;
    Ok(())
}

/// Sprite edge length of the battle poses (`72x72` PCX).
const ANIM_PET_SIZE: i32 = 72;
/// Name-label baseline, in the band between the pets' feet (y=106) and the
/// stage-2 banner strip (y=122).
const ANIM_NAME_Y: i32 = 108;
/// Horizontal centre of each fighter's sprite column. The right sprite runs
/// past the 152px panel edge, so its label centres on the *visible* part.
const ANIM_LEFT_CX: i32 = ANIM_LEFT_X + ANIM_PET_SIZE / 2;
const ANIM_RIGHT_CX: i32 = (ANIM_RIGHT_X + 152) / 2;

/// Longest name that fits a 72px sprite column at 6px/char.
const ANIM_NAME_MAX: usize = 12;

/// Label both fighters under their sprites: the player's pet name on the
/// left, the opponent's on the right. Falls back to the species name when a
/// pet is unnamed, or when the opponent never beaconed us (so they aren't in
/// the friends list) — better than a blank label. Drawn in both stages.
fn draw_anim_names<D>(display: &mut D, own_kind: u8, opp_kind: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::iso_8859_1::FONT_6X10;

    let ts = crate::ui::CENTERED_TOP;
    let style = MonoTextStyle::new(&FONT_6X10, BLACK);

    let own = super::lifecycle::pet_name();
    let own = if own.is_empty() {
        super::engine::PetKind::from_u8(own_kind).name()
    } else {
        own
    };
    crate::ui::draw_text(
        display,
        crate::truncate_str(own, ANIM_NAME_MAX),
        Point::new(ANIM_LEFT_CX, ANIM_NAME_Y),
        style,
        ts,
    )?;

    let opp_buf;
    let opp = match super::friends::name_of(super::battle_anim_opp_id()) {
        Some((buf, len)) => {
            opp_buf = buf;
            super::engine::name_str(&opp_buf, len)
        }
        None => "",
    };
    let opp = if opp.is_empty() {
        super::engine::PetKind::from_u8(opp_kind).name()
    } else {
        opp
    };
    crate::ui::draw_text(
        display,
        crate::truncate_str(opp, ANIM_NAME_MAX),
        Point::new(ANIM_RIGHT_CX, ANIM_NAME_Y),
        style,
        ts,
    )?;

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
#[cfg(feature = "embassy-core")]
pub async fn render_anim<D>(display: &mut D, stage: BattleStage)
where
    D: embedded_graphics::draw_target::DrawTarget<Color = TriColor>
        + crate::epd_driver::PlaneAccess,
{
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
    let _ = draw_anim_names(display, own_kind, opp_kind);
    if stage == BattleStage::Result {
        let _ = draw_anim_banner(display, viewer_won);
    }
}

/// Simulator render of the current battle-animation frame (host build).
#[cfg(all(feature = "simulator", not(feature = "embassy-core")))]
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
    let _ = draw_anim_names(display, own_kind, opp_kind);
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

/// Challenge the highlighted friend, if any. Every `Err` shows its own
/// reason on the message line (too tired, injured, wait 5m, wrong league...),
/// so a Fire press never fails silently. The Session checks the pet's own
/// state, the league and the pair rest, in that order.
fn try_challenge() {
    #[cfg(all(feature = "embassy-core", not(feature = "mesh")))]
    {
        // No radio, so nothing could ever answer: say so, rather than the
        // "no need" the glue's plain refusal would read as.
        set_picker_msg(MSG_NO_RADIO);
        return;
    }
    #[cfg(not(all(feature = "embassy-core", not(feature = "mesh"))))]
    {
        let cursor = CURSOR.load(Ordering::Relaxed) as usize;
        if !super::friends::get(cursor).is_some_and(|f| f.has_numbers()) {
            set_picker_msg(MSG_NOT_READY);
            return;
        }
        match super::battle_link::challenge(cursor) {
            Ok(()) => clear_picker_msg(),
            Err(why) => set_picker_reason(why),
        }
    }
}

/// The pre-battle screen opens or changes (the Session has its numbers). The
/// caller (`battle_link`) wakes the display, so the e-paper redraws now.
pub fn open_pre_battle() {
    // A fresh state, so the card of an old fight never reopens.
    clear_picker_msg();
    // A challenge must be answerable wherever the player is: put the game
    // screen in front and close the views that would cover or swallow the
    // buttons (like an NFC bonus, which also jumps to the game screen).
    if super::battle_link::view().is_some_and(|p| p.role == Role::Target) {
        super::friends_view::close();
        super::diary_view::close();
        super::traits_view::close();
        crate::DISPLAY_STATE.with_mut(|s| s.set_active_screen(crate::SCREEN_GAME));
    }
    // The guard runs from the first show only: a later change of the same
    // screen (the numbers arrive) does not restart it.
    if STATE.load(Ordering::Relaxed) != STATE_PRE || !ACTIVE.load(Ordering::Relaxed) {
        mark_shown(&PRE_SHOWN_MS);
    }
    STATE.store(STATE_PRE, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Show the card of a fight that just ended against the friend named
/// `friend_name`.
pub fn show_fight(friend_name: [u8; PET_NAME_MAX], friend_name_len: u8, fight: Fight) {
    clear_picker_msg();
    unsafe {
        *RESULT.get() = Some(ResultDisplay {
            friend_name,
            friend_name_len,
            fight,
        });
    }
    mark_shown(&RESULT_SHOWN_MS);
    STATE.store(STATE_RESULT, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
}

/// Route a button press while the Battle screen is active. Owns its own
/// input logic across the two sub-states (mirrors `pet_select`).
pub fn handle_input(btn: ButtonId) {
    match STATE.load(Ordering::Relaxed) {
        STATE_RESULT => {
            // Any button dismisses the result — but not for a brief guard
            // window after it appears, so the Fire that resolved the battle
            // (or a joystick bounce) can't flick it away before it's seen.
            if guard_over(&RESULT_SHOWN_MS, RESULT_DISMISS_GUARD_MS) {
                close();
                // The card is done with: the Session forgets its finished screen.
                super::battle_link::dismiss_finished();
            }
        }
        STATE_PRE => handle_pre_input(btn),
        _ => handle_picker_input(btn),
    }
}

/// The pre-battle screen. The target answers (Fire or Execute fights, Cancel
/// declines, the rest do nothing). The challenger cannot cancel while it
/// waits; once a note is up any button closes it and the Session forgets the
/// finished screen.
fn handle_pre_input(btn: ButtonId) {
    let Some(p) = super::battle_link::view() else {
        // Nothing is pending any more (the answer window ran out): leave.
        close();
        return;
    };
    match (p.role, p.note) {
        (Role::Target, Note::Waiting) => match btn {
            // Not yet on the display: a press in flight is not an answer.
            ButtonId::Fire | ButtonId::Execute | ButtonId::Cancel
                if !guard_over(&PRE_SHOWN_MS, PRE_ANSWER_GUARD_MS) => {}
            ButtonId::Fire | ButtonId::Execute => {
                // A pet that cannot fight now says why: the Session sends the
                // refusal and the glue shows the reason.
                super::battle_link::answer(true);
            }
            // Decline is the only path that counts as a decline.
            ButtonId::Cancel => super::battle_link::answer(false),
            _ => {}
        },
        (Role::Challenger, Note::Waiting) => {}
        _ => {
            close();
            super::battle_link::dismiss_finished();
        }
    }
}

fn handle_picker_input(btn: ButtonId) {
    match btn {
        ButtonId::Up => {
            clear_picker_msg();
            super::friends::cursor_prev(&CURSOR);
        }
        ButtonId::Down => {
            clear_picker_msg();
            super::friends::cursor_next(&CURSOR);
        }
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
    crate::ui::clear_screen(display)?;

    match STATE.load(Ordering::Relaxed) {
        STATE_RESULT => draw_result(display),
        STATE_PRE => match super::battle_link::view() {
            Some(p) => draw_pre(display, &p, own_name()),
            None => Ok(()),
        },
        _ => draw_picker(display),
    }
}

/// This pet's name for the screen, else its species.
fn own_name() -> &'static str {
    let name = super::lifecycle::pet_name();
    if name.is_empty() {
        super::lifecycle::pet_kind().name()
    } else {
        name
    }
}

/// Picker title carrying the lifetime record, e.g. `Battle 3W-1L`, or
/// `Demo 3W-1L` in demo mode.
pub fn picker_title(wins: u16, losses: u16, demo: bool) -> heapless::String<21> {
    use core::fmt::Write;
    let mut title = heapless::String::new();
    let _ = write!(
        title,
        "{} {}W-{}L",
        if demo { "Demo" } else { "Battle" },
        wins,
        losses
    );
    title
}

/// The name a friend goes by: its own, else its species.
fn friend_name(f: &FriendRecord) -> &str {
    if f.name_str().is_empty() {
        PetKind::from_u8(f.pet_kind).name()
    } else {
        f.name_str()
    }
}

/// The widest a picker row may be (FONT_7X13 columns).
const PICKER_COLS: usize = 21;

/// One picker row: `<name> ~<n>% <age>`, where `~<n>%` is my chance from the
/// friend's last beacon. A friend of the other league shows `demo` or
/// `player` instead of the estimate; a friend without numbers, only the name.
/// The name is cut first.
pub fn picker_row(
    f: &FriendRecord,
    mine: &Snapshot,
    now_tick: u32,
    i_am_demo: bool,
) -> heapless::String<24> {
    use core::fmt::Write;
    let mut tail: heapless::String<12> = heapless::String::new();
    if f.has_numbers() {
        if f.demo != i_am_demo {
            let _ = write!(tail, " {}", if f.demo { LEAGUE_DEMO } else { LEAGUE_PLAYER });
        } else {
            let n = win_chance(mine, &f.estimate_snapshot());
            // One tick is 10 s, so six make a minute.
            let age = super::diary::fmt_age(now_tick.saturating_sub(f.last_seen_tick) / 6);
            let _ = write!(tail, " ~{n}% {age}");
        }
    }
    let mut row: heapless::String<24> = heapless::String::new();
    let _ = row.push_str(crate::truncate_str(friend_name(f), PICKER_COLS - tail.len()));
    let _ = row.push_str(tail.as_str());
    row
}

/// The form word under the picker rows.
pub fn foot_line(form: Form) -> &'static str {
    form.text()
}

/// `+word` / `-word` for one factor.
fn push_factor(out: &mut heapless::String<26>, f: super::engine::combat::Factor) {
    let _ = out.push(if f.helps() { '+' } else { '-' });
    let _ = out.push_str(f.text());
}

/// The pet's own factors on one line, e.g. `+well rested -hung over`; empty
/// for an average pet.
pub fn factor_line(s: &Snapshot) -> heapless::String<26> {
    let mut out = heapless::String::new();
    for f in factors(s).into_iter().flatten() {
        if !out.is_empty() {
            let _ = out.push(' ');
        }
        push_factor(&mut out, f);
    }
    out
}

/// What decided a lost or won fight: `You were sloppy`, `They ran out of
/// steam`. `why.0` is true when I was the one who lost.
pub fn decisive_line(why: (bool, Decisive)) -> heapless::String<22> {
    use core::fmt::Write;
    let who = if why.0 { "You" } else { "They" };
    let mut out = heapless::String::new();
    let _ = match why.1 {
        Decisive::Factor(f) => write!(out, "{who} were {}", f.text()),
        Decisive::OutOfSteam(_) => write!(out, "{who} ran out of steam"),
    };
    out
}

/// The chance line of the pre-battle screen: `62% to win`, `~55% to win`
/// while it is only an estimate.
pub fn chance_line(chance: u8, estimate: bool) -> heapless::String<16> {
    use core::fmt::Write;
    let mut out = heapless::String::new();
    let _ = write!(out, "{}{chance}% to win", if estimate { "~" } else { "" });
    out
}

/// `(v - lo) * 100 / (hi - lo)`, kept inside 0..=100.
fn range_pct(v: i32, lo: i32, hi: i32) -> u8 {
    ((v - lo) * 100 / (hi - lo)).clamp(0, 100) as u8
}

/// The peer's name on the pre-battle screen: its own, else its species.
fn peer_label(p: &PreBattle) -> &str {
    let name = super::engine::name_str(&p.peer_name, p.peer_name_len);
    if name.is_empty() {
        PetKind::from_u8(p.peer_kind).name()
    } else {
        name
    }
}

/// The status line (y 126): who waits for whom, or the note.
fn status_line(p: &PreBattle) -> heapless::String<32> {
    let mut out: heapless::String<32> = heapless::String::new();
    let name = peer_label(p);
    match p.note {
        Note::Waiting => match p.role {
            Role::Challenger => {
                let _ = out.push_str("waiting for ");
                let _ = out.push_str(crate::truncate_str(name, PET_NAME_MAX));
            }
            Role::Target => {
                // 25 columns fit the panel; the sentence takes 16 of them.
                const TAIL: &str = "challenges you!";
                let _ = out.push_str(crate::truncate_str(name, 25 - 1 - TAIL.len()));
                let _ = out.push(' ');
                let _ = out.push_str(TAIL);
            }
        },
        Note::Declined => {
            let _ = out.push_str(NOTE_DECLINED);
        }
        Note::Refused(why) => {
            let _ = out.push_str(why.text().as_str());
        }
        Note::NotReady => {
            let _ = out.push_str(MSG_NOT_READY_TEXT);
        }
        Note::TimedOut => {
            let _ = out.push_str(NOTE_NO_ANSWER);
        }
    }
    out
}

/// The hint line (y 140): none while the challenger waits.
fn hint_line(p: &PreBattle) -> Option<&'static str> {
    match (p.role, p.note) {
        (Role::Target, Note::Waiting) => Some(TARGET_HINT),
        (Role::Challenger, Note::Waiting) => None,
        _ => Some(CLOSE_HINT),
    }
}

/// FONT_6X10 in `color`.
fn small(color: TriColor) -> MonoTextStyle<'static, TriColor> {
    MonoTextStyle::new(&FONT_6X10, color)
}

/// Up to two factor lines for the pre-battle screen (y 86 and 97), `+word`
/// black, `-word` red, on the left or right side.
fn draw_side_factors<D>(display: &mut D, s: &Snapshot, right: bool) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let (x, ts) = if right {
        (150, ui::TOP_RIGHT)
    } else {
        (2, ui::TOP_LEFT)
    };
    for (i, f) in factors(s).into_iter().flatten().enumerate() {
        let mut line = heapless::String::new();
        push_factor(&mut line, f);
        let color = if f.helps() { BLACK } else { RED };
        ui::draw_text(display, line.as_str(), Point::new(x, 86 + 11 * i as i32), small(color), ts)?;
    }
    Ok(())
}

/// The pre-battle screen (152 x 152, text and bars only, so a partial refresh
/// serves). `own` is this pet's name.
pub fn draw_pre<D>(display: &mut D, p: &PreBattle, own: &str) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use core::fmt::Write;
    let cut = |s| crate::truncate_str(s, PET_NAME_MAX);

    ui::draw_text(display, cut(own), Point::new(2, 1), small(BLACK), ui::TOP_LEFT)?;
    ui::draw_text(display, cut(peer_label(p)), Point::new(150, 1), small(BLACK), ui::TOP_RIGHT)?;

    let rows: [(i32, i32, i32, i32); 5] = [
        (
            p.mine.power as i32,
            p.theirs.power as i32,
            WIRE_POWER.0 as i32,
            WIRE_POWER.1 as i32,
        ),
        (
            p.mine.guard as i32,
            p.theirs.guard as i32,
            WIRE_GUARD.0 as i32,
            WIRE_GUARD.1 as i32,
        ),
        (
            p.mine.speed as i32,
            p.theirs.speed as i32,
            WIRE_SPEED.0 as i32,
            WIRE_SPEED.1 as i32,
        ),
        (
            p.mine.stamina as i32,
            p.theirs.stamina as i32,
            WIRE_STAMINA.0 as i32,
            WIRE_STAMINA.1 as i32,
        ),
        (
            p.mine.focus as i32,
            p.theirs.focus as i32,
            WIRE_FOCUS.0 as i32,
            WIRE_FOCUS.1 as i32,
        ),
    ];
    for (i, (mine, theirs, lo, hi)) in rows.into_iter().enumerate() {
        let y = 14 + 12 * i as i32;
        ui::draw_text(display, STAT_LABELS[i], Point::new(76, y), small(BLACK), ui::CENTERED_TOP)?;
        super::stat_bar::draw_mirrored_bar(display, 2, y + 2, 60, range_pct(mine, lo, hi), false, BLACK)?;
        super::stat_bar::draw_mirrored_bar(display, 90, y + 2, 60, range_pct(theirs, lo, hi), true, BLACK)?;
    }

    let mut hp: heapless::String<12> = heapless::String::new();
    let _ = write!(hp, "HP {}", p.mine.hp);
    ui::draw_text(display, hp.as_str(), Point::new(2, 74), small(BLACK), ui::TOP_LEFT)?;
    hp.clear();
    let _ = write!(hp, "{}", p.theirs.hp);
    ui::draw_text(display, hp.as_str(), Point::new(150, 74), small(BLACK), ui::TOP_RIGHT)?;

    draw_side_factors(display, &p.mine, false)?;
    draw_side_factors(display, &p.theirs, true)?;

    ui::draw_text(
        display,
        chance_line(p.chance, p.estimate).as_str(),
        Point::new(76, 110),
        TEXT_BOLD_BLACK,
        ui::CENTERED_TOP,
    )?;
    ui::draw_text(display, status_line(p).as_str(), Point::new(76, 126), small(BLACK), ui::CENTERED_TOP)?;
    if let Some(hint) = hint_line(p) {
        ui::draw_text(display, hint, Point::new(76, 140), small(BLACK), ui::CENTERED_TOP)?;
    }
    Ok(())
}

/// The pre-battle screen as the challenger saw it when the ACCEPT came in,
/// held for the one refresh that shows the exact chance before the animation
/// (the Session's screen is gone by the time the display loop runs).
static PRE_SHOT: SyncCell<Option<PreBattle>> = SyncCell::new(None);
static PRE_SHOT_DUE: AtomicBool = AtomicBool::new(false);

/// Queue `p` for one refresh before the battle animation starts.
pub fn queue_pre_show(p: PreBattle) {
    unsafe { *PRE_SHOT.get() = Some(p) };
    PRE_SHOT_DUE.store(true, Ordering::Relaxed);
}

/// The animation takeover asks once: is a pre-battle frame due first?
pub fn take_pre_show() -> bool {
    PRE_SHOT_DUE.swap(false, Ordering::Relaxed)
}

/// Test hook: the queued frame, if any.
#[cfg(test)]
pub(crate) fn queued_pre_for_test() -> Option<PreBattle> {
    unsafe { *PRE_SHOT.get() }
}

/// Test hook: forget the queued frame.
#[cfg(test)]
pub(crate) fn clear_queued_pre_for_test() {
    unsafe { *PRE_SHOT.get() = None };
    PRE_SHOT_DUE.store(false, Ordering::Relaxed);
}

/// Draw the queued pre-battle frame (the caller cleared the display).
pub fn draw_queued_pre<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    match unsafe { &*PRE_SHOT.get() } {
        Some(p) => draw_pre(display, p, own_name()),
        None => Ok(()),
    }
}

fn draw_picker<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let (wins, losses) = super::lifecycle::battle_record().unwrap_or((0, 0));
    let demo = super::lifecycle::is_demo();
    ui::draw_title_bar(display, &picker_title(wins, losses, demo), Point::zero(), 152)?;

    let count = super::friends::count();
    if count == 0 {
        // Two lines — the single string is wider than the 152px panel and
        // got clipped to "o friends to battle ye".
        ui::draw_centered_message(display, NO_FRIENDS_1, Point::new(76, 78))?;
        ui::draw_centered_message(display, NO_FRIENDS_2, Point::new(76, 92))?;
        return Ok(());
    }

    let mine = super::lifecycle::snapshot()
        .unwrap_or_else(|| super::engine::combat::derive(&super::engine::combat::Body::REFERENCE));
    let now_tick = super::lifecycle::now_tick();
    let cursor = CURSOR.load(Ordering::Relaxed) as usize;
    let left = crate::ui::TOP_LEFT;
    let row_h = 20i32;
    let start_y = 21;

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
            Rectangle::new(Point::new(2, y), Size::new(148, row_h as u32 - 2))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
        }

        let line = picker_row(&friend, &mine, now_tick, demo);
        let style = if is_selected {
            TEXT_BOLD_WHITE
        } else {
            TEXT_BOLD_BLACK
        };
        // 3 + 21 columns of 7 px end at x 150, inside the highlight.
        crate::ui::draw_text(display, line.as_str(), Point::new(3, y + 3), style, left)?;
    }

    // Foot: the pet's own form word, then what is helping or hurting it.
    let form = super::engine::combat::form(&mine);
    ui::draw_text(display, foot_line(form), Point::new(76, 101), TEXT_BLACK, ui::CENTERED_TOP)?;
    draw_foot_factors(display, &mine)?;

    // Feedback line when a previous Fire couldn't start a battle.
    let msg = picker_message();
    if !msg.is_empty() {
        ui::draw_centered_message(display, msg.as_str(), Point::new(76, 131))?;
    }

    let hint = crate::ui::CENTERED_BOTTOM;
    Text::with_text_style(PICKER_HINT, Point::new(76, 150), TEXT_BLACK, hint)
        .draw(display)?;

    Ok(())
}

/// The pet's own `factor_line`, centred, hurting words in red.
fn draw_foot_factors<D>(display: &mut D, s: &Snapshot) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let width = factor_line(s).len() as i32 * 6;
    let mut x = 76 - width / 2;
    for f in factors(s).into_iter().flatten() {
        let mut part = heapless::String::new();
        push_factor(&mut part, f);
        let color = if f.helps() { BLACK } else { RED };
        ui::draw_text(
            display,
            part.as_str(),
            Point::new(x, 115),
            MonoTextStyle::new(&FONT_6X10, color),
            ui::TOP_LEFT,
        )?;
        x += (part.len() as i32 + 1) * 6;
    }
    Ok(())
}

fn draw_result<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    use core::fmt::Write;
    ui::draw_title_bar(display, RESULT_TITLE, Point::zero(), 152)?;

    let result = unsafe { &*RESULT.get() };
    let Some(result) = result else {
        ui::draw_centered_message(display, NO_RESULT, Point::new(76, 85))?;
        return Ok(());
    };

    let left = crate::ui::TOP_LEFT;
    let friend_name = super::engine::name_str(&result.friend_name, result.friend_name_len);
    let friend_label = if friend_name.is_empty() {
        "Friend"
    } else {
        friend_name
    };

    Text::with_text_style("You", Point::new(6, 20), TEXT_BOLD_BLACK, left).draw(display)?;
    super::stat_bar::draw_stat_bar(
        display,
        "HP",
        result.fight.my_hp,
        Point::new(6, 36),
        Point::new(30, 34),
        Size::new(116, 15),
        if result.fight.my_hp == 0 { RED } else { BLACK },
    )?;

    crate::ui::draw_text(
        display,
        friend_label,
        Point::new(6, 53),
        TEXT_BOLD_BLACK,
        left,
    )?;
    super::stat_bar::draw_stat_bar(
        display,
        "HP",
        result.fight.their_hp,
        Point::new(6, 69),
        Point::new(30, 67),
        Size::new(116, 15),
        if result.fight.their_hp == 0 { RED } else { BLACK },
    )?;

    let banner = if result.fight.won {
        BANNER_WON
    } else {
        BANNER_LOST
    };
    let centered = crate::ui::CENTERED;
    let banner_color = if result.fight.won {
        TEXT_BOLD_BLACK
    } else {
        crate::ui::TEXT_BOLD_RED
    };
    Text::with_text_style(banner, Point::new(76, 93), banner_color, centered).draw(display)?;

    // Show the HAX the fight paid.
    if result.fight.prize > 0 {
        let mut reward: heapless::String<16> = heapless::String::new();
        let _ = write!(reward, "+{} HAX", result.fight.prize);
        Text::with_text_style(
            reward.as_str(),
            Point::new(76, 106),
            TEXT_BOLD_BLACK,
            centered,
        )
        .draw(display)?;
    }

    // What the chance was, and what decided it.
    let mut chance: heapless::String<24> = heapless::String::new();
    let _ = write!(chance, "chance was {}%", result.fight.chance);
    ui::draw_text(display, chance.as_str(), Point::new(76, 114), small(BLACK), ui::CENTERED_TOP)?;
    if let Some(why) = result.fight.why {
        ui::draw_text(display, decisive_line(why).as_str(), Point::new(76, 125), small(BLACK), ui::CENTERED_TOP)?;
    }

    let hint = crate::ui::CENTERED_BOTTOM;
    Text::with_text_style(CLOSE_HINT, Point::new(76, 150), TEXT_BLACK, hint)
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
        assert_eq!(
            pose_aa(BattleStage::Standing, false, false),
            BATTLE_AA_STAND
        );
        // Stage 2, viewer won: own pet WON, opponent LOST.
        assert_eq!(pose_aa(BattleStage::Result, true, true), BATTLE_AA_WON);
        assert_eq!(pose_aa(BattleStage::Result, true, false), BATTLE_AA_LOST);
        // Stage 2, viewer lost: own pet LOST, opponent WON.
        assert_eq!(pose_aa(BattleStage::Result, false, true), BATTLE_AA_LOST);
        assert_eq!(pose_aa(BattleStage::Result, false, false), BATTLE_AA_WON);
    }

    #[test]
    fn name_labels_fit_the_sprite_column() {
        // Short names pass through untouched.
        assert_eq!(crate::truncate_str("Rex", ANIM_NAME_MAX), "Rex");
        // 12 chars is exactly the 72px column at 6px/char — the longest
        // species name ("Bartholomeus") must survive intact.
        assert_eq!(
            crate::truncate_str("Bartholomeus", ANIM_NAME_MAX),
            "Bartholomeus"
        );
        assert_eq!(crate::truncate_str("Bartholomeus", ANIM_NAME_MAX).len(), 12);
        // Longer names are clipped rather than overflowing the panel.
        assert_eq!(
            crate::truncate_str("AAAAAAAAAAAAAAAA", ANIM_NAME_MAX),
            "AAAAAAAAAAAA"
        );
        // Multi-byte codepoints are never split mid-sequence.
        let s = crate::truncate_str("ÆÆÆÆÆÆÆÆ", ANIM_NAME_MAX); // 8 x 2 bytes = 16
        assert!(s.len() <= 12);
        assert_eq!(s, "ÆÆÆÆÆÆ");
    }

    #[test]
    fn name_labels_clear_the_banner_and_sprites() {
        // Labels sit below the pets' feet and above the banner strip, so
        // neither the sprite nor the WON/LOST banner overlaps them.
        assert!(ANIM_NAME_Y >= ANIM_PET_Y + ANIM_PET_SIZE);
        assert!(ANIM_NAME_Y + 10 <= 122);
        // Both labels centre inside the 152px panel.
        assert!(ANIM_LEFT_CX > 0 && ANIM_LEFT_CX < 152);
        assert!(ANIM_RIGHT_CX > 0 && ANIM_RIGHT_CX < 152);
    }
}

#[cfg(test)]
mod title_tests {
    use super::picker_title;

    #[test]
    fn the_battle_picker_shows_your_record() {
        assert_eq!(picker_title(3, 1, false).as_str(), "Battle 3W-1L");
        assert_eq!(picker_title(3, 1, true).as_str(), "Demo 3W-1L");
        assert!(picker_title(u16::MAX, u16::MAX, false).len() <= 21);
    }
}

#[cfg(test)]
mod pre_tests {
    use super::*;
    use crate::game::battle::{Note, PreBattle, Role};
    use crate::game::battle_link;
    use crate::game::engine::combat::{
        Body, Decisive, Factor, Flags, Form, Snapshot, derive, win_chance, WIRE_FOCUS, WIRE_GUARD, WIRE_HP,
        WIRE_POWER, WIRE_SPEED, WIRE_STAMINA,
    };
    use crate::game::engine::doses::{Reason, Refusal};
    use crate::game::engine::{GameState, PetKind, Phase};
    use crate::game::friends::{self, FriendRecord};
    use crate::game::friends_view;
    use crate::game::test_canvas::Canvas;
    use crate::game::{lifecycle, lock_test_state};

    fn mine() -> Snapshot {
        derive(&Body::REFERENCE)
    }

    fn friend(name: &[u8], seen: u32) -> FriendRecord {
        let mut n = [0u8; PET_NAME_MAX];
        n[..name.len()].copy_from_slice(name);
        FriendRecord {
            device_id: [0xA1, 0x01],
            pet_kind: 1,
            name: n,
            name_len: name.len() as u8,
            first_seen_tick: 1,
            last_boost_tick: 1,
            last_seen_tick: seen,
            power: 20,
            guard: 10,
            speed: 30,
            hp: 50,
            stamina: 5,
            focus: 0,
            demo: false,
            v3: true,
            fought_tick: 0,
            wins: 0,
            losses: 0,
            heard_this_boot: true,
        }
    }

    fn pet() -> GameState {
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s
    }

    fn flags(fs: &[Factor]) -> Flags {
        fs.iter().fold(Flags(0), |f, &x| f.with(x))
    }

    fn pre(role: Role, note: Note, chance: u8, estimate: bool) -> PreBattle {
        let mut name = [0u8; PET_NAME_MAX];
        name.copy_from_slice(b"Bartholomeus");
        PreBattle {
            role,
            peer_id: [0xB0, 0x01],
            peer_kind: 1,
            peer_name: name,
            peer_name_len: 12,
            mine: mine(),
            theirs: mine(),
            chance,
            estimate,
            ends_ms: 0,
            note,
        }
    }

    #[test]
    fn picker_row_marks_the_estimate() {
        let f = friend(b"Bram", 1);
        let row = picker_row(&f, &mine(), 1 + 48, false);
        let n = win_chance(&mine(), &f.estimate_snapshot());
        assert!(row.starts_with("Bram ~"));
        assert_eq!(row.as_str(), format!("Bram ~{n}% 8m"));
    }

    #[test]
    fn picker_row_shows_the_league() {
        let mut f = friend(b"Bram", 1);
        f.demo = true;
        assert_eq!(picker_row(&f, &mine(), 49, false).as_str(), "Bram demo");
        f.demo = false;
        assert_eq!(picker_row(&f, &mine(), 49, true).as_str(), "Bram player");
        f.hp = 0;
        assert_eq!(picker_row(&f, &mine(), 49, false).as_str(), "Bram");
    }

    #[test]
    fn picker_rows_fit() {
        let mut f = friend(b"Bartholomeus", 1);
        f.power = 70;
        f.guard = 0;
        f.hp = 15;
        let strong = derive(&Body {
            fit: 100,
            ..Body::REFERENCE
        });
        for me in [mine(), strong] {
            let row = picker_row(&f, &me, 1 + 6 * 1440 * 99, false);
            assert!(row.len() <= 21, "{row}");
            assert!(row.ends_with("99d"));
        }
        f.demo = true;
        assert!(picker_row(&f, &mine(), 49, false).len() <= 21);
        // A 100 % row cuts the name, never the numbers.
        let mut hi = friend(b"Bartholomeus", 1);
        hi.power = 5;
        hi.guard = 40;
        hi.speed = 5;
        hi.hp = 15;
        let row = picker_row(&hi, &mine(), 1 + 6 * 1440 * 99, false);
        assert!(row.len() <= 21, "{row}");
        assert_eq!(win_chance(&mine(), &hi.estimate_snapshot()), 100, "the worst case");
        assert!(row.contains("~100%"), "{row}");
    }

    #[test]
    fn factor_line_fits() {
        let all = Factor::ALL;
        for &a in &all {
            for &b in &all {
                let s = Snapshot {
                    flags: flags(&[a, b]),
                    ..mine()
                };
                assert!(factor_line(&s).len() <= 25, "{a:?} {b:?}");
            }
        }
        let calm = Snapshot {
            flags: Flags(0),
            ..mine()
        };
        assert!(factor_line(&calm).is_empty());
        let s = Snapshot {
            flags: flags(&[Factor::Rested, Factor::HungOver]),
            ..mine()
        };
        assert_eq!(factor_line(&s).as_str(), "+well rested -hung over");
    }

    #[test]
    fn decisive_lines_fit() {
        let mut all: Vec<Decisive> = Factor::ALL.iter().map(|&f| Decisive::Factor(f)).collect();
        all.push(Decisive::OutOfSteam(10));
        for d in all {
            for lost in [true, false] {
                let l = decisive_line((lost, d));
                assert!(!l.is_empty() && l.len() <= 21, "{d:?} {l}");
            }
        }
        assert_eq!(
            decisive_line((true, Decisive::Factor(Factor::Sloppy))).as_str(),
            "You were sloppy"
        );
        assert_eq!(
            decisive_line((false, Decisive::OutOfSteam(4))).as_str(),
            "They ran out of steam"
        );
    }

    #[test]
    fn foot_line_is_the_form_word() {
        assert_eq!(foot_line(Form::Top), "top form");
        assert_eq!(foot_line(Form::Poor), "poor form");
    }

    #[test]
    fn pre_battle_lines_fit() {
        let c = pre(Role::Challenger, Note::Waiting, 62, false);
        let t = pre(Role::Target, Note::Waiting, 62, false);
        assert_eq!(status_line(&c).as_str(), "waiting for Bartholomeus");
        assert!(status_line(&c).len() <= 25);
        assert!(status_line(&t).len() <= 25, "{}", status_line(&t));
        assert!(status_line(&t).ends_with(" challenges you!"));
        assert!(TARGET_HINT.len() <= 25 && CLOSE_HINT.len() <= 25);
        for note in [
            Note::Declined,
            Note::NotReady,
            Note::TimedOut,
            Note::Refused(Reason::Wait(72 * 3600)),
            Note::Refused(Reason::Refused(Refusal::DemoOnly)),
            Note::Refused(Reason::Refused(Refusal::TooDrunk)),
        ] {
            let l = status_line(&pre(Role::Challenger, note, 50, false));
            assert!(!l.is_empty() && l.len() <= 25, "{note:?} {l}");
        }
        assert_eq!(chance_line(62, false).as_str(), "62% to win");
        assert_eq!(chance_line(55, true).as_str(), "~55% to win");
    }

    /// A 152 x 152 target that counts pixels drawn outside the panel.
    struct Probe {
        outside: u32,
        inside: u32,
    }

    impl OriginDimensions for Probe {
        fn size(&self) -> Size {
            Size::new(152, 152)
        }
    }

    impl DrawTarget for Probe {
        type Color = TriColor;
        type Error = core::convert::Infallible;
        fn draw_iter<I: IntoIterator<Item = Pixel<TriColor>>>(
            &mut self,
            px: I,
        ) -> Result<(), Self::Error> {
            for Pixel(p, _) in px {
                if (0..152).contains(&p.x) && (0..152).contains(&p.y) {
                    self.inside += 1;
                } else {
                    self.outside += 1;
                }
            }
            Ok(())
        }
    }

    #[test]
    fn pre_battle_draws_inside_the_panel() {
        let lo = Snapshot {
            power: WIRE_POWER.0,
            guard: WIRE_GUARD.0,
            speed: WIRE_SPEED.0,
            hp: WIRE_HP.0,
            stamina: WIRE_STAMINA.0,
            focus: WIRE_FOCUS.0,
            flags: flags(&[Factor::Rested, Factor::HungOver]),
        };
        let hi = Snapshot {
            power: WIRE_POWER.1,
            guard: WIRE_GUARD.1,
            speed: WIRE_SPEED.1,
            hp: WIRE_HP.1,
            stamina: WIRE_STAMINA.1,
            focus: WIRE_FOCUS.1,
            flags: flags(&[Factor::Waterlogged, Factor::Injured]),
        };
        for (role, note, chance, est) in [
            (Role::Challenger, Note::Waiting, 0, true),
            (Role::Target, Note::Waiting, 100, false),
            (Role::Challenger, Note::Refused(Reason::Wait(72 * 3600)), 50, false),
        ] {
            let mut p = pre(role, note, chance, est);
            p.mine = lo;
            p.theirs = hi;
            let mut probe = Probe { outside: 0, inside: 0 };
            draw_pre(&mut probe, &p, "Bartholomeus").unwrap();
            assert_eq!(probe.outside, 0, "{role:?} {note:?}");
            assert!(probe.inside > 300);
            p.mine = hi;
            p.theirs = lo;
            let mut probe = Probe { outside: 0, inside: 0 };
            draw_pre(&mut probe, &p, "Bartholomeus").unwrap();
            assert_eq!(probe.outside, 0);
        }
    }

    #[test]
    fn the_bars_follow_the_wire_ranges() {
        assert_eq!(range_pct(WIRE_POWER.0 as i32, WIRE_POWER.0 as i32, WIRE_POWER.1 as i32), 0);
        assert_eq!(range_pct(WIRE_POWER.1 as i32, WIRE_POWER.0 as i32, WIRE_POWER.1 as i32), 100);
        assert_eq!(range_pct(-100, WIRE_FOCUS.0 as i32, WIRE_FOCUS.1 as i32), 0);
        assert_eq!(range_pct(20, WIRE_FOCUS.0 as i32, WIRE_FOCUS.1 as i32), 100);
        assert_eq!(range_pct(500, 0, 10), 100, "clamped");
    }

    #[test]
    fn result_card_shows_what_was_paid() {
        let _guard = lock_test_state();
        let fight = |prize| Fight {
            peer_id: [0xB0, 0x01],
            peer_kind: 1,
            won: true,
            my_hp: 60,
            their_hp: 0,
            prize,
            chance: 62,
            why: Some((false, Decisive::OutOfSteam(4))),
            live: true,
        };
        let band = Rectangle::new(Point::new(0, 99), Size::new(152, 15));
        let mut name = [0u8; PET_NAME_MAX];
        name[..4].copy_from_slice(b"Bram");
        show_fight(name, 4, fight(20));
        let mut c = Canvas::new();
        draw(&mut c).unwrap();
        assert!(c.any(band, BLACK), "+20 HAX is drawn");
        show_fight(name, 4, fight(0));
        let mut c = Canvas::new();
        draw(&mut c).unwrap();
        assert!(!c.any(band, BLACK), "no HAX line for a prize of 0");
        // The chance and the decisive line are there.
        assert!(c.any(Rectangle::new(Point::new(0, 114), Size::new(152, 10)), BLACK));
        assert!(c.any(Rectangle::new(Point::new(0, 125), Size::new(152, 10)), BLACK));
        reset_for_test();
    }

    #[test]
    fn a_refusal_shows_its_own_reason() {
        let _guard = lock_test_state();
        reset_for_test();
        set_picker_reason(Reason::Refused(Refusal::Injured));
        assert_eq!(picker_message().as_str(), "injured");
        set_picker_reason(Reason::Refused(Refusal::Tired));
        assert_eq!(picker_message().as_str(), "too tired");
        set_picker_reason(Reason::Wait(120));
        assert_eq!(picker_message().as_str(), "wait 2m");
        set_picker_msg(MSG_NOT_READY);
        assert_eq!(picker_message().as_str(), "Friend not ready");
        clear_picker_msg();
        assert!(picker_message().is_empty());
        assert!(!words().contains(&"Can't battle now"));
    }

    /// Moves the fake clock past the pre-battle guard; back to the real clock
    /// when dropped.
    struct ClockReset;
    impl Drop for ClockReset {
        fn drop(&mut self) {
            crate::platform::set_fake_uptime_ms(u64::MAX);
        }
    }
    fn pre_guard_over() -> ClockReset {
        let now = crate::platform::uptime_ms();
        crate::platform::set_fake_uptime_ms(now + PRE_ANSWER_GUARD_MS as u64);
        ClockReset
    }

    /// A press already in flight when a challenge lands is not an answer:
    /// the e-paper has not shown the screen yet.
    #[test]
    fn the_pre_battle_screen_ignores_presses_until_it_is_shown() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        crate::platform::set_fake_uptime_ms(10_000);
        battle_link::inject_challenge_for_shot("Bram", 1);
        for b in [ButtonId::Fire, ButtonId::Execute, ButtonId::Cancel] {
            crate::platform::set_fake_uptime_ms(10_000 + PRE_ANSWER_GUARD_MS as u64 - 1);
            handle_input(b);
            assert!(battle_link::view().is_some() && is_active(), "a press answered early");
        }
        assert!(battle_link::last_fight().is_none());
        // Once the guard is over, Fire fights.
        crate::platform::set_fake_uptime_ms(10_000 + PRE_ANSWER_GUARD_MS as u64);
        handle_input(ButtonId::Fire);
        assert!(battle_link::last_fight().is_some());
        crate::platform::set_fake_uptime_ms(u64::MAX);
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }

    #[test]
    fn the_target_answers_with_fire_and_cancel() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        battle_link::inject_challenge_for_shot("Bram", 1);
        let _clock = pre_guard_over();
        assert!(is_active() && STATE.load(Ordering::Relaxed) == STATE_PRE);
        // Other buttons do nothing.
        handle_input(ButtonId::Left);
        assert!(battle_link::view().is_some() && is_active());
        // Cancel declines: the screen closes, no fight.
        handle_input(ButtonId::Cancel);
        assert!(battle_link::view().is_none() && !is_active());
        assert!(battle_link::last_fight().is_none());
        // Fire fights.
        battle_link::inject_challenge_for_shot("Bram", 1);
        let _clock = pre_guard_over();
        handle_input(ButtonId::Fire);
        assert!(battle_link::last_fight().is_some());
        assert!(wants_full_refresh(), "the card is up");
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }

    #[test]
    fn a_note_closes_on_any_button_and_forgets_the_pending() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        let mut f = friend(b"Bram", 1);
        f.v3 = false;
        friends::insert_for_test(f);
        assert!(battle_link::challenge(0).is_ok());
        assert!(is_active());
        assert_eq!(battle_link::view().unwrap().note, Note::NotReady);
        handle_input(ButtonId::Left);
        assert!(!is_active());
        assert!(battle_link::view().is_none(), "the resolved pending is gone");
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }

    #[test]
    fn a_waiting_challenger_ignores_every_button() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        friends::insert_for_test(friend(b"Bram", 1));
        assert!(battle_link::challenge(0).is_ok());
        for b in [ButtonId::Cancel, ButtonId::Fire, ButtonId::Left] {
            handle_input(b);
            assert!(is_active() && battle_link::view().is_some());
        }
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }

    #[test]
    fn the_exact_chance_is_shown_once_before_the_animation() {
        let _guard = lock_test_state();
        reset_for_test();
        assert!(!take_pre_show(), "nothing queued");
        queue_pre_show(pre(Role::Challenger, Note::Waiting, 61, false));
        assert!(take_pre_show());
        let mut probe = Probe { outside: 0, inside: 0 };
        draw_queued_pre(&mut probe).unwrap();
        assert!(probe.inside > 300);
        assert!(!take_pre_show(), "taken once");
    }

    #[test]
    fn a_challenge_pulls_the_player_to_the_pre_battle_screen() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        for open in [
            friends_view::open as fn(),
            super::super::diary_view::open,
            super::super::traits_view::open,
        ] {
            open();
            crate::DISPLAY_STATE.with_mut(|s| s.set_active_screen(crate::SCREEN_MAIN));
            battle_link::inject_challenge_for_shot("Bram", 1);
            let _clock = pre_guard_over();
        let _clock = pre_guard_over();
            assert!(is_active() && STATE.load(Ordering::Relaxed) == STATE_PRE);
            assert!(!friends_view::is_active());
            assert!(!super::super::diary_view::is_active());
            assert!(!super::super::traits_view::is_active());
            assert_eq!(
                crate::DISPLAY_STATE.with(|s| s.active_screen()),
                crate::SCREEN_GAME
            );
            // The press reaches the pre-battle screen (not a hidden one).
            crate::menu::dispatch_press(ButtonId::Cancel);
            assert!(battle_link::view().is_none() && !is_active());
        }
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }

    #[test]
    fn the_pre_battle_screen_outranks_the_other_views_for_input() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        battle_link::inject_challenge_for_shot("Bram", 1);
        let _clock = pre_guard_over();
        // Even if one of them is somehow open, Fire/Cancel go to the battle.
        super::super::traits_view::open();
        assert!(crate::game::input::dispatch(ButtonId::Cancel));
        assert!(battle_link::view().is_none(), "declined, not a traits close");
        super::super::traits_view::close();
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }

    #[test]
    fn a_refused_fire_toasts_the_reason() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        battle_link::inject_challenge_for_shot("Bram", 1);
        let _clock = pre_guard_over();
        lifecycle::with_state(|s| {
            s.injured_min = 60;
            true
        });
        handle_input(ButtonId::Fire);
        assert!(!is_active());
        assert_eq!(
            crate::game::reason_toast(),
            Some(Reason::Refused(Refusal::Injured))
        );
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }

    #[test]
    fn six_pixel_text_centres_and_right_aligns_through_draw_text() {
        use embedded_graphics::text::Text;
        let style = MonoTextStyle::new(&FONT_6X10, BLACK);
        let same = |a: &Canvas, b: &Canvas| {
            (0..152).all(|y| (0..152).all(|x| a.get(x, y) == b.get(x, y)))
        };
        // "HHHHH" is 30 px wide: centred at 76 it starts at 61.
        let mut got = Canvas::new();
        ui::draw_text(&mut got, "HHHHH", Point::new(76, 10), style, ui::CENTERED_TOP).unwrap();
        let mut want = Canvas::new();
        Text::with_text_style("HHHHH", Point::new(61, 10), style, ui::TOP_LEFT)
            .draw(&mut want)
            .unwrap();
        assert!(same(&got, &want), "centred");
        // Right-aligned at 150 it starts at 120.
        let mut got = Canvas::new();
        ui::draw_text(&mut got, "HHHHH", Point::new(150, 10), style, ui::TOP_RIGHT).unwrap();
        let mut want = Canvas::new();
        Text::with_text_style("HHHHH", Point::new(120, 10), style, ui::TOP_LEFT)
            .draw(&mut want)
            .unwrap();
        assert!(same(&got, &want), "right");
    }

    #[test]
    fn the_picker_layout_keeps_its_bands_apart() {
        let _guard = lock_test_state();
        battle_link::reset_for_test();
        lifecycle::set_state_for_test(Some(pet()));
        for i in 0..4u8 {
            let mut f = friend(b"Bartholomeus", 1);
            f.device_id = [0xC0, i];
            friends::insert_for_test(f);
        }
        open();
        set_picker_reason(Reason::Wait(120));
        let mut c = Canvas::new();
        draw(&mut c).unwrap();
        let band = |y0: i32, y1: i32, x0: i32, x1: i32| {
            Rectangle::new(Point::new(x0, y0), Size::new((x1 - x0) as u32, (y1 - y0) as u32))
        };
        // Rows are 20 px: the selected one is a 148 px highlight from y 21.
        assert_eq!(c.get(2, 22), BLACK);
        assert_eq!(c.get(149, 22), BLACK);
        assert_eq!(c.get(150, 22), WHITE, "the highlight ends at x 149");
        assert_eq!(c.get(5, 40), WHITE, "the next row starts clear of it");
        // Its text starts at x 3 and stays inside the highlight: white ink.
        assert!(c.any(band(24, 36, 3, 12), WHITE));
        assert!(c.any(band(24, 36, 100, 149), WHITE));
        // Nothing is drawn right of the highlight in the rows' area.
        assert!(!c.any(band(21, 100, 150, 152), BLACK));
        // Form word, message and hint have their own bands.
        assert!(c.any(band(101, 114, 0, 152), BLACK), "form word");
        assert!(c.any(band(125, 137, 0, 152), BLACK), "message line");
        assert!(c.any(band(138, 151, 0, 152), BLACK), "hint");
        // Between the last row and the form word, and under the hint: clear.
        assert!(!c.any(band(100, 101, 0, 152), BLACK));
        reset_for_test();
        lifecycle::set_state_for_test(None);
        battle_link::reset_for_test();
    }
}

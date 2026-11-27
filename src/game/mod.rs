//! BornPets — game screen renderer.
//!
//! Layout (152×152 EPD, black & white + red):
//!
//! ```text
//! ┌───────────────────────────────────────┐  y =  0
//! │  [Stats] [Hibernate]  (empty)(empty)  │  y = 0–34   top icon row
//! ├───────────────────────────────────────┤  y = 34
//! │                                       │
//! │            [pet / egg]                │  y = 35–110  pet area
//! │                                       │
//! ├───────────────────────────────────────┤  y = 111
//! │  [Feed]  [Heal]  [Play]  [Rest]       │  y = 111–152 bottom icon row
//! └───────────────────────────────────────┘  y = 152
//! ```
//!
//! The status line (and the demo note / help line) straddles the bottom
//! separator: its last line is a 13-row white backing on rows 105–117, the
//! separator row 111 in the middle, so the text interrupts the line. Extra
//! lines stack upward. The bottom icon cells start at row 118, clear of it.

pub mod background;
pub mod battle;
pub mod battle_link;
pub mod battle_view;
pub mod blackhole;
pub mod bornjeweled;
pub mod debug_cheats;
pub mod diary;
pub mod diary_view;
pub mod engine;
pub mod friends;
pub mod friends_view;
pub mod input;
pub mod lifecycle;
pub mod lightsout;
pub mod minigame;
pub mod modal;
pub mod nav;
pub mod bornpets_cfg;
pub mod pacing;
pub mod pet_registry;
pub mod pets_cfg;
pub mod pet_select;
pub mod realm_picture;
pub mod realm_view;
pub mod settings;
pub mod sprite_loader;
pub mod stat_bar;
pub mod stats_pages;
pub mod station;
pub mod tictactoe;
#[cfg(test)]
mod test_canvas;
pub mod timed_flag;
pub mod traits_view;
// ── Action feedback toast ────────────────────────────────────────────────────
use core::sync::atomic::{AtomicU8, AtomicU16, AtomicU32, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
pub use nav::{GameNav, Row};

use crate::{BLACK, TriColor};
use timed_flag::TimedFlag;

/// Action feedback shown briefly after an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Toast {
    None = 0,
    Feed = 1,
    Heal = 2,
    Sleep = 3,
    Relax = 4,
    Play = 5,
    Inspired = 6,
    Hibernate = 7,
    Wake = 8,
    StationFood = 9,
    StationDrugs = 10,
    StationInspire = 11,
    StationRest = 12,
    /// Station tap was rejected because the matching effect is still
    /// on cooldown.  The remaining seconds are read from
    /// `STATION_COOLDOWN_SECS` and formatted at draw time.
    StationCooldown = 13,
    /// Took Ozempic — weight + hunger relief, distinct from Exercise.
    /// Reuses discriminant 14 (freed when Triple Born was removed).
    Ozempic = 14,
    Exercise = 15,
    Medicate = 16,
    DebugCheat = 17,
    /// Drank something alcoholic (Beer/Wine/Whiskey) — raises `drunk`.
    Drink = 18,
    Rehab = 19,
    NewFriend = 20,
    FriendReunion = 21,
    /// Shown when we were the *target* of a friend's mesh Battle
    /// challenge and it resolved in our favor — see `game::battle`.
    BattleWon = 22,
    BattleLost = 23,
    /// Drank something non-alcoholic (Water/Coffee) — never touches
    /// `drunk`, unlike `Drink`. Kept as its own variant so the toast
    /// after picking Water/Coffee doesn't misleadingly say "+drunk".
    Refreshed = 24,
    /// Sent the pet to "Only pets" — see `game::engine::Action::OnlyPets`.
    OnlyPets = 25,
    /// Won (or drew) a mini-game — see `GameState::award_inspiration`.
    /// Neutral flavor toast; replaced the old `Inspired` ("+inspired")
    /// toast when the `drained`/inspired stat was removed.
    MinigameWin = 26,
    /// Why an action was refused or a row is disabled; the text comes from
    /// the stored `engine::doses::Reason` (see `show_reason`).
    Reason = 27,
    /// Started a wash — see `engine::Action::Wash`.
    Wash = 28,
    /// Debug menu "Engine info": two lines read from the engine at draw time.
    DebugInfo = 29,
    /// Why something went badly: two lines (what, why) from `Cause`, read
    /// from `CAUSE_CODE` (see `show_cause`).
    Cause = 30,
    /// Finished and lost a mini-game — still pays (see `GameState::award_inspiration`).
    MinigameTry = 31,
}

impl Toast {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Feed,
            2 => Self::Heal,
            3 => Self::Sleep,
            4 => Self::Relax,
            5 => Self::Play,
            6 => Self::Inspired,
            7 => Self::Hibernate,
            8 => Self::Wake,
            9 => Self::StationFood,
            10 => Self::StationDrugs,
            11 => Self::StationInspire,
            12 => Self::StationRest,
            13 => Self::StationCooldown,
            14 => Self::Ozempic,
            15 => Self::Exercise,
            16 => Self::Medicate,
            17 => Self::DebugCheat,
            18 => Self::Drink,
            19 => Self::Rehab,
            20 => Self::NewFriend,
            21 => Self::FriendReunion,
            22 => Self::BattleWon,
            23 => Self::BattleLost,
            24 => Self::Refreshed,
            25 => Self::OnlyPets,
            26 => Self::MinigameWin,
            27 => Self::Reason,
            28 => Self::Wash,
            29 => Self::DebugInfo,
            30 => Self::Cause,
            31 => Self::MinigameTry,
            _ => Self::None,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Toast::None => "",
            Toast::Feed => "+full",
            Toast::Heal => "healing",
            Toast::Sleep => "sleeping",
            Toast::Relax => "calmer +energy",
            Toast::Wash => "cleaner",
            Toast::Play => "+fun",
            Toast::Inspired => "+inspired",
            Toast::Hibernate => "hibernating",
            Toast::Wake => "waking up",
            Toast::StationFood => "Food bonus!",
            Toast::StationDrugs => "Heal bonus!",
            Toast::StationInspire => "Inspire bonus!",
            Toast::StationRest => "Sleep bonus!",
            // Dynamic — handled in the renderer.
            Toast::StationCooldown => "",
            Toast::Ozempic => "-weight",
            Toast::Exercise => "-weight",
            Toast::Medicate => "+medicated",
            Toast::DebugCheat => "cheat applied",
            Toast::Drink => "+drunk",
            Toast::Rehab => "rehab started",
            Toast::NewFriend => "new friend!",
            Toast::FriendReunion => "+happy",
            Toast::BattleWon => "won a battle!",
            Toast::BattleLost => "lost a battle",
            Toast::Refreshed => "refreshed!",
            Toast::OnlyPets => "Only pets!",
            Toast::MinigameWin => "nice game!",
            Toast::MinigameTry => "good try!",
            // Dynamic — handled in the renderer.
            Toast::Reason => "",
            // Dynamic — handled in the renderer.
            Toast::DebugInfo => "",
            // Two lines, drawn by `draw_cause_toast`.
            Toast::Cause => "",
        }
    }
}

/// Toast message index.
static TOAST_MSG: AtomicU8 = AtomicU8::new(0);
/// Whether a toast is currently visible.  Cleared on the first
/// render that occurs at or after `TOAST_MIN_VISIBLE_MS` since it was shown.
static TOAST: TimedFlag = TimedFlag::new();

/// The reason shown by [`Toast::Reason`]: `Reason::pack` kind and seconds.
static REASON_KIND: AtomicU8 = AtomicU8::new(0);
static REASON_SECS: AtomicU32 = AtomicU32::new(0);

/// The cause shown by [`Toast::Cause`]: `Cause::pack`.
static CAUSE_CODE: AtomicU8 = AtomicU8::new(0);

/// Remaining cooldown in seconds, read by the renderer when the
/// active toast is [`Toast::StationCooldown`].  Set by
/// [`show_station_cooldown`].
static STATION_COOLDOWN_SECS: AtomicU16 = AtomicU16::new(0);

/// Minimum wall-clock visibility for a toast.  After this elapses the
/// toast disappears on the next display refresh.  E-paper refreshes
/// (fast LUT or full) take roughly 0.5–3 s each, so the actual
/// on-screen time is `max(TOAST_MIN_VISIBLE_MS, time-to-next-refresh)`.
const TOAST_MIN_VISIBLE_MS: u32 = 2000;

/// Whether the full-screen "now diabetic" alert is currently taking
/// over the display.  Set by [`show_diabetes_alert`], cleared once
/// `DIABETES_ALERT_MIN_VISIBLE_MS` has elapsed since it started — same
/// start-timestamp-plus-minimum-duration pattern as the toast above,
/// just occupying the whole screen instead of one line.
static DIABETES_ALERT: TimedFlag = TimedFlag::new();
/// Minimum wall-clock visibility for the diabetes alert screen.
pub const DIABETES_ALERT_MIN_VISIBLE_MS: u32 = 3000;

/// Whether the diabetes alert is up (host-only, for the guide's shot check).
#[cfg(any(test, feature = "simulator"))]
pub fn diabetes_alert_active() -> bool {
    DIABETES_ALERT.is_active()
}

/// Show the full-screen "now diabetic" alert, taking over the whole
/// display (icons, pet, everything) for at least
/// `DIABETES_ALERT_MIN_VISIBLE_MS`. Called once, the instant diabetes
/// triggers — see `lifecycle::check_diabetes_onset`.
pub fn show_diabetes_alert() {
    DIABETES_ALERT.start();
    #[cfg(feature = "embassy-core")]
    crate::TOAST_SIGNAL.signal(());
}

/// Full-screen "now alcoholic" alert — the alcoholism twin of the
/// diabetes onset takeover above, reusing `DIABETES_ALERT_MIN_VISIBLE_MS`.
static ALCOHOLISM_ALERT: TimedFlag = TimedFlag::new();

/// Show the full-screen "now alcoholic" alert, fired once the instant
/// alcoholism triggers — see `lifecycle::check_alcoholism_onset`.
pub fn show_alcoholism_alert() {
    ALCOHOLISM_ALERT.start();
    #[cfg(feature = "embassy-core")]
    crate::TOAST_SIGNAL.signal(());
}

// ── Battle animation takeover ────────────────────────────────────────────────
//
// A full-screen two-stage battle result (see the battle-animation spec). Unlike
// the alerts above it takes over the *whole* display regardless of the active
// screen, so the main display loop checks `battle_anim_active()` before drawing
// its normal screen. Two stages of `BATTLE_STAGE_MS` each: standing, then the
// won/lost poses.

/// Duration of one battle-animation stage.
const BATTLE_STAGE_MS: u32 = 5_000;
/// Total animation length — both stages back to back.
const BATTLE_TOTAL_MS: u32 = 2 * BATTLE_STAGE_MS;

static BATTLE_ANIM: TimedFlag = TimedFlag::new();
/// Packed context: byte0 = own pet kind, byte1 = opponent kind, bit16 =
/// viewer-won. Set once by [`show_battle_anim`], read by the renderer.
static BATTLE_ANIM_CTX: AtomicU32 = AtomicU32::new(0);
/// Opponent device id, packed little-endian into a u16. Kept out of
/// `BATTLE_ANIM_CTX` (which is full) — the renderer uses it to look the
/// opponent's pet name up in the friends list for the on-screen labels.
static BATTLE_ANIM_OPP_ID: AtomicU16 = AtomicU16::new(0);

/// Which stage of the battle animation to draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BattleStage {
    /// Both pets standing.
    Standing,
    /// Winner/loser poses + banner.
    Result,
    /// Animation finished — the takeover should end.
    Done,
}

/// Pure mapping from elapsed milliseconds to the current [`BattleStage`].
pub fn battle_stage_from_elapsed(ms: u32) -> BattleStage {
    if ms < BATTLE_STAGE_MS {
        BattleStage::Standing
    } else if ms < BATTLE_TOTAL_MS {
        BattleStage::Result
    } else {
        BattleStage::Done
    }
}

/// Start the full-screen battle animation. `own_kind` is drawn on the left,
/// `opp_kind` on the right (mirrored); `viewer_won` picks the stage-2 poses.
/// `own_hp`/`opp_hp` are the outcome HP percentages (0..=100) shown by the
/// stage-2 power bars — stage 1 always shows 100%.
///
/// `opp_id` is the opponent's device id, used only to label them on screen.
///
/// Layout of the packed context (`BATTLE_ANIM_CTX`, all in one u32):
/// bits 0-7 own_kind, 8-15 opp_kind, 16-22 own_hp, 23-29 opp_hp, 30 viewer_won.
pub fn show_battle_anim(
    own_kind: u8,
    opp_kind: u8,
    viewer_won: bool,
    own_hp: u8,
    opp_hp: u8,
    opp_id: [u8; 2],
) {
    let ctx = own_kind as u32
        | ((opp_kind as u32) << 8)
        | ((own_hp.min(100) as u32) << 16)
        | ((opp_hp.min(100) as u32) << 23)
        | ((viewer_won as u32) << 30);
    BATTLE_ANIM_CTX.store(ctx, Ordering::Relaxed);
    BATTLE_ANIM_OPP_ID.store(u16::from_le_bytes(opp_id), Ordering::Relaxed);
    BATTLE_ANIM.start();
    #[cfg(feature = "embassy-core")]
    crate::TOAST_SIGNAL.signal(());
}

/// True while the battle-animation takeover should own the display.
pub fn battle_anim_active() -> bool {
    BATTLE_ANIM.is_active()
}

/// Current stage from the elapsed time since [`show_battle_anim`].
pub fn battle_anim_stage() -> BattleStage {
    battle_stage_from_elapsed(BATTLE_ANIM.elapsed_ms())
}

/// Unpacked animation context: `(own_kind, opp_kind, viewer_won)`.
pub fn battle_anim_ctx() -> (u8, u8, bool) {
    let c = BATTLE_ANIM_CTX.load(Ordering::Relaxed);
    (c as u8, (c >> 8) as u8, (c >> 30) & 1 != 0)
}

/// Opponent device id, for resolving their name in the friends list.
pub fn battle_anim_opp_id() -> [u8; 2] {
    BATTLE_ANIM_OPP_ID.load(Ordering::Relaxed).to_le_bytes()
}

/// Outcome HP percentages `(own_hp, opp_hp)` for the stage-2 power bars.
pub fn battle_anim_hp() -> (u8, u8) {
    let c = BATTLE_ANIM_CTX.load(Ordering::Relaxed);
    (((c >> 16) & 0x7F) as u8, ((c >> 23) & 0x7F) as u8)
}

/// End the battle-animation takeover (called once the stage reaches `Done`).
pub fn clear_battle_anim() {
    BATTLE_ANIM.clear();
}

/// Show a feedback toast.  Becomes visible on the next display
/// refresh and stays visible until the first refresh at or after
/// `TOAST_MIN_VISIBLE_MS` elapsed.  Fires `TOAST_SIGNAL` so the
/// display loop wakes up immediately even if no other event would
/// have triggered a redraw (e.g. an NFC station bonus arriving while
/// the display task is parked).
pub fn show_toast(toast: Toast) {
    raise_toast(toast);
    #[cfg(feature = "embassy-core")]
    crate::TOAST_SIGNAL.signal(());
}

/// Raise a toast without waking the display loop.
fn raise_toast(toast: Toast) {
    TOAST_MSG.store(toast as u8, Ordering::Relaxed);
    TOAST.start();
}

/// The cause toast: what went wrong (bold red, on the HAX row), then why
/// (red, one row lower, clear of the balance).
fn draw_cause_toast<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = crate::TriColor>,
{
    let Some(cause) = engine::causes::Cause::unpack(CAUSE_CODE.load(Ordering::Relaxed)) else {
        return Ok(());
    };
    let style = crate::ui::TOP_LEFT;
    let second = TOAST_POS + Point::new(0, TOAST_LINE_DY);
    for (text, at, ink) in [
        (cause.what(), TOAST_POS, crate::ui::TEXT_BOLD_RED),
        (cause.why(), second, crate::ui::TEXT_RED),
    ] {
        let pad = Point::new(STATUS_BACKING_PAD / 2, 0);
        draw_backing(display, at - pad, backing_width(text))?;
        crate::ui::draw_text(display, text, at, ink, style)?;
    }
    Ok(())
}

/// The diabetes alert: two bold lines and a red hint.
const ALERT_DIABETES: [&str; 2] = ["TYPE 2", "DIABETES"];
const ALERT_DIABETES_HINT: &str = "Give medication soon";
/// The dependence alert.
const ALERT_ALCOHOL: [&str; 2] = ["ALCOHOL", "DEPENDENCE"];
const ALERT_ALCOHOL_HINT: &str = "Give rehab soon";
/// The gone screen: the farewell (when there is no farewell animation) and
/// the two prompt lines.
const GONE_LEFT: &str = "Your pet has left";
/// Replaces `GONE_LEFT` for a pet moved to the Unicorn Realm because the badge
/// has no sprites for its kind.
const RETIRED_LINES: [&str; 2] = ["Moved to the", "Unicorn Realm"];
const GONE_PROMPT: [&str; 2] = ["Press Fire", "for a new egg"];
/// The pet area when the sprite files are missing.
const NO_SPRITES: &str = "No sprites on flash";

/// Every fixed word of the game screen: the toasts (group "toasts", the debug
/// cheat toast "debug"), the alerts, the gone screen and the missing-sprites
/// notice (group "screen"). Host-only, for the guide's word list.
#[cfg(any(test, feature = "simulator"))]
pub fn screen_words() -> std::vec::Vec<(&'static str, &'static str)> {
    let mut out: std::vec::Vec<(&'static str, &'static str)> = std::vec::Vec::new();
    for v in 0..=255u8 {
        let toast = Toast::from_u8(v);
        let text = toast.message();
        if !text.is_empty() {
            out.push((if toast == Toast::DebugCheat { "debug" } else { "toasts" }, text));
        }
    }
    for line in ALERT_DIABETES.into_iter().chain(ALERT_ALCOHOL) {
        out.push(("screen", line));
    }
    for text in [ALERT_DIABETES_HINT, ALERT_ALCOHOL_HINT, GONE_LEFT, NO_SPRITES] {
        out.push(("screen", text));
    }
    for line in RETIRED_LINES.into_iter().chain(GONE_PROMPT) {
        out.push(("screen", line));
    }
    out.push(("screen", NAME_PROMPT));
    // Demo mode: the help line and the pet's notes take the status line's place.
    out.push(("status", HELP_TEXT));
    for note in engine::demo::Note::ALL {
        out.push(("status", note.text()));
    }
    out
}

/// Full-screen one-shot condition alert: two bold black lines and a red hint,
/// cleared once `ALERT_MIN_VISIBLE_MS` has elapsed.
fn draw_alert<D>(
    display: &mut D,
    lines: [&str; 2],
    hint: &str,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    
    
    use embedded_graphics::text::Text;

    let centered = crate::ui::CENTERED;
    let font = crate::ui::TEXT_BOLD_BLACK;
    let font_red = crate::ui::TEXT_BOLD_RED;
    crate::ui::clear_screen(display)?;
    Text::with_text_style(lines[0], Point::new(76, 56), font, centered).draw(display)?;
    Text::with_text_style(lines[1], Point::new(76, 74), font, centered).draw(display)?;
    Text::with_text_style(hint, Point::new(76, 100), font_red, centered).draw(display)?;
    Ok(())
}

/// True when the game screen is showing a red status message — an active
/// toast, or the "gone / new egg" prompt.  The display loop uses this to run
/// a genuine full (tri-color) refresh instead of the fast delta refresh: red
/// ink under-drives on the non-inverting delta LUT (LUT2), so red text only
/// seats properly on the full-waveform path.  `partial_idle` upstream keeps
/// this from re-flashing once the message is drawn.
pub fn status_wants_full_refresh() -> bool {
    // The onset takeovers replace the screen, modal or not. A modal covers the
    // red message and the status line: its redraws are plain delta refreshes,
    // and the closing redraw gets the full one.
    DIABETES_ALERT.is_active()
        || ALCOHOLISM_ALERT.is_active()
        || (!modal::is_open()
            && (TOAST.is_active()
                || lifecycle::display_anim() == engine::DisplayAnim::Gone
                || (!lifecycle::is_hibernating()
                    && lifecycle::pet_line().is_some_and(|l| l.red()))
                || battle_view::wants_full_refresh()))
}

/// The reason of the reason toast while it is up. A menu shows it in its
/// title, because the toast itself is hidden under the popover.
pub(crate) fn reason_toast() -> Option<engine::doses::Reason> {
    (TOAST.is_active() && Toast::from_u8(TOAST_MSG.load(Ordering::Relaxed)) == Toast::Reason)
        .then(|| {
            engine::doses::Reason::unpack(
                REASON_KIND.load(Ordering::Relaxed),
                REASON_SECS.load(Ordering::Relaxed),
            )
        })
}

/// Tell the player why an action was refused or a row is disabled.
pub fn show_reason(why: engine::doses::Reason) {
    let (kind, secs) = why.pack();
    REASON_KIND.store(kind, Ordering::Relaxed);
    REASON_SECS.store(secs, Ordering::Relaxed);
    show_toast(Toast::Reason);
}

/// True while a full-screen view replaces the main game screen. Only the
/// main screen clears a toast, so none may be raised behind one.
pub fn full_screen_view_up() -> bool {
    crate::text_entry::is_active()
        || diary_view::is_active()
        || traits_view::is_active()
        || friends_view::is_active()
        || battle_view::is_active()
        || realm_view::is_active()
        || pet_select::is_active()
        || tictactoe::is_active()
        || lightsout::is_active()
        || blackhole::is_active()
        || bornjeweled::is_active()
}

/// Tell the player what went wrong and why (silent: no buzz). It does not
/// signal the display loop: the only caller is the cause drain inside
/// `lifecycle::cycle`, and every `cycle` caller is a draw or input path that
/// redraws right after, so a signal would only abort that very refresh.
pub fn show_cause(c: engine::causes::Cause) {
    CAUSE_CODE.store(c.pack(), Ordering::Relaxed);
    raise_toast(Toast::Cause);
}

/// The toast that is up now, if any (host-only, for the guide's shot checks).
#[cfg(any(test, feature = "simulator"))]
pub fn current_toast() -> Option<Toast> {
    TOAST
        .is_active()
        .then(|| Toast::from_u8(TOAST_MSG.load(Ordering::Relaxed)))
        .filter(|t| *t != Toast::None)
}

/// The cause the last `show_cause` stored.
#[cfg(test)]
pub(crate) fn last_cause_for_test() -> Option<engine::causes::Cause> {
    engine::causes::Cause::unpack(CAUSE_CODE.load(Ordering::Relaxed))
}

/// The reason the last `show_reason` stored.
#[cfg(test)]
pub(crate) fn last_reason_for_test() -> engine::doses::Reason {
    engine::doses::Reason::unpack(
        REASON_KIND.load(Ordering::Relaxed),
        REASON_SECS.load(Ordering::Relaxed),
    )
}

/// Tick the live view was last drawn (or opened); its refresh deadline is
/// anchored here so minute ticks cannot slide it forward.
static LIVE_DRAWN_TICK: AtomicU32 = AtomicU32::new(0);

/// Record that a live view was just drawn.
pub(crate) fn note_live_view_drawn() {
    LIVE_DRAWN_TICK.store(lifecycle::now_tick(), Ordering::Relaxed);
}

/// The next refresh tick of a live view last drawn at `drawn`: on the grid
/// of idle frames anchored at the draw, so it is never in the past (a view
/// hidden under another screen cannot cause a poll) and at most one idle
/// frame ahead.
fn live_deadline(drawn: u32, now: u32) -> u32 {
    let frames = now.saturating_sub(drawn) / pacing::IDLE_FRAME_TICKS + 1;
    drawn.saturating_add(pacing::IDLE_FRAME_TICKS.saturating_mul(frames))
}

/// True while a view whose numbers drift by themselves is on screen. Such a
/// view is refreshed at least once per idle frame (display pacing, not
/// engine polling). A Gone pet shows no live view.
pub fn live_view_open() -> bool {
    (modal::live_modal_view_open() || diary_view::is_active())
        && lifecycle::display_anim() != engine::DisplayAnim::Gone
}

/// Next tick the frame loop must wake for UI-only timed state, whatever the
/// engine says: a toast or alert is cleared by the first draw after its
/// minimum time, and the sleep animation floor ends at a fixed tick. Without
/// this the 2 h engine cap would leave them on screen for hours. The battle
/// result card is NOT here: it waits for a button press, which redraws, and
/// polling it would wake the badge every tick for as long as it is left up.
pub fn next_ui_deadline_tick() -> u32 {
    let now = lifecycle::now_tick();
    let mut next = lifecycle::sleep_anim_deadline_tick();
    // Only the main screen clears a toast or alert, so none is counted while
    // a full-screen view hides it. It is handled normally once the view closes.
    // A toast is also hidden under a modal's popover (the menu title shows a
    // reason instead); the alerts take the whole screen, modal or not.
    if !full_screen_view_up() {
        for (flag, min_ms, under_modal) in [
            (&TOAST, TOAST_MIN_VISIBLE_MS, false),
            (&DIABETES_ALERT, DIABETES_ALERT_MIN_VISIBLE_MS, true),
            (&ALCOHOLISM_ALERT, DIABETES_ALERT_MIN_VISIBLE_MS, true),
        ] {
            if flag.is_active() && (under_modal || !modal::is_open()) {
                next = next.min(flag.clear_tick(min_ms, now));
            }
        }
    }
    if live_view_open() {
        let drawn = LIVE_DRAWN_TICK.load(Ordering::Relaxed);
        next = next.min(live_deadline(drawn, now));
    }
    next
}

/// Show the station-cooldown toast with the remaining time formatted
/// from `secs` (e.g. `"wait 4:50"`).
pub fn show_station_cooldown(secs: u16) {
    STATION_COOLDOWN_SECS.store(secs, Ordering::Relaxed);
    show_toast(Toast::StationCooldown);
}

// ── Layout constants
// ──────────────────────────────────────────────────────────

/// X centres of the four icon columns (evenly spaced across 152 px).
const ICON_CX: [i32; 4] = [19, 57, 95, 133];
/// Y centre of the top icon row.
const TOP_CY: i32 = 17;
/// Y centre of the bottom icon row.
const BOT_CY: i32 = 131;
/// Y of the separator below the top icon row.
const SEP_TOP: i32 = 34;
/// Y of the separator above the bottom icon row.
const SEP_BOT: i32 = 111;
/// Status line: x of the centre of every line (panel is 152 px wide).
const STATUS_CENTER_X: i32 = 76;
/// Status line: y of the bottom row of the last line. The last line is
/// centred on the bottom separator (6 rows above it, 6 below), so its white
/// backing interrupts the separator under the text.
const STATUS_BASE_Y: i32 = SEP_BOT + 6;
/// Status line: height of one text line (FONT_7X13_BOLD).
const STATUS_LINE_H: i32 = 13;
/// Status line: width of one column (FONT_7X13_BOLD).
const STATUS_CHAR_W: i32 = 7;
/// Status line: extra white backing width, split evenly left and right.
const STATUS_BACKING_PAD: i32 = 4;

/// The HAX balance: right-aligned, top baseline, ending at x = 150.
const HAX_POS: Point = Point::new(150, 33);
/// The toast's first line, top-left. It shares the HAX row, so the cause
/// toast keeps its `what` line short enough to end left of the balance.
const TOAST_POS: Point = Point::new(2, SEP_TOP + 2);
/// Distance from a toast's first line to its second.
const TOAST_LINE_DY: i32 = 13;
/// First display row of the pet/sprite area.
pub const PET_AREA_TOP: usize = SEP_TOP as usize + 1;

// ── Public entry point
// ────────────────────────────────────────────────────────

/// Width of the white backing behind a line of 7x13 text.
fn backing_width(text: &str) -> i32 {
    text.chars().count() as i32 * STATUS_CHAR_W + STATUS_BACKING_PAD
}

/// White backing for one line of text, so it reads over the sprite. `top_left`
/// is the backing's corner and `w` its width ([`backing_width`]).
fn draw_backing<D>(display: &mut D, top_left: Point, w: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(top_left, Size::new(w as u32, STATUS_LINE_H as u32))
        .into_styled(PrimitiveStyle::with_fill(crate::WHITE))
        .draw(display)
}

/// The background behind the pet, when there is one and sprites exist
/// (`frame_count() > 0`). Nothing while a full-screen view, a mini-game or a
/// takeover alert is up: the screen returns early then, and the "No sprites
/// on flash" text stays on white.
pub(crate) fn draw_backdrop<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    if sprite_loader::frame_count() == 0
        || full_screen_view_up()
        || DIABETES_ALERT.is_active()
        || ALCOHOLISM_ALERT.is_active()
    {
        return Ok(());
    }
    match lifecycle::background_state() {
        Some(bg) => background::draw_background(display, &bg),
        None => Ok(()),
    }
}

/// Draw the status line centred on the bottom separator: bold
/// black (bold red in danger), each line on a white backing of its own width
/// plus [`STATUS_BACKING_PAD`] so it reads over the sprite. A second line sits
/// above the first.
pub fn draw_status_line<D>(
    display: &mut D,
    line: &engine::status::StatusLine,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let style = if line.danger {
        crate::ui::TEXT_BOLD_RED
    } else {
        crate::ui::TEXT_BOLD_BLACK
    };
    draw_pet_text(display, line.text().as_str(), style)
}

/// Draw what is written under the pet: the status line, the demo pet's note
/// (bold black) or its help line (bold red), all at the same place on the
/// same white backing.
pub fn draw_pet_line<D>(display: &mut D, line: &engine::PetLine) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    match line {
        engine::PetLine::Status(l) => draw_status_line(display, l),
        engine::PetLine::Note(n) => {
            draw_pet_text(display, n.text(), crate::ui::TEXT_BOLD_BLACK)
        }
        engine::PetLine::Help => draw_pet_text(display, HELP_TEXT, crate::ui::TEXT_BOLD_RED),
    }
}

/// The help line's words (the demo pet is below `DEMO_HELP_HEALTH_PCT`).
const HELP_TEXT: &str = "could use a hand";

/// Bottom-centred `text` (lines split on `\n`), each on its own backing.
fn draw_pet_text<D>(
    display: &mut D,
    text: &str,
    style: embedded_graphics::mono_font::MonoTextStyle<'static, TriColor>,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let count = text.split('\n').count() as i32;
    for (i, row) in text.split('\n').enumerate() {
        let base = STATUS_BASE_Y - (count - 1 - i as i32) * STATUS_LINE_H;
        let w = backing_width(row);
        draw_backing(
            display,
            Point::new(STATUS_CENTER_X - w / 2, base - (STATUS_LINE_H - 1)),
            w,
        )?;
        crate::ui::draw_text(
            display,
            row,
            Point::new(STATUS_CENTER_X, base),
            style,
            crate::ui::CENTERED_BOTTOM,
        )?;
    }
    Ok(())
}

/// The battery bar's top edge (embassy builds draw the bar).
const BATTERY_Y: i32 = 2;

/// The demo mark: a 4 x 5 `D`, bit 3 the left column.
const DEMO_MARK: [u8; 5] = [0b1110, 0b1001, 0b1001, 0b1001, 0b1110];

/// Where the demo mark sits: under the battery bar, with a 2 px gap.
const DEMO_MARK_POS: Point = Point::new(
    152 - crate::BATTERY_BAR_W as i32,
    BATTERY_Y + crate::BATTERY_BAR_H as i32 + 2,
);

/// Draw the demo mark in black. Black only, so plain delta refreshes keep it.
fn draw_demo_mark<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let on = DEMO_MARK.iter().enumerate().flat_map(|(row, bits)| {
        (0..4)
            .filter(move |col| bits & (0b1000 >> col) != 0)
            .map(move |col| Pixel(DEMO_MARK_POS + Point::new(col, row as i32), BLACK))
    });
    display.draw_iter(on)
}

/// The action feedback toast, shown until the first render at or after
/// `TOAST_MIN_VISIBLE_MS` has elapsed since it was raised.
fn draw_toast<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Clear first: an expired toast is dropped on this wake and never redrawn.
    TOAST.clear_if_elapsed(TOAST_MIN_VISIBLE_MS);
    if TOAST.is_active() {
        let toast = Toast::from_u8(TOAST_MSG.load(Ordering::Relaxed));
        // Dynamic toasts (station cooldown) format their text at draw
        // time from a small atomic; everything else uses the static
        // message table.
        let mut dyn_buf: heapless::String<48> = heapless::String::new();
        let msg: &str = if let Toast::Reason = toast {
            let why = engine::doses::Reason::unpack(
                REASON_KIND.load(Ordering::Relaxed),
                REASON_SECS.load(Ordering::Relaxed),
            );
            dyn_buf.push_str(why.text().as_str()).ok();
            dyn_buf.as_str()
        } else if let Toast::DebugInfo = toast {
            // Read live: the toast shows what the engine is doing now.
            if let Some(info) = lifecycle::debug_info() {
                let line = info.line();
                dyn_buf.push_str(line.as_str()).ok();
            }
            dyn_buf.as_str()
        } else if let Toast::StationCooldown = toast {
            let secs = STATION_COOLDOWN_SECS.load(Ordering::Relaxed);
            let m = secs / 60;
            let s = secs % 60;
            let _ = core::fmt::Write::write_fmt(&mut dyn_buf, format_args!("wait {}:{:02}", m, s));
            dyn_buf.as_str()
        } else {
            toast.message()
        };
        if let Toast::Cause = toast {
            draw_cause_toast(display)?;
        } else if !msg.is_empty() {
            
            
            use embedded_graphics::text::{Text};
            let style = crate::ui::TOP_LEFT;
            // White backing per line, so red text reads on the night sky.
            let pad = Point::new(STATUS_BACKING_PAD / 2, 0);
            for (i, line) in msg.split('\n').enumerate() {
                let at = TOAST_POS + Point::new(0, i as i32 * TOAST_LINE_DY);
                draw_backing(display, at - pad, backing_width(line))?;
            }
            Text::with_text_style(
                msg,
                TOAST_POS,
                // Red so the action feedback (-hunger / -sick / +inspired /
                // bonus) pops against the black UI.
                crate::ui::TEXT_BOLD_RED,
                style,
            )
            .draw(display)?;
        }
    }
    Ok(())
}

/// Render the BornPets game screen.
///
/// Handles four states:
/// - **Not started**: "Press Fire to start" — no icons.
/// - **Hatching**: egg animation + countdown — no icons.
/// - **Gone**: farewell + "Press Execute for new egg" — no icons.
/// - **Active**: icons + pet animation + modal overlay.
pub fn draw_screen_game<D>(display: &mut D, nav: GameNav) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    
    
    use embedded_graphics::text::{Text};
    use engine::to_display::DisplayAnim;

    // `nav` only drives the simulator's in-line icon blit (firmware
    // does that work in the async pre-pass `render()` instead).  Suppress
    // the unused-variable warning for non-simulator builds.
    #[cfg(not(feature = "simulator"))]
    let _ = &nav;

    let centered = crate::ui::CENTERED;
    let font = crate::ui::TEXT_BOLD_BLACK;
    // Red style for pet status prompts (the "gone / new egg" screen) so they
    // stand out from the black countdown timer / labels.
    let font_red = crate::ui::TEXT_BOLD_RED;

    // ── Full-screen takeover screens ───────────────────────────────────
    // Diabetes onset alert takes priority over everything else, including
    // mini-games and modals — a rare, one-shot event worth interrupting
    // whatever's on screen for.
    // Clear first, like the toast: an expired alert is dropped on this wake
    // and never redrawn.
    DIABETES_ALERT.clear_if_elapsed(DIABETES_ALERT_MIN_VISIBLE_MS);
    ALCOHOLISM_ALERT.clear_if_elapsed(DIABETES_ALERT_MIN_VISIBLE_MS);
    if DIABETES_ALERT.is_active() {
        return draw_alert(
            display,
            ALERT_DIABETES,
            ALERT_DIABETES_HINT,
        );
    }

    // Alcoholism onset alert — same one-shot full-screen takeover as
    // diabetes, the twin permanent condition on the drink arc.
    if ALCOHOLISM_ALERT.is_active() {
        return draw_alert(
            display,
            ALERT_ALCOHOL,
            ALERT_ALCOHOL_HINT,
        );
    }

    if pet_select::is_active() {
        return pet_select::draw(display);
    }
    if tictactoe::is_active() {
        return tictactoe::draw(display);
    }
    if lightsout::is_active() {
        return lightsout::draw(display);
    }
    if blackhole::is_active() {
        return blackhole::draw(display);
    }
    if bornjeweled::is_active() {
        return bornjeweled::draw(display);
    }

    // Battery — a slim segmented bar flush against the right edge, not the
    // full icon. The 24 px-wide icon crowded the game menu icon on this
    // screen; the bar is 4 px, leaving that row clear.
    #[cfg(feature = "embassy-core")]
    {
        let pct = crate::fw::battery::read_pct();
        let x = 152 - crate::BATTERY_BAR_W as i32;
        crate::draw_battery_bar(display, x, BATTERY_Y, pct)?;
    }

    // Sim-only sprite blit: in firmware the async `render()` blits sprites
    // from FAT12 between display refreshes; the simulator has no flash, so
    // we resolve PCX assets directly from `assets/to-badge/` here.  Drawn
    // before icons/modals so the UI overlays it the same way it would on
    // hardware.  Sprite frame is driven from wall-clock elapsed time so
    // multi-frame animations actually cycle.
    #[cfg(feature = "simulator")]
    {
        use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

        draw_backdrop(display)?;

        use engine::anim_files;
        // Per-animation start delta: when the anim id changes we pin
        // the wall-clock origin so the new anim begins at frame 0.
        // The free-running clock keeps ticking — we just subtract.
        static LAST_ANIM_ID: AtomicU8 = AtomicU8::new(0xFF);
        static ANIM_START_MS: AtomicU64 = AtomicU64::new(0);
        // Frame interval fixed when the animation starts, so a shrinking
        // action/hatching countdown cannot make the frame index jump or wrap.
        static ANIM_INTERVAL_MS: AtomicU64 = AtomicU64::new(u64::MAX);

        if !lifecycle::is_started() {
            sprite_loader::blit_pcx_sim(display, &anim_files::start_screen_filename(), 0, 0, false);
        } else {
            let kind = lifecycle::pet_kind();
            let anim = lifecycle::display_anim();
            let count = anim_files::frame_count(kind, anim);
            if count > 0 {
                let elapsed_ms = crate::platform::uptime_ms();
                let id = anim_files::anim_id_for(anim);
                if LAST_ANIM_ID.load(Ordering::Relaxed) != id {
                    LAST_ANIM_ID.store(id, Ordering::Relaxed);
                    ANIM_START_MS.store(elapsed_ms, Ordering::Relaxed);
                    let interval = pacing::frame_interval_secs(
                        anim,
                        count as u32,
                        lifecycle::action_ticks_remaining(),
                    )
                    .map_or(u64::MAX, |s| s.saturating_mul(1000));
                    ANIM_INTERVAL_MS.store(interval, Ordering::Relaxed);
                }
                let delta_ms = elapsed_ms.saturating_sub(ANIM_START_MS.load(Ordering::Relaxed));
                // Same pace as the firmware waiter (`pacing`): idle frames
                // every 5 minutes, actions and hatching at full speed.
                let interval_ms = ANIM_INTERVAL_MS.load(Ordering::Relaxed);
                let raw = (delta_ms / interval_ms).min(u32::MAX as u64) as u32;
                let frame = anim_files::clamp_or_wrap(raw, count, anim);
                let name = anim_files::anim_filename(kind, anim, frame);
                sprite_loader::blit_pcx_sim(display, &name, 0, PET_AREA_TOP as i32, false);
            }
        }
    }

    // HAX balance — top-right, below the battery icon + menu icon row
    // (y=33 clears the icons; y=16 overlapped them). Larger bold font.
    // Hidden entirely when money mode is disabled for this pet.
    if lifecycle::money_enabled() {
        let right_style = crate::ui::TOP_RIGHT;
        let mut money_buf: heapless::String<12> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(
            &mut money_buf,
            format_args!("{} H", lifecycle::money()),
        );
        // Reuse the already-linked bold font (FONT_7X13_BOLD via `font`) —
        // pulling in a second glyph table (9x15) overflowed FLASH by ~12 KB.
        // White backing, so the balance reads over the night sky.
        let w = backing_width(money_buf.as_str());
        draw_backing(display, Point::new(HAX_POS.x - w, HAX_POS.y), w)?;
        Text::with_text_style(money_buf.as_str(), HAX_POS, font, right_style)
            .draw(display)?;
    }

    // ── Not started ──────────────────────────────────────────────────────
    if !lifecycle::is_started() {
        // Start screen graphic is blitted by render(); only battery shown here.
        return Ok(());
    }

    let anim = lifecycle::display_anim();

    // ── Hatching ─────────────────────────────────────────────────────────
    if let DisplayAnim::Hatching { ticks_remaining } = anim {
        // Egg animation is blitted by embassy.rs.  The countdown timer
        // below acts as the sole hatching indicator.
        let secs = ticks_remaining as u32 * 10;
        let mut time_str: heapless::String<16> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(
            &mut time_str,
            format_args!("{}:{:02}", secs / 60, secs % 60),
        );
        Text::with_text_style(time_str.as_str(), Point::new(76, 100), font, centered)
            .draw(display)?;
        return Ok(());
    }

    // ── Gone ─────────────────────────────────────────────────────────────
    if anim == DisplayAnim::Gone {
        // A Gone pet shows no menu or live view.
        if modal::is_open() {
            modal::close();
        }
        // Farewell animation blitted by embassy.rs if available.
        // Where the prompt goes: a pet moved to the Realm gets the unicorn
        // picture on top, so its lines sit lower.
        let prompt_ys = if lifecycle::is_hidden_kind(lifecycle::pet_kind()) {
            realm_picture::draw_realm_picture(display, Point::new(28, 4))?;
            for (line, y) in RETIRED_LINES.into_iter().zip([84, 100]) {
                Text::with_text_style(line, Point::new(76, y), font_red, centered)
                    .draw(display)?;
            }
            [122, 138]
        } else {
            if sprite_loader::frame_count() == 0 {
                Text::with_text_style(GONE_LEFT, Point::new(76, 50), font_red, centered)
                    .draw(display)?;
            }
            [90, 106]
        };
        Text::with_text_style(GONE_PROMPT[0], Point::new(76, prompt_ys[0]), font_red, centered)
            .draw(display)?;
        Text::with_text_style(GONE_PROMPT[1], Point::new(76, prompt_ys[1]), font_red, centered)
            .draw(display)?;
        return Ok(());
    }

    // ── Active game ──────────────────────────────────────────────────────

    // Menu icons — sprite-based.  In firmware the async `render()`
    // pre-pass already blitted the six 26×26 PCX icons before this
    // function ran (one per slot, F1 = normal, F2 = selected, the
    // selected variant replaces the firmware-drawn selection circle
    // entirely).  In simulator we don't have an async pre-pass, so
    // resolve them here from `assets/to-badge/`.
    #[cfg(feature = "simulator")]
    for slot in 0..engine::anim_files::MENU_ICON_COUNT {
        let (top_row, col) = engine::anim_files::menu_icon_pos(slot);
        let row_kind = if top_row { Row::Top } else { Row::Bottom };
        let cy = if matches!(row_kind, Row::Top) {
            TOP_CY
        } else {
            BOT_CY
        };
        let cx = ICON_CX[col as usize];
        let selected = nav.row == row_kind && nav.col == col;
        let name = engine::anim_files::menu_icon_filename(slot, selected);
        sprite_loader::blit_pcx_sim(display, &name, cx - 13, cy - 13, false);
    }

    Rectangle::new(Point::new(0, SEP_TOP), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Pet area: sprite blitted by embassy.rs, or fallback.
    if sprite_loader::frame_count() == 0 {
        Text::with_text_style(
            NO_SPRITES,
            Point::new(76, (SEP_TOP + SEP_BOT) / 2),
            font,
            centered,
        )
        .draw(display)?;
    }

    Rectangle::new(Point::new(0, SEP_BOT), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    draw_toast(display)?;

    // The status line replaces the old corner banners. A hibernating pet is
    // frozen, so it says nothing.
    if !lifecycle::is_hibernating()
        && let Some(line) = lifecycle::pet_line()
    {
        draw_pet_line(display, &line)?;
    }

    // The demo mark shows on the main game screen only: not under a menu.
    if lifecycle::is_demo() && !modal::is_open() {
        draw_demo_mark(display)?;
    }

    modal::draw_modal(display)?;

    Ok(())
}

// ── Async render — called from the display loop ──────────────────────────────

/// Full game render cycle: engine update, sprite blit, debug overlay, save.
///
/// Handles the start screen (full 152×152 blit of `00000000.PCX`),
/// in-game animation blitting, and the debug animation name overlay
/// when no artwork is loaded.
#[cfg(feature = "embassy-core")]
pub async fn render<D>(display: &mut D, sprite_frame: u8)
where
    D: embedded_graphics::draw_target::DrawTarget<Color = crate::TriColor>
        + crate::epd_driver::PlaneAccess,
{
    use engine::anim_files;
    use engine::to_display::DisplayAnim;

    use crate::fw::fat12;

    if lifecycle::is_started() {
        lifecycle::cycle();

        // After hatching completes, prompt the player to name their pet.
        prompt_for_name();

        // The background goes down first; the sprite blits on top. Drop an
        // expired alert first (as `draw_screen_game` does), or the backdrop
        // would skip this frame while the game screen is drawn anyway.
        DIABETES_ALERT.clear_if_elapsed(DIABETES_ALERT_MIN_VISIBLE_MS);
        ALCOHOLISM_ALERT.clear_if_elapsed(DIABETES_ALERT_MIN_VISIBLE_MS);
        if draw_backdrop(display).is_err() {
            defmt::warn!("game: backdrop draw failed");
        }
    }

    // Blit sprite from flash.
    let mut has_sprite = false;

    if !lifecycle::is_started() {
        // Start screen: full 152×152 graphic at origin.
        let start_name = anim_files::start_screen_filename();
        if let Ok(file) = fat12::find_file(&start_name).await {
            sprite_loader::blit_file(display, &file, 0, 0, false).await;
            has_sprite = true;
        }
    } else {
        // In-game animation in the pet area.
        let kind = lifecycle::pet_kind();
        let anim = lifecycle::display_anim();
        let frame_count = anim_files::frame_count(kind, anim);
        if frame_count > 0 {
            let name = anim_files::anim_filename(kind, anim, sprite_frame);
            if let Ok(file) = fat12::find_file(&name).await {
                sprite_loader::blit_file(display, &file, 0, PET_AREA_TOP as i32, false).await;
                has_sprite = true;
            }
        }

        // Menu icons (top + bottom rows) — six 26×26 PCX sprites under
        // prefix `0x03`.  The selected variant fully replaces the
        // firmware-drawn selection circle so we don't call
        // `draw_selection_bg` for those slots.  Missing PCX files fail
        // soft (cell stays whatever the EPD was cleared to).
        let nav = nav::get_nav();
        for slot in 0..anim_files::MENU_ICON_COUNT {
            let (top_row, col) = anim_files::menu_icon_pos(slot);
            let cy = if top_row { TOP_CY } else { BOT_CY };
            let cx = ICON_CX[col as usize];
            let row_kind = if top_row { Row::Top } else { Row::Bottom };
            let selected = nav.row == row_kind && nav.col == col;
            let name = anim_files::menu_icon_filename(slot, selected);
            if let Ok(file) = fat12::find_file(&name).await {
                sprite_loader::blit_file(display, &file, cx - 13, cy - 13, false).await;
            }
        }
    }

    // Debug: show animation name when no artwork loaded.
    if !has_sprite && lifecycle::is_started() && lifecycle::display_anim() != DisplayAnim::Gone {
        use embedded_graphics::text::{Text};

        let anim = lifecycle::display_anim();
        let anim_name: &str = match anim {
            // Unreachable: the guard above excludes Gone; kept for exhaustiveness.
            DisplayAnim::Gone => "GONE",
            DisplayAnim::Hibernating => "HIBERNATE",
            DisplayAnim::Hatching { .. } => "HATCHING",
            DisplayAnim::Feeding => "FEEDING",
            DisplayAnim::Healing => "HEALING",
            DisplayAnim::Playing => "PLAYING",
            DisplayAnim::Sleeping => "SLEEPING",
            DisplayAnim::Exercising => "EXERCISING",
            DisplayAnim::Medicating => "MEDICATING",
            DisplayAnim::Drinking => "DRINKING",
            DisplayAnim::Ozempic => "OZEMPIC",
            DisplayAnim::Rehab => "REHAB",
            DisplayAnim::OnlyPets => "ONLYPETS",
            DisplayAnim::Relaxing => "RELAXING",
            DisplayAnim::Washing => "WASHING",
            DisplayAnim::Tipsy => "TIPSY",
            DisplayAnim::Wasted => "WASTED",
            DisplayAnim::ThrowingUp => "THROWUP",
            DisplayAnim::HungOver => "HUNGOVER",
            DisplayAnim::Sore => "SORE",
            DisplayAnim::Injured => "INJURED",
            DisplayAnim::Groggy => "GROGGY",
            DisplayAnim::Stuffed => "STUFFED",
            DisplayAnim::Thirsty => "THIRSTY",
            DisplayAnim::Wired => "WIRED",
            DisplayAnim::NeedsRehab => "NEEDSREHAB",
            DisplayAnim::Starving => "STARVING",
            DisplayAnim::Leaving { .. } => "LEAVING",
            DisplayAnim::CriticalSick => "CRIT:SICK",
            DisplayAnim::CriticalTired => "CRIT:TIRED",
            DisplayAnim::CriticalHungry => "CRIT:HUNGRY",
            DisplayAnim::WarningSick => "WARN:SICK",
            DisplayAnim::WarningTired => "WARN:TIRED",
            DisplayAnim::WarningHungry => "WARN:HUNGRY",
            DisplayAnim::WarningMiserable => "WARN:MISER",
            DisplayAnim::Happy => "HAPPY",
            DisplayAnim::Idle => "IDLE",
        };
        let style = crate::ui::TOP_RIGHT;
        let _ = Text::with_text_style(
            anim_name,
            Point::new(150, 36),
            crate::ui::TEXT_BLACK,
            style,
        )
        .draw(display);
    }

    if lifecycle::is_started() {
        lifecycle::save_if_needed().await;
    }
}

/// Title of the naming prompt.
pub const NAME_PROMPT: &str = "Name your Pet";

/// Longest name the prompt accepts.
const NAME_PROMPT_MAX: u8 = 12;

/// Once the egg has hatched, open the naming prompt with a default name
/// filled in. True when it opened. The firmware's render loop and the guide's
/// `naming` shot both call this.
pub fn prompt_for_name() -> bool {
    if !lifecycle::take_naming_pending() {
        return false;
    }
    let default = lifecycle::random_default_name(lifecycle::now_tick());
    crate::text_entry::begin(
        default.as_bytes(),
        NAME_PROMPT_MAX,
        on_pet_named,
        NAME_PROMPT,
        true,
    );
    true
}

/// Text entry callback: player has submitted a pet name.
fn on_pet_named(name: &[u8]) {
    lifecycle::set_pet_name(name);
}

#[cfg(test)]
mod battle_anim_tests {
    use super::{BattleStage, battle_stage_from_elapsed};

    #[test]
    fn stage_boundaries() {
        assert_eq!(battle_stage_from_elapsed(0), BattleStage::Standing);
        assert_eq!(battle_stage_from_elapsed(4_999), BattleStage::Standing);
        assert_eq!(battle_stage_from_elapsed(5_000), BattleStage::Result);
        assert_eq!(battle_stage_from_elapsed(9_999), BattleStage::Result);
        assert_eq!(battle_stage_from_elapsed(10_000), BattleStage::Done);
        assert_eq!(battle_stage_from_elapsed(u32::MAX), BattleStage::Done);
    }
}

/// The one lock for every test that touches game-global state (`GAME`, the
/// toast, the sleep-animation floor, `battle_view`, alerts). Hold the guard
/// for the whole test; dropping it (also on a failed assert) clears the UI
/// statics so the next test starts clean.
#[cfg(test)]
pub(crate) struct TestState(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

/// Mini-game payout tests: a live pet with money mode on, installed as the
/// game state. Hold the guard for the whole test.
#[cfg(test)]
pub(crate) fn minigame_pay_setup() -> TestState {
    let guard = lock_test_state();
    let mut s = engine::GameState::new_egg(30, engine::PetKind::DEFAULT);
    s.update(engine::thresholds::HATCHING_TICKS() as u32);
    s.money = 0;
    s.money_enabled = true;
    lifecycle::set_state_for_test(Some(s));
    TOAST.clear();
    guard
}

/// HAX the test pet holds, and whether `game`'s cooldown runs.
#[cfg(test)]
pub(crate) fn minigame_pay_read(game: engine::MiniGame) -> (u32, bool) {
    let mut out = (0, false);
    lifecycle::with_state(|s| {
        out = (s.money, s.game_cooldown(game) > 0);
        false
    });
    out
}

#[cfg(test)]
static TEST_STATE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) fn lock_test_state() -> TestState {
    let guard = TEST_STATE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Lib tests run at noon, whatever the hour of the build.
    crate::platform::set_fake_unix(Some(crate::platform::GUIDE_NOON_UNIX));
    TestState(guard)
}

/// Every full-screen view the lib.rs and game draw dispatch handles under the
/// game feature (the firmware qwiic overlay aside): name, open, close.
#[cfg(test)]
const FULL_SCREEN_VIEWS: [(&str, fn(), fn()); 11] = [
    ("text_entry", || crate::text_entry::begin(b"", 1, |_| {}, "t", false), crate::text_entry::close_for_test),
    ("realm", realm_view::open, realm_view::close),
    ("traits", traits_view::open, traits_view::close),
    ("diary", diary_view::open, diary_view::close),
    ("friends", friends_view::open, friends_view::close),
    ("battle", battle_view::open, battle_view::close),
    ("pet_select", pet_select::open_new_game, pet_select::close),
    ("tictactoe", tictactoe::open, tictactoe::close),
    ("lightsout", lightsout::open, lightsout::close),
    ("blackhole", blackhole::open, blackhole::close),
    ("bornjeweled", bornjeweled::open, bornjeweled::close),
];

#[cfg(test)]
impl Drop for TestState {
    fn drop(&mut self) {
        crate::platform::set_fake_unix(None);
        TOAST.clear();
        DIABETES_ALERT.clear();
        ALCOHOLISM_ALERT.clear();
        lifecycle::clear_sleep_anim_for_test();
        battle_view::reset_for_test();
        diary_view::close();
        modal::close();
        lifecycle::clear_diary();
        for (_, _, close) in FULL_SCREEN_VIEWS {
            close();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_full_screen_view_is_in_the_predicate() {
        let _guard = super::lock_test_state();
        assert!(!super::full_screen_view_up());
        for (name, open, close) in super::FULL_SCREEN_VIEWS {
            open();
            assert!(super::full_screen_view_up(), "{name} is not in full_screen_view_up");
            close();
            assert!(!super::full_screen_view_up(), "{name} did not close");
        }
    }

    fn active_test_pet() -> crate::game::engine::GameState {
        use crate::game::engine::{GameState, PetKind, Phase};
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        s
    }

    #[test]
    fn a_toast_or_alert_behind_a_full_screen_view_pulls_no_deadline() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        crate::game::lifecycle::set_state_for_test(Some(active_test_pet()));
        for alert in [None, Some(&super::DIABETES_ALERT), Some(&super::ALCOHOLISM_ALERT)] {
            match alert {
                None => super::show_toast(super::Toast::Feed),
                Some(a) => a.start(),
            }
            let now = crate::game::lifecycle::now_tick();
            assert!(super::next_ui_deadline_tick() <= now + 1, "drawable: cleared next wake");
            super::diary_view::open();
            assert!(super::full_screen_view_up());
            let now = crate::game::lifecycle::now_tick();
            assert!(
                super::next_ui_deadline_tick() > now + 1,
                "nothing can clear it behind the view"
            );
            super::diary_view::close();
            let now = crate::game::lifecycle::now_tick();
            assert!(super::next_ui_deadline_tick() <= now + 1, "handled again once the view closes");
            super::TOAST.clear();
            super::DIABETES_ALERT.clear();
            super::ALCOHOLISM_ALERT.clear();
        }
    }

    #[test]
    fn a_cause_behind_a_modal_is_logged_but_not_toasted() {
        use crate::game::engine::causes::Cause;
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        crate::game::lifecycle::set_state_for_test(Some(active_test_pet()));
        super::modal::open(super::modal::ModalKind::Stats);
        crate::game::lifecycle::with_state(|s| {
            s.causes.push(Cause::FoodComa);
            true
        });
        crate::game::lifecycle::cycle();
        assert_eq!(crate::game::lifecycle::diary().len(), 1);
        assert_eq!(super::current_toast().unwrap_or(super::Toast::None), super::Toast::None);
    }

    #[test]
    fn an_expired_toast_is_cleared_before_the_draw_not_after() {
        let _guard = super::lock_test_state();
        super::show_toast(super::Toast::Feed);
        let mut fresh = Canvas::new();
        super::draw_toast(&mut fresh).unwrap();
        assert!(fresh.any(rows(0, 151), RED), "a fresh toast is drawn");
        assert!(super::TOAST.is_active());
        super::TOAST.backdate_for_test(super::TOAST_MIN_VISIBLE_MS);
        let mut late = Canvas::new();
        super::draw_toast(&mut late).unwrap();
        assert!(!late.any(rows(0, 151), RED), "an expired toast is never redrawn");
        assert!(!super::TOAST.is_active());
    }

    #[test]
    fn a_toast_deadline_is_its_remaining_time_never_in_the_past() {
        let _guard = super::lock_test_state();
        super::show_toast(super::Toast::Feed);
        assert_eq!(super::TOAST.clear_tick(super::TOAST_MIN_VISIBLE_MS, 500), 501);
        assert_eq!(super::TOAST.clear_tick(25_000, 500), 503, "the remaining time, rounded up");
        super::TOAST.backdate_for_test(60_000);
        assert_eq!(super::TOAST.clear_tick(25_000, 500), 501, "past due: next tick");
    }

    #[test]
    fn no_full_refresh_is_forced_while_a_modal_hides_the_status() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        let mut s = active_test_pet();
        s.fullness = (10 * crate::game::engine::needs::P) as u16;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert!(super::status_wants_full_refresh(), "danger on the main screen");
        super::show_toast(super::Toast::Feed);
        super::modal::open(super::modal::ModalKind::Stats);
        assert!(!super::status_wants_full_refresh(), "the popover covers it");
        super::modal::close();
        assert!(super::status_wants_full_refresh(), "the closing redraw gets the flash");
    }

    #[test]
    fn the_cause_toast_lines_sit_on_white_over_a_sprite() {
        use crate::game::engine::causes::Cause;
        let cause = Cause::FoodComa;
        super::CAUSE_CODE.store(cause.pack(), core::sync::atomic::Ordering::Relaxed);
        let mut c = Canvas::new();
        c.fill(BLACK);
        super::draw_cause_toast(&mut c).unwrap();
        for (text, y) in [
            (cause.what(), super::TOAST_POS.y),
            (cause.why(), super::TOAST_POS.y + super::TOAST_LINE_DY),
        ] {
            let w = text.chars().count() as i32 * super::STATUS_CHAR_W + super::STATUS_BACKING_PAD;
            let x0 = super::TOAST_POS.x - super::STATUS_BACKING_PAD / 2;
            let h = super::STATUS_LINE_H;
            for x in x0..x0 + w {
                assert_eq!(c.get(x, y), crate::WHITE, "{text}: backing top at x={x}");
                assert_eq!(c.get(x, y + h - 1), crate::WHITE, "{text}: backing bottom at x={x}");
            }
            assert_eq!(c.get(x0 + w, y + 5), BLACK, "{text}: no wider");
            // The two lines stack, so only the outer edges touch the sprite.
            let outer = if y == super::TOAST_POS.y { y - 1 } else { y + h };
            assert_eq!(c.get(x0 + 20, outer), BLACK, "{text}: no taller");
        }
    }

    #[test]
    fn an_onset_alert_still_gets_the_full_refresh_under_a_modal() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        crate::game::lifecycle::set_state_for_test(Some(active_test_pet()));
        super::modal::open(super::modal::ModalKind::Stats);
        assert!(!super::status_wants_full_refresh());
        super::DIABETES_ALERT.start();
        assert!(super::status_wants_full_refresh(), "diabetes takeover");
        super::DIABETES_ALERT.clear();
        super::ALCOHOLISM_ALERT.start();
        assert!(super::status_wants_full_refresh(), "alcoholism takeover");
    }

    #[test]
    fn an_expired_alert_is_cleared_before_the_screen_is_drawn() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        crate::game::lifecycle::set_state_for_test(Some(active_test_pet()));
        super::DIABETES_ALERT.start();
        super::DIABETES_ALERT.backdate_for_test(super::DIABETES_ALERT_MIN_VISIBLE_MS);
        let mut c = Canvas::new();
        super::draw_screen_game(&mut c, crate::game::nav::get_nav()).unwrap();
        assert!(!super::DIABETES_ALERT.is_active());
        assert!(!c.any(rows(95, 105), RED), "the alert hint is not drawn");
    }

    #[test]
    fn a_reason_toast_under_a_modal_pulls_no_deadline() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        crate::game::lifecycle::set_state_for_test(Some(active_test_pet()));
        super::modal::open(super::modal::ModalKind::Stats);
        super::show_reason(crate::game::engine::doses::Reason::Busy);
        let now = crate::game::lifecycle::now_tick();
        assert!(super::next_ui_deadline_tick() > now + 1, "the popover hides it");
        super::modal::close();
        assert!(super::next_ui_deadline_tick() <= now + 2, "handled again after close");
    }

    #[test]
    fn the_cause_toast_clears_the_hax_balance() {
        use embedded_graphics::prelude::*;
        use embedded_graphics::text::Text;
        // The widest balance the badge shows: "{} H" of a six-digit number.
        let mut hax: heapless::String<12> = heapless::String::new();
        core::fmt::Write::write_fmt(&mut hax, format_args!("{} H", 999_999)).unwrap();
        let hax_box = Text::with_text_style(
            hax.as_str(),
            super::HAX_POS,
            crate::ui::TEXT_BOLD_BLACK,
            crate::ui::TOP_RIGHT,
        )
        .bounding_box();
        for cause in super::engine::causes::Cause::all() {
            let what_box = Text::with_text_style(
                cause.what(),
                super::TOAST_POS,
                crate::ui::TEXT_BOLD_RED,
                crate::ui::TOP_LEFT,
            )
            .bounding_box();
            assert!(
                what_box.intersection(&hax_box).is_zero_sized(),
                "{cause:?}: {what_box:?} meets {hax_box:?}"
            );
            let why_box = Text::with_text_style(
                cause.why(),
                super::TOAST_POS + Point::new(0, super::TOAST_LINE_DY),
                crate::ui::TEXT_RED,
                crate::ui::TOP_LEFT,
            )
            .bounding_box();
            assert!(why_box.intersection(&hax_box).is_zero_sized(), "{cause:?}");
            assert!(why_box.bottom_right().unwrap().x <= 150, "{cause:?}");
        }
    }

    #[test]
    fn toasts_name_the_v3_effects() {
        use super::Toast;
        for (toast, text) in [
            (Toast::Feed, "+full"),
            (Toast::Heal, "healing"),
            (Toast::Sleep, "sleeping"),
            (Toast::Relax, "calmer +energy"),
            (Toast::Wash, "cleaner"),
            (Toast::Play, "+fun"),
            (Toast::Rehab, "rehab started"),
            (Toast::Ozempic, "-weight"),
        ] {
            assert_eq!(toast.message(), text);
            assert!(
                text.len() <= 14,
                "{text}: wider than the longest toast (Inspire bonus!)"
            );
        }
    }

    use super::test_canvas::Canvas;
    use super::{SEP_BOT, draw_status_line};
    use crate::game::engine::status::{StatusLine, Word};
    use crate::{BLACK, RED, WHITE};
    use embedded_graphics::prelude::*;
    use embedded_graphics::primitives::Rectangle;

    fn rows(y0: i32, y1: i32) -> Rectangle {
        Rectangle::new(Point::new(0, y0), Size::new(152, (y1 - y0 + 1) as u32))
    }

    /// Clears the installed test pet on drop, also after a failed assert.
    struct ClearPet;
    impl Drop for ClearPet {
        fn drop(&mut self) {
            crate::game::lifecycle::set_state_for_test(None);
        }
    }

    fn line_of(first: Word, danger: bool) -> StatusLine {
        StatusLine {
            first,
            second: None,
            danger,
        }
    }

    /// A one-line status line drawn over a `ink` backdrop (the sprite): the
    /// backing strip around the glyphs must be white, the rest untouched.
    fn assert_backing(first: Word, danger: bool, ink: crate::TriColor) {
        let mut c = Canvas::new();
        c.fill(ink);
        draw_status_line(&mut c, &line_of(first, danger)).unwrap();
        // Text width + STATUS_BACKING_PAD, centred, rows 97..=109.
        let w = first.text().len() as i32 * super::STATUS_CHAR_W + super::STATUS_BACKING_PAD;
        let x0 = super::STATUS_CENTER_X - w / 2;
        let y0 = super::STATUS_BASE_Y - (super::STATUS_LINE_H - 1);
        let pad = super::STATUS_BACKING_PAD / 2;
        for y in y0..=super::STATUS_BASE_Y {
            for x in (x0..x0 + pad).chain(x0 + w - pad..x0 + w) {
                assert_eq!(c.get(x, y), crate::WHITE, "backing margin at ({x},{y})");
            }
        }
        for x in x0..x0 + w {
            assert_eq!(c.get(x, y0), crate::WHITE, "backing top at x={x}");
        }
        assert_eq!(c.get(x0 - 1, y0 + 5), ink, "backing is no wider");
        assert_eq!(c.get(x0 + w, y0 + 5), ink, "backing is no wider");
        assert_eq!(c.get(x0 + 10, y0 - 1), ink, "backing is no taller");
    }

    #[test]
    fn status_line_draws_on_the_bottom_separator() {
        assert_backing(Word::Hungry, false, BLACK);
        let mut c = Canvas::new();
        draw_status_line(&mut c, &line_of(Word::Hungry, false)).unwrap();
        assert!(c.any(rows(105, 117), BLACK));
        assert!(!c.any(rows(0, 104), BLACK));
        assert!(!c.any(rows(0, 151), RED));
        assert!(!c.any(rows(118, 151), BLACK), "nothing over the icons");
    }

    #[test]
    fn status_line_straddles_the_bottom_separator() {
        use embedded_graphics::primitives::PrimitiveStyle;
        let mut c = Canvas::new();
        c.fill(WHITE);
        Rectangle::new(Point::new(0, SEP_BOT), Size::new(152, 1))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(&mut c)
            .unwrap();
        draw_status_line(&mut c, &line_of(Word::Hungry, false)).unwrap();
        let w = Word::Hungry.text().len() as i32 * super::STATUS_CHAR_W + super::STATUS_BACKING_PAD;
        let x0 = super::STATUS_CENTER_X - w / 2;
        assert_eq!(super::STATUS_BASE_Y - (super::STATUS_LINE_H - 1), 105);
        assert_eq!(super::STATUS_BASE_Y, 117);
        assert_eq!(c.get(x0 - 1, SEP_BOT), BLACK, "separator left of the text");
        assert_eq!(c.get(x0 + w, SEP_BOT), BLACK, "separator right of the text");
        assert_eq!(c.get(0, SEP_BOT), BLACK);
        assert_eq!(c.get(151, SEP_BOT), BLACK);
        // The backing's pad columns are white across the separator row.
        assert_eq!(c.get(x0, SEP_BOT), WHITE, "backing interrupts the line");
        assert_eq!(c.get(x0 + w - 1, SEP_BOT), WHITE);
        let icon_top = super::BOT_CY - 13;
        assert!(super::STATUS_BASE_Y < icon_top, "backing clear of the icons");
    }

    #[test]
    fn danger_line_is_red() {
        assert_backing(Word::Starving, true, BLACK);
        let mut c = Canvas::new();
        draw_status_line(&mut c, &line_of(Word::Starving, true)).unwrap();
        assert!(c.any(rows(105, 117), RED));
        assert!(!c.any(rows(0, 104), RED));
        assert!(!c.any(rows(118, 151), RED));
        assert!(!c.any(rows(0, 151), BLACK));
    }

    #[test]
    fn two_long_words_stack_up_from_the_separator() {
        let mut c = Canvas::new();
        let line = StatusLine {
            first: Word::Waterlogged,
            second: Some(Word::Underweight),
            danger: false,
        };
        assert_eq!(line.text().as_str(), "waterlogged\nunderweight");
        draw_status_line(&mut c, &line).unwrap();
        assert!(c.any(rows(92, 104), BLACK));
        assert!(c.any(rows(105, 117), BLACK));
        assert!(!c.any(rows(0, 91), BLACK));
        assert!(!c.any(rows(118, 151), BLACK));
    }

    #[test]
    fn full_refresh_follows_danger_not_banners() {
        use crate::game::engine::needs::P;
        use crate::game::engine::{GameState, PetKind, Phase};
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        let pet = || {
            let mut s = GameState::new_egg(7, PetKind(0));
            s.phase = Phase::Active;
            s
        };
        // 75 % weight was the old OVERWEIGHT banner: no longer a red message.
        let mut s = pet();
        s.weight = (75 * P) as u16;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert!(!super::status_wants_full_refresh());
        let mut s = pet();
        s.fullness = (10 * P) as u16;
        crate::game::lifecycle::set_state_for_test(Some(s.clone()));
        assert!(super::status_wants_full_refresh());
        // The line is not drawn while hibernating, so nothing red to seat.
        s.hibernating = true;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert!(!super::status_wants_full_refresh());
    }

    #[test]
    fn the_battle_result_card_needs_no_wake_loop() {
        let _guard = super::lock_test_state();
        // The card waits for a button press; the press redraws. Polling it
        // every tick would wake the badge for as long as it is left up.
        super::battle_view::show_result_for_test();
        assert!(super::battle_view::wants_full_refresh());
        assert_eq!(super::next_ui_deadline_tick(), u32::MAX);
    }

    #[test]
    fn live_deadline_is_on_a_grid_never_in_the_past() {
        use super::{live_deadline, pacing::IDLE_FRAME_TICKS as F};
        assert_eq!(live_deadline(500, 500), 500 + F);
        assert_eq!(live_deadline(500, 500 + F - 1), 500 + F);
        assert_eq!(live_deadline(500, 500 + F), 500 + 2 * F);
        let now = 1000;
        let d = live_deadline(now - 100, now);
        assert!(d > now && d <= now + F, "{d}");
        // a later draw moves the anchor
        assert_eq!(live_deadline(now, now), now + F);
    }

    #[test]
    fn a_gone_pet_keeps_no_live_view() {
        use crate::game::engine::{GameState, PetKind, Phase};
        let _guard = super::lock_test_state();
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        crate::game::lifecycle::set_state_for_test(Some(s));
        super::modal::open(super::modal::ModalKind::Stats);
        super::modal::activate();
        assert!(super::live_view_open());
        let mut gone = GameState::new_egg(7, PetKind(0));
        gone.phase = Phase::Gone;
        crate::game::lifecycle::set_state_for_test(Some(gone));
        assert_eq!(super::lifecycle::display_anim(), super::engine::DisplayAnim::Gone);
        assert!(!super::live_view_open());
        assert_eq!(super::next_ui_deadline_tick(), u32::MAX);
        super::modal::close();
        crate::game::lifecycle::set_state_for_test(None);
    }

    #[test]
    fn the_gone_screen_says_where_it_went() {
        use crate::game::engine::{GameState, PetKind, Phase};
        use crate::game::pet_registry;
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        pet_registry::install_test_roster();
        pet_registry::set_dropped_for_test(1 << 8);
        let nav = crate::game::nav::get_nav();
        let draw = |kind: u8| {
            let mut s = GameState::new_egg(7, PetKind(kind));
            s.phase = Phase::Gone;
            crate::game::lifecycle::set_state_for_test(Some(s));
            let mut c = Canvas::new();
            super::draw_screen_game(&mut c, nav).unwrap();
            c
        };
        let retired = draw(8);
        assert!(retired.any(rows(76, 88), RED), "first line");
        assert!(retired.any(rows(92, 104), RED), "second line");
        assert!(retired.any(rows(114, 128), RED), "Press Fire");
        assert!(retired.any(rows(130, 144), RED), "for a new egg");
        // frame_count() is u8::MAX on the simulator, so a farewell-less kind 0
        // draws no text of its own in these rows.
        let left = draw(0);
        assert!(!left.any(rows(42, 70), RED), "no retired lines for a known kind");
        // The normal gone screen is unchanged: no picture, prompt where it was.
        assert!(left.any(rows(82, 96), RED), "Press Fire stays put");
        assert!(left.any(rows(98, 112), RED), "for a new egg stays put");
        assert!(!left.any(rows(114, 151), RED), "nothing lower");
        // The retired screen carries the picture, and no text touches its box.
        let (pw, ph) = (
            crate::game::realm_picture::WIDTH as i32,
            crate::game::realm_picture::HEIGHT as i32,
        );
        let mut want = Canvas::new();
        crate::game::realm_picture::draw_realm_picture(&mut want, Point::new(28, 4)).unwrap();
        // (The test canvas also draws the sky backdrop, which the picture sits on.)
        let (mut ink, mut on_retired, mut on_left) = (0, 0, 0);
        for y in 4..4 + ph {
            for x in 28..28 + pw {
                if want.get(x, y) != WHITE {
                    ink += 1;
                    on_retired += (retired.get(x, y) == want.get(x, y)) as u32;
                    on_left += (left.get(x, y) == want.get(x, y)) as u32;
                }
            }
        }
        assert!(ink > 200, "the picture has ink");
        assert_eq!(on_retired, ink, "every picture pixel is on the realm screen");
        assert!(on_left < ink / 10, "the normal gone screen has no picture");
        assert!(!retired.any(rows(68, 75), RED), "gap between picture and text");
        assert!(want.any(rows(4, 67), RED), "the picture has red");
        assert!(want.any(rows(4, 67), BLACK), "the picture has black");
        // All text rows stay on the screen: the last row of ink is above 152.
        assert!(retired.any(rows(140, 151), RED));
        pet_registry::set_dropped_for_test(0);
        for line in super::RETIRED_LINES {
            assert!(line.len() <= 21, "{line}");
            assert!(super::screen_words().iter().any(|(_, w)| *w == line), "{line}");
        }
    }

    #[test]
    fn an_open_stats_view_refreshes_within_an_idle_frame() {
        use crate::game::engine::{GameState, PetKind, Phase};
        let _guard = super::lock_test_state();
        let mut s = GameState::new_egg(7, PetKind(0));
        s.phase = Phase::Active;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert_eq!(super::next_ui_deadline_tick(), u32::MAX);
        super::modal::open(super::modal::ModalKind::Stats);
        assert_eq!(
            super::next_ui_deadline_tick(),
            u32::MAX,
            "menu list has no drifting numbers"
        );
        super::modal::activate(); // first row: View stats
        assert!(super::live_view_open());
        // The deadline is anchored to the last draw, never to `now`: time
        // passing without a redraw (minute ticks) must not move it.
        super::LIVE_DRAWN_TICK.store(1000, core::sync::atomic::Ordering::Relaxed);
        let anchored = 1000 + super::pacing::IDLE_FRAME_TICKS;
        assert_eq!(super::next_ui_deadline_tick(), anchored);
        assert_eq!(super::next_ui_deadline_tick(), anchored);
        super::modal::close();
        assert!(!super::live_view_open());
        assert_eq!(super::next_ui_deadline_tick(), u32::MAX);
        crate::game::lifecycle::set_state_for_test(None);
    }

    use super::{BATTERY_Y, DEMO_MARK, DEMO_MARK_POS, draw_demo_mark, draw_pet_line};
    use crate::game::engine::PetLine;
    use crate::game::{pet_select, tictactoe};

    fn demo_pet() -> crate::game::engine::GameState {
        let mut s = active_test_pet();
        s.demo.on = true;
        s
    }

    /// Whether the 4 x 5 mark box on `c` holds exactly `DEMO_MARK` in black.
    fn mark_drawn(c: &Canvas) -> bool {
        (0..5).all(|row| {
            (0..4).all(|col| {
                let on = DEMO_MARK[row] & (0b1000 >> col) != 0;
                let want = if on { BLACK } else { crate::WHITE };
                c.get(DEMO_MARK_POS.x + col as i32, DEMO_MARK_POS.y + row as i32) == want
            })
        })
    }

    #[test]
    fn demo_mark_sits_under_the_battery() {
        let x = 152 - crate::BATTERY_BAR_W as i32;
        let mut bar = Canvas::new();
        crate::draw_battery_bar(&mut bar, x, BATTERY_Y, 100).unwrap();
        let mut c = Canvas::new();
        crate::draw_battery_bar(&mut c, x, BATTERY_Y, 100).unwrap();
        draw_demo_mark(&mut c).unwrap();
        assert_eq!(DEMO_MARK_POS, Point::new(148, 19));
        assert!(mark_drawn(&c));
        for yy in BATTERY_Y..BATTERY_Y + crate::BATTERY_BAR_H as i32 {
            for xx in x..x + 4 {
                assert_eq!(c.get(xx, yy), bar.get(xx, yy), "bar pixel ({xx},{yy})");
            }
        }
        for yy in 17..=18 {
            for xx in 148..=151 {
                assert_eq!(c.get(xx, yy), crate::WHITE, "gap ({xx},{yy})");
            }
        }
        let mut m = Canvas::new();
        draw_demo_mark(&mut m).unwrap();
        assert!(!m.any(rows(0, 151), RED));
        let blacks = (0..152)
            .flat_map(|yy| (0..152).map(move |xx| (xx, yy)))
            .filter(|&(xx, yy)| m.get(xx, yy) == BLACK);
        for (xx, yy) in blacks {
            let drink = (120..146).contains(&xx) && (4..30).contains(&yy);
            assert!(!drink, "({xx},{yy}) in the Drink icon box");
            assert!(xx >= 148, "({xx},{yy}) closer than 2 columns to Drink");
            assert!(yy < 33, "({xx},{yy}) reaches the HAX row or the separator");
        }
    }

    #[test]
    fn demo_mark_only_on_the_game_screen() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        let nav = crate::game::nav::get_nav();
        crate::game::lifecycle::set_state_for_test(Some(demo_pet()));
        let mut c = Canvas::new();
        super::draw_screen_game(&mut c, nav).unwrap();
        assert!(mark_drawn(&c), "demo on");

        crate::game::lifecycle::set_state_for_test(Some(active_test_pet()));
        let mut c = Canvas::new();
        super::draw_screen_game(&mut c, nav).unwrap();
        let box_px = Rectangle::new(DEMO_MARK_POS, Size::new(4, 5));
        assert!(!c.any(box_px, BLACK) && !c.any(box_px, RED), "demo off");

        crate::game::lifecycle::set_state_for_test(Some(demo_pet()));
        super::modal::open(super::modal::ModalKind::Stats);
        let mut c = Canvas::new();
        super::draw_screen_game(&mut c, nav).unwrap();
        assert!(!mark_drawn(&c), "a menu is open");
        super::modal::close();

        // The views `draw_screen_game` itself hands the screen to; the others
        // (diary, realm, ...) are dispatched before it is called at all.
        for (name, open, close) in [
            ("tictactoe", tictactoe::open as fn(), tictactoe::close as fn()),
            ("pet_select", pet_select::open_new_game, pet_select::close),
        ] {
            open();
            let mut c = Canvas::new();
            super::draw_screen_game(&mut c, nav).unwrap();
            assert!(!mark_drawn(&c), "{name} is up");
            close();
        }

        let mut gone = demo_pet();
        gone.phase = crate::game::engine::Phase::Gone;
        crate::game::lifecycle::set_state_for_test(Some(gone));
        let mut c = Canvas::new();
        super::draw_screen_game(&mut c, nav).unwrap();
        assert!(!mark_drawn(&c), "a gone pet");
    }

    #[test]
    fn demo_shows_hax() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        let nav = crate::game::nav::get_nav();
        let hax_rows = |c: &Canvas| {
            (33..=46)
                .flat_map(|yy| (110..152).map(move |xx| (xx, yy)))
                .map(|(xx, yy)| c.get(xx, yy))
                .collect::<Vec<_>>()
        };
        let draw = |demo: bool, money: bool| {
            let mut s = active_test_pet();
            s.money_enabled = money;
            s.money = 50;
            s.demo.on = demo;
            crate::game::lifecycle::set_state_for_test(Some(s));
            let mut c = Canvas::new();
            super::draw_screen_game(&mut c, nav).unwrap();
            hax_rows(&c)
        };
        let none = draw(false, false);
        assert_ne!(draw(false, true), none, "HAX shows when live");
        assert_ne!(draw(true, true), none, "demo shows it too");
        assert_eq!(draw(true, false), none, "no money layer, no HAX in demo");
    }

    #[test]
    fn help_line_is_red() {
        let mut c = Canvas::new();
        draw_pet_line(&mut c, &PetLine::Help).unwrap();
        assert!(c.any(rows(105, 117), RED));
        let mut n = Canvas::new();
        let note = crate::game::engine::demo::Note::Washing;
        draw_pet_line(&mut n, &PetLine::Note(note)).unwrap();
        assert!(n.any(rows(105, 117), BLACK));
        assert!(!n.any(rows(0, 151), RED), "a note is black");
    }

    #[test]
    fn full_refresh_follows_the_pet_line() {
        use crate::game::engine::needs::P;
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        let mut s = demo_pet();
        s.fullness = (2 * P) as u16;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert!(!super::status_wants_full_refresh(), "no red in demo");
        let mut s = demo_pet();
        s.health = (30 * P) as u16;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert!(super::status_wants_full_refresh(), "the help line is red");
        // Both ways of the boundary: just over the line, the help line is
        // off, and so is the red.
        let mut s = demo_pet();
        s.health = (crate::game::engine::thresholds::DEMO_HELP_HEALTH_PCT() as i32 * P + P) as u16;
        crate::game::lifecycle::set_state_for_test(Some(s));
        assert!(!super::status_wants_full_refresh(), "over the help line");
    }

    /// The views `lib.rs` hands the whole screen to (diary, realm, ...) never
    /// reach `draw_screen_game`, so the demo mark is not drawn over them.
    #[test]
    fn full_screen_views_do_not_draw_the_demo_mark() {
        let _guard = super::lock_test_state();
        let _clear = ClearPet;
        crate::game::lifecycle::set_state_for_test(Some(demo_pet()));
        for (name, open, close) in super::FULL_SCREEN_VIEWS {
            open();
            let mut c = Canvas::new();
            crate::draw_graphics(&mut c, "", &100).unwrap();
            assert!(!mark_drawn(&c), "{name} is up");
            close();
        }
    }

    /// The background behind the pet, on the game screen (simulator draws it
    /// in-line, the firmware in `render`).
    #[cfg(feature = "simulator")]
    mod backdrop {
        use crate::BLACK;
        use crate::game::engine::needs::P;
        use crate::game::engine::{GameState, PetKind, Phase};
        use crate::game::test_canvas::Canvas;
        use crate::game::*;

        struct ClearPet;
        impl Drop for ClearPet {
            fn drop(&mut self) {
                lifecycle::set_state_for_test(None);
            }
        }

        const NIGHT_MIN: Option<u16> = Some(23 * 60);

        fn pet(day_min: Option<u16>) -> GameState {
            let mut s = GameState::new_egg(7, PetKind(0));
            s.phase = Phase::Active;
            s.day_min = day_min;
            s.money_enabled = true;
            s.money = 120;
            s
        }

        fn screen() -> Canvas {
            let mut c = Canvas::new();
            draw_screen_game(&mut c, nav::get_nav()).unwrap();
            c
        }

        fn boxed(x: i32, y: i32, w: i32, h: i32) -> Rectangle {
            Rectangle::new(Point::new(x, y), Size::new(w as u32, h as u32))
        }

        fn same_in(a: &Canvas, b: &Canvas, area: Rectangle, what: &str) {
            for p in area.points() {
                assert_eq!(a.get(p.x, p.y), b.get(p.x, p.y), "{what} differs at {p:?}");
            }
        }

        fn differs_in(a: &Canvas, b: &Canvas, area: Rectangle) -> bool {
            area.points().any(|p| a.get(p.x, p.y) != b.get(p.x, p.y))
        }

        #[test]
        fn a_wake_with_an_unchanged_view_draws_the_same_screen() {
            // A wake up to two minutes before a sky switch finds the same
            // view. The firmware drives the panel only for pixels that differ
            // (`partial_idle` on the SSD1675, `delta.build` on the SSD1680), so
            // the whole screen (pet, status, scene) must be identical.
            use crate::game::engine::TICKS_PER_MINUTE;
            use crate::game::engine::scene::wake_view;

            let _guard = lock_test_state();
            let _clear = ClearPet;
            let mut s = pet(Some(20 * 60 + 57)); // the switch is at 21:00
            s.fullness = (60 * P) as u16;
            s.hydration = (60 * P) as u16;
            s.energy = (80 * P) as u16;
            s.health = (95 * P) as u16;
            s.mood = (90 * P) as u16;
            s.fun = (60 * P) as u16;
            s.dirt = 0;
            s.booze = 0;
            let t0 = s.last_update_tick;
            lifecycle::set_state_for_test(Some(s.clone()));
            let before = screen();
            s.update(t0 + TICKS_PER_MINUTE);
            let view = wake_view(&s);
            lifecycle::set_state_for_test(Some(s.clone()));
            let wake = screen();
            let whole = boxed(0, 0, 152, 152);
            same_in(&before, &wake, whole, "the screen at a same-view wake");
            s.update(t0 + 3 * TICKS_PER_MINUTE);
            assert_ne!(wake_view(&s), view, "21:00 switches the sky");
            lifecycle::set_state_for_test(Some(s));
            assert!(
                differs_in(&wake, &screen(), boxed(0, 35, 152, 75)),
                "the switch is on the screen"
            );
        }

        #[test]
        fn hax_reads_at_night() {
            let _guard = lock_test_state();
            let _clear = ClearPet;
            lifecycle::set_state_for_test(Some(pet(None)));
            let plain = screen();
            lifecycle::set_state_for_test(Some(pet(NIGHT_MIN)));
            let night = screen();
            let w = backing_width("120 H");
            let hax = boxed(HAX_POS.x - w, 35, w, 11);
            assert!(
                differs_in(&night, &plain, boxed(0, 50, 100, 40)),
                "the night dither is on the screen"
            );
            same_in(&night, &plain, hax, "HAX backing");
            assert!(
                !differs_in(&night, &plain, boxed(0, 0, 152, 34)),
                "header untouched"
            );
        }

        #[test]
        fn ui_boxes_stay_clear() {
            let _guard = lock_test_state();
            let _clear = ClearPet;
            let busy = || show_reason(engine::doses::Reason::Busy);
            let cause = || show_cause(engine::causes::Cause::FoodComa);
            for (name, raise) in [
                ("reason toast", &busy as &dyn Fn()),
                ("cause toast", &cause),
            ] {
                let mut shots = [None, None];
                for (i, day_min) in [None, NIGHT_MIN].into_iter().enumerate() {
                    let mut s = pet(day_min);
                    s.dirt = (95 * P) as u16;
                    s.health = (5 * P) as u16;
                    s.demo.on = true;
                    lifecycle::set_state_for_test(Some(s));
                    raise();
                    shots[i] = Some(screen());
                }
                let [Some(plain), Some(night)] = shots else {
                    unreachable!()
                };
                assert!(
                    differs_in(&night, &plain, boxed(0, 50, 100, 30)),
                    "{name}: the night dither is on the screen"
                );
                // The demo mark is on screen (the check is not vacuous) and
                // the whole header above the pet area is untouched.
                let mark = boxed(DEMO_MARK_POS.x, DEMO_MARK_POS.y, 4, 5);
                assert!(night.any(mark, BLACK), "{name}: demo mark drawn");
                same_in(&night, &plain, boxed(0, 0, 152, 34), "header");
                // Toast backings, one per line.
                let lines: std::vec::Vec<std::string::String> = if name == "reason toast" {
                    std::vec![engine::doses::Reason::Busy.text().as_str().into()]
                } else {
                    let c = engine::causes::Cause::FoodComa;
                    std::vec![c.what().into(), c.why().into()]
                };
                for (i, line) in lines.iter().enumerate() {
                    let y = TOAST_POS.y + i as i32 * TOAST_LINE_DY;
                    let w = backing_width(line);
                    same_in(
                        &night,
                        &plain,
                        boxed(TOAST_POS.x - STATUS_BACKING_PAD / 2, y, w, STATUS_LINE_H),
                        name,
                    );
                }
                // Status backing, one per line.
                lifecycle::set_state_for_test(Some({
                    let mut s = pet(NIGHT_MIN);
                    s.dirt = (95 * P) as u16;
                    s.health = (5 * P) as u16;
                    s.demo.on = true;
                    s
                }));
                let text = match lifecycle::pet_line().unwrap() {
                    engine::PetLine::Status(l) => l.text().as_str().into(),
                    engine::PetLine::Note(n) => std::string::String::from(n.text()),
                    engine::PetLine::Help => std::string::String::from(HELP_TEXT),
                };
                let count = text.split('\n').count() as i32;
                for (i, row) in text.split('\n').enumerate() {
                    let base = STATUS_BASE_Y - (count - 1 - i as i32) * STATUS_LINE_H;
                    let w = backing_width(row);
                    let area = boxed(
                        STATUS_CENTER_X - w / 2,
                        base - (STATUS_LINE_H - 1),
                        w,
                        STATUS_LINE_H,
                    );
                    same_in(&night, &plain, area, "status backing");
                }
            }
        }

        #[test]
        fn backdrop_sits_under_the_sprite() {
            let _guard = lock_test_state();
            let _clear = ClearPet;
            lifecycle::set_state_for_test(Some(pet(NIGHT_MIN)));
            let night = screen();
            lifecycle::set_state_for_test(Some(pet(None)));
            let plain = screen();
            // The idle sprite alone, on white and on black: a pixel is opaque
            // when it is not the canvas colour on at least one of them.
            let name = engine::anim_files::anim_filename(
                lifecycle::pet_kind(),
                lifecycle::display_anim(),
                0,
            );
            let (mut on_white, mut on_black) = (Canvas::new(), Canvas::new());
            on_black.fill(BLACK);
            sprite_loader::blit_pcx_sim(&mut on_white, &name, 0, PET_AREA_TOP as i32, false);
            sprite_loader::blit_pcx_sim(&mut on_black, &name, 0, PET_AREA_TOP as i32, false);
            let area = boxed(0, 35, 152, 49);
            let (mut opaque, mut transparent_dither) = (0, 0);
            for p in area.points() {
                let (w, b) = (on_white.get(p.x, p.y), on_black.get(p.x, p.y));
                if w != crate::WHITE || b != BLACK {
                    // Opaque (black, red or white): the sprite colour, whatever
                    // the sky. Skip pixels the UI overdraws on the plain screen.
                    let colour = if w != crate::WHITE { w } else { b };
                    if plain.get(p.x, p.y) == colour {
                        opaque += 1;
                        assert_eq!(
                            night.get(p.x, p.y),
                            colour,
                            "dither over the sprite at {p:?}"
                        );
                    }
                } else if night.get(p.x, p.y) != plain.get(p.x, p.y) {
                    transparent_dither += 1;
                }
            }
            assert!(opaque > 100, "the sprite has pixels to check ({opaque})");
            assert!(
                transparent_dither > 500,
                "the dither shows round it ({transparent_dither})"
            );
        }

        #[test]
        fn hibernating_has_no_background() {
            let _guard = lock_test_state();
            let _clear = ClearPet;
            let mut s = pet(NIGHT_MIN);
            s.hibernating = true;
            lifecycle::set_state_for_test(Some(s));
            let night = screen();
            let mut s = pet(None);
            s.hibernating = true;
            lifecycle::set_state_for_test(Some(s));
            let plain = screen();
            same_in(&night, &plain, boxed(0, 35, 152, 75), "pet area");
        }

        #[test]
        fn dirt_pct_shows_floor_items() {
            let _guard = lock_test_state();
            let _clear = ClearPet;
            lifecycle::set_state_for_test(Some(pet(None)));
            let clean = screen();
            lifecycle::debug_set_dirt_pct(95);
            let dirty = screen();
            assert!(differs_in(&clean, &dirty, boxed(0, 100, 152, 10)));
        }

        #[test]
        fn backdrop_draws_nothing_for_a_takeover() {
            let _guard = lock_test_state();
            let _clear = ClearPet;
            lifecycle::set_state_for_test(Some(pet(NIGHT_MIN)));
            let mut c = Canvas::new();
            draw_backdrop(&mut c).unwrap();
            assert!(c.any(boxed(0, 35, 152, 65), BLACK), "the night sky");
            DIABETES_ALERT.start();
            let mut c = Canvas::new();
            draw_backdrop(&mut c).unwrap();
            assert!(
                !c.any(boxed(0, 0, 152, 152), BLACK),
                "no sky behind an alert"
            );
        }
    }
}

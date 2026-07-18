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

pub mod battle;
pub mod battle_view;
pub mod blackhole;
pub mod bornjeweled;
pub mod debug_cheats;
pub mod engine;
pub mod friends;
pub mod friends_view;
pub mod health_view;
pub mod input;
pub mod lifecycle;
pub mod lightsout;
pub mod modal;
pub mod nav;
pub mod nim;
pub mod pet_registry;
pub mod pet_select;
pub mod realm_view;
pub mod settings;
pub mod sprite_loader;
pub mod stat_bar;
pub mod station;
pub mod tictactoe;
pub mod traits_view;
// ── Action feedback toast ────────────────────────────────────────────────────
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering};

use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
pub use nav::{GameNav, Row};

use crate::{BLACK, RED, WHITE, TriColor};

/// Action feedback shown briefly after an action.
#[derive(Clone, Copy, PartialEq, Eq)]
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
    // 14 (TripleBornBonus) removed with the Triple Born mini-game — value
    // left as a gap so persisted toast codes 15+ keep their meaning.
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
    /// Drank something non-alcoholic (Water/Cola) — never touches
    /// `drunk`, unlike `Drink`. Kept as its own variant so the toast
    /// after picking Water/Cola doesn't misleadingly say "+drunk".
    Refreshed = 24,
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
            _ => Self::None,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Toast::None => "",
            Toast::Feed => "-hunger",
            Toast::Heal => "-sick",
            Toast::Sleep => "-tired",
            Toast::Relax => "-drained",
            Toast::Play => "-miserable",
            Toast::Inspired => "+inspired",
            Toast::Hibernate => "hibernating",
            Toast::Wake => "waking up",
            Toast::StationFood => "Food bonus!",
            Toast::StationDrugs => "Heal bonus!",
            Toast::StationInspire => "Inspire bonus!",
            Toast::StationRest => "Sleep bonus!",
            // Dynamic — handled in the renderer.
            Toast::StationCooldown => "",
            Toast::Exercise => "-weight",
            Toast::Medicate => "+medicated",
            Toast::DebugCheat => "cheat applied",
            Toast::Drink => "+drunk",
            Toast::Rehab => "+sober",
            Toast::NewFriend => "new friend!",
            Toast::FriendReunion => "+happy",
            Toast::BattleWon => "won a battle!",
            Toast::BattleLost => "lost a battle",
            Toast::Refreshed => "-drained",
        }
    }
}

/// Toast message index.
static TOAST_MSG: AtomicU8 = AtomicU8::new(0);
/// Whether a toast is currently visible.  Cleared on the first
/// render that occurs at or after `TOAST_STARTED_MS + TOAST_MIN_VISIBLE_MS`.
static TOAST_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Wall-clock millisecond timestamp when the active toast was shown.
/// Stored as the low 32 bits of the uptime-millisecond counter — wraps
/// after ~49 days, far longer than any toast's lifetime.
static TOAST_STARTED_MS: AtomicU32 = AtomicU32::new(0);

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
static DIABETES_ALERT_ACTIVE: AtomicBool = AtomicBool::new(false);
static DIABETES_ALERT_STARTED_MS: AtomicU32 = AtomicU32::new(0);
/// Minimum wall-clock visibility for the diabetes alert screen.
const DIABETES_ALERT_MIN_VISIBLE_MS: u32 = 3000;

/// Show the full-screen "now diabetic" alert, taking over the whole
/// display (icons, pet, everything) for at least
/// `DIABETES_ALERT_MIN_VISIBLE_MS`. Called once, the instant diabetes
/// triggers — see `lifecycle::check_diabetes_onset`.
pub fn show_diabetes_alert() {
    DIABETES_ALERT_STARTED_MS.store(now_ms_u32(), Ordering::Relaxed);
    DIABETES_ALERT_ACTIVE.store(true, Ordering::Relaxed);
    #[cfg(feature = "embassy-base")]
    crate::TOAST_SIGNAL.signal(());
}

/// Low 32 bits of the current uptime in milliseconds.  Cross-platform
/// wrapper so `mod.rs` compiles on both firmware (`embassy_time`) and
/// simulator (`lifecycle::sim_elapsed_ms`).
fn now_ms_u32() -> u32 {
    #[cfg(feature = "embassy-base")]
    {
        embassy_time::Instant::now().as_millis() as u32
    }
    #[cfg(all(feature = "simulator", not(feature = "embassy-base")))]
    {
        lifecycle::sim_elapsed_ms() as u32
    }
    #[cfg(not(any(feature = "embassy-base", feature = "simulator")))]
    {
        0
    }
}

/// Show a feedback toast.  Becomes visible on the next display
/// refresh and stays visible until the first refresh at or after
/// `TOAST_MIN_VISIBLE_MS` elapsed.  Fires `TOAST_SIGNAL` so the
/// display loop wakes up immediately even if no other event would
/// have triggered a redraw (e.g. an NFC station bonus arriving while
/// the display task is parked).
pub fn show_toast(toast: Toast) {
    TOAST_MSG.store(toast as u8, Ordering::Relaxed);
    TOAST_STARTED_MS.store(now_ms_u32(), Ordering::Relaxed);
    TOAST_ACTIVE.store(true, Ordering::Relaxed);
    #[cfg(feature = "embassy-base")]
    crate::TOAST_SIGNAL.signal(());
}

/// True when the game screen is showing a red status message — an active
/// toast, or the "gone / new egg" prompt.  The display loop uses this to run
/// a genuine full (tri-color) refresh instead of the fast delta refresh: red
/// ink under-drives on the non-inverting delta LUT (LUT2), so red text only
/// seats properly on the full-waveform path.  `partial_idle` upstream keeps
/// this from re-flashing once the message is drawn.
pub fn status_wants_full_refresh() -> bool {
    TOAST_ACTIVE.load(Ordering::Relaxed)
        || DIABETES_ALERT_ACTIVE.load(Ordering::Relaxed)
        || lifecycle::display_anim() == engine::DisplayAnim::Gone
        || lifecycle::is_diabetic_unmedicated()
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

/// First display row of the pet/sprite area.
pub const PET_AREA_TOP: usize = SEP_TOP as usize + 1;

// ── Public entry point
// ────────────────────────────────────────────────────────

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
    use embedded_graphics::mono_font::MonoTextStyle;
    use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
    use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
    use engine::to_display::DisplayAnim;

    // `nav` only drives the simulator's in-line icon blit (firmware
    // does that work in the async pre-pass `render()` instead).  Suppress
    // the unused-variable warning for non-simulator builds.
    #[cfg(not(feature = "simulator"))]
    let _ = &nav;

    let centered = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    // Red style for pet status prompts (the "gone / new egg" screen) so they
    // stand out from the black countdown timer / labels.
    let font_red = MonoTextStyle::new(&FONT_7X13_BOLD, RED);

    // ── Full-screen takeover screens ───────────────────────────────────
    // Diabetes onset alert takes priority over everything else, including
    // mini-games and modals — a rare, one-shot event worth interrupting
    // whatever's on screen for.
    if DIABETES_ALERT_ACTIVE.load(Ordering::Relaxed) {
        Rectangle::new(Point::new(0, 0), Size::new(152, 152))
            .into_styled(PrimitiveStyle::with_fill(WHITE))
            .draw(display)?;
        Text::with_text_style("TYPE 2", Point::new(76, 56), font, centered).draw(display)?;
        Text::with_text_style("DIABETES", Point::new(76, 74), font, centered).draw(display)?;
        Text::with_text_style(
            "Give medication soon",
            Point::new(76, 100),
            font_red,
            centered,
        )
        .draw(display)?;

        let elapsed = now_ms_u32().wrapping_sub(DIABETES_ALERT_STARTED_MS.load(Ordering::Relaxed));
        if elapsed >= DIABETES_ALERT_MIN_VISIBLE_MS {
            DIABETES_ALERT_ACTIVE.store(false, Ordering::Relaxed);
        }
        return Ok(());
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
    if nim::is_active() {
        return nim::draw(display);
    }
    if bornjeweled::is_active() {
        return bornjeweled::draw(display);
    }

    // Battery icon — top-right.
    #[cfg(feature = "embassy-base")]
    {
        let pct = crate::fw::battery::read_pct();
        crate::draw_battery_icon(display, 128, 2, pct)?;
    }

    // HEX balance — top-right, below the battery icon + menu icon row
    // (y=36 clears the icons; y=16 overlapped them).
    // Hidden entirely when money mode is disabled for this pet.
    if lifecycle::money_enabled() {
        let right_style = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Right)
            .build();
        let mut money_buf: heapless::String<12> = heapless::String::new();
        let _ = core::fmt::Write::write_fmt(
            &mut money_buf,
            format_args!("{} HEX", lifecycle::money()),
        );
        Text::with_text_style(money_buf.as_str(), Point::new(150, 16), font, right_style)
            .draw(display)?;
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

        use engine::anim_files;
        // Per-animation start delta: when the anim id changes we pin
        // the wall-clock origin so the new anim begins at frame 0.
        // The free-running clock keeps ticking — we just subtract.
        static LAST_ANIM_ID: AtomicU8 = AtomicU8::new(0xFF);
        static ANIM_START_MS: AtomicU64 = AtomicU64::new(0);

        if !lifecycle::is_started() {
            sprite_loader::blit_pcx_sim(display, &anim_files::start_screen_filename(), 0, 0);
        } else {
            let kind = lifecycle::pet_kind();
            let anim = lifecycle::display_anim();
            let count = anim_files::frame_count(kind, anim);
            if count > 0 {
                let elapsed_ms = lifecycle::sim_elapsed_ms();
                let id = anim_files::anim_id_for(anim);
                if LAST_ANIM_ID.load(Ordering::Relaxed) != id {
                    LAST_ANIM_ID.store(id, Ordering::Relaxed);
                    ANIM_START_MS.store(elapsed_ms, Ordering::Relaxed);
                }
                let delta_ms = elapsed_ms.saturating_sub(ANIM_START_MS.load(Ordering::Relaxed));
                // 10 s per frame — matches the firmware default sprite
                // tick interval.  Hatching clamps to the last frame.
                let raw = (delta_ms / 10_000) as u32;
                let frame = if matches!(anim, engine::DisplayAnim::Hatching { .. }) {
                    raw.min(count.saturating_sub(1) as u32) as u8
                } else {
                    (raw % count as u32) as u8
                };
                let name = anim_files::anim_filename(kind, anim, frame);
                sprite_loader::blit_pcx_sim(display, &name, 0, PET_AREA_TOP as i32);
            }
        }
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
        // Farewell animation blitted by embassy.rs if available.
        if sprite_loader::frame_count() == 0 {
            Text::with_text_style("Your pet has left", Point::new(76, 50), font_red, centered)
                .draw(display)?;
        }
        Text::with_text_style("Press Fire", Point::new(76, 90), font_red, centered).draw(display)?;
        Text::with_text_style("for a new egg", Point::new(76, 106), font_red, centered)
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
        let (row_kind, col) = match slot {
            0 | 1 => (Row::Top, slot),
            6 => (Row::Top, 2), // Exercise — added after slots 0-5 shipped.
            7 => (Row::Top, 3), // Drink — added after slot 6 shipped.
            _ => (Row::Bottom, slot - 2),
        };
        let cy = if matches!(row_kind, Row::Top) {
            TOP_CY
        } else {
            BOT_CY
        };
        let cx = ICON_CX[col as usize];
        let selected = nav.row == row_kind && nav.col == col;
        let name = engine::anim_files::menu_icon_filename(slot, selected);
        sprite_loader::blit_pcx_sim(display, &name, cx - 13, cy - 13);
    }

    Rectangle::new(Point::new(0, SEP_TOP), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Pet area: sprite blitted by embassy.rs, or fallback.
    if sprite_loader::frame_count() == 0 {
        Text::with_text_style(
            "No sprites on flash",
            Point::new(76, (SEP_TOP + SEP_BOT) / 2),
            font,
            centered,
        )
        .draw(display)?;
    }

    Rectangle::new(Point::new(0, SEP_BOT), Size::new(152, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Action feedback toast — shown until the first render at or
    // after TOAST_MIN_VISIBLE_MS has elapsed since show_toast.
    if TOAST_ACTIVE.load(Ordering::Relaxed) {
        let toast = Toast::from_u8(TOAST_MSG.load(Ordering::Relaxed));
        // Dynamic toasts (station cooldown) format their text at draw
        // time from a small atomic; everything else uses the static
        // message table.
        let mut dyn_buf: heapless::String<24> = heapless::String::new();
        let msg: &str = if let Toast::StationCooldown = toast {
            let secs = STATION_COOLDOWN_SECS.load(Ordering::Relaxed);
            let m = secs / 60;
            let s = secs % 60;
            let _ = core::fmt::Write::write_fmt(&mut dyn_buf, format_args!("wait {}:{:02}", m, s));
            dyn_buf.as_str()
        } else {
            toast.message()
        };
        if !msg.is_empty() {
            use embedded_graphics::mono_font::MonoTextStyle;
            use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
            use embedded_graphics::text::{Baseline, Text, TextStyleBuilder};
            let style = TextStyleBuilder::new().baseline(Baseline::Top).build();
            Text::with_text_style(
                msg,
                Point::new(2, SEP_TOP + 2),
                // Red so the action feedback (-hunger / -sick / +inspired /
                // bonus) pops against the black UI.
                MonoTextStyle::new(&FONT_7X13_BOLD, RED),
                style,
            )
            .draw(display)?;
        }
        // Wrapping subtraction handles the once-per-49-days uptime
        // wraparound correctly: if start was before wrap and now after,
        // `now - start` still yields the true elapsed delta (mod 2^32).
        let elapsed = now_ms_u32().wrapping_sub(TOAST_STARTED_MS.load(Ordering::Relaxed));
        if elapsed >= TOAST_MIN_VISIBLE_MS {
            TOAST_ACTIVE.store(false, Ordering::Relaxed);
        }
    }

    // Persistent "needs meds" banner — diabetic and medication has
    // lapsed. Deliberately independent of the toast timer above: unlike
    // a toast, this needs to stay visible for as long as the condition
    // holds, not just flash briefly. Drawn on the opposite side from
    // the toast (top-right vs top-left) so the two can't collide if
    // both are showing at once. There's no dedicated sprite/animation
    // for this state — see the note in `engine::to_display` for why.
    if lifecycle::is_diabetic_unmedicated() {
        use embedded_graphics::mono_font::MonoTextStyle;
        use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
        use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
        let style = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Right)
            .build();
        Text::with_text_style(
            "NEEDS MEDS",
            Point::new(150, SEP_TOP + 2),
            MonoTextStyle::new(&FONT_7X13_BOLD, RED),
            style,
        )
        .draw(display)?;
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
#[cfg(feature = "embassy-base")]
pub async fn render(display: &mut crate::fw::epd::EpdGfx<'_>, sprite_frame: u8) {
    use engine::anim_files;
    use engine::to_display::DisplayAnim;

    use crate::fw::fat12;

    if lifecycle::is_started() {
        lifecycle::cycle();

        // After hatching completes, prompt the player to name their pet.
        if lifecycle::take_naming_pending() {
            let seed = lifecycle::now_tick();
            let default = lifecycle::random_default_name(seed);
            crate::text_entry::begin(default.as_bytes(), 12, on_pet_named, "Name your Pet");
        }
    }

    // Blit sprite from flash.
    let mut has_sprite = false;

    if !lifecycle::is_started() {
        // Start screen: full 152×152 graphic at origin.
        let start_name = anim_files::start_screen_filename();
        if let Ok(file) = fat12::find_file(&start_name).await {
            sprite_loader::blit_file(display, &file, 0, 0).await;
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
                sprite_loader::blit_file(display, &file, 0, PET_AREA_TOP as i32).await;
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
            let (top_row, col) = match slot {
                0 | 1 => (true, slot),
                6 => (true, 2), // Exercise — added after slots 0-5 shipped.
                7 => (true, 3), // Drink — added after slot 6 shipped.
                _ => (false, slot - 2),
            };
            let cy = if top_row { TOP_CY } else { BOT_CY };
            let cx = ICON_CX[col as usize];
            let row_kind = if top_row { Row::Top } else { Row::Bottom };
            let selected = nav.row == row_kind && nav.col == col;
            let name = anim_files::menu_icon_filename(slot, selected);
            if let Ok(file) = fat12::find_file(&name).await {
                sprite_loader::blit_file(display, &file, cx - 13, cy - 13).await;
            }
        }
    }

    // Debug: show animation name when no artwork loaded.
    if !has_sprite && lifecycle::is_started() {
        use embedded_graphics::mono_font::MonoTextStyle;
        use embedded_graphics::mono_font::iso_8859_1::FONT_7X13;
        use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};
        use ssd1675::graphics::Color;

        let anim = lifecycle::display_anim();
        let anim_name: &str = match anim {
            DisplayAnim::Gone => "GONE",
            DisplayAnim::Hibernating => "HIBERNATE",
            DisplayAnim::Hatching { .. } => "HATCHING",
            DisplayAnim::Feeding => "FEEDING",
            DisplayAnim::Healing => "HEALING",
            DisplayAnim::Relaxing => "RELAXING",
            DisplayAnim::Playing => "PLAYING",
            DisplayAnim::Sleeping => "SLEEPING",
            DisplayAnim::Exercising => "EXERCISING",
            DisplayAnim::Medicating => "MEDICATING",
            DisplayAnim::Drinking => "DRINKING",
            DisplayAnim::Ozempic => "OZEMPIC",
            DisplayAnim::Rehab => "REHAB",
            DisplayAnim::Leaving { .. } => "LEAVING",
            DisplayAnim::CriticalSick => "CRIT:SICK",
            DisplayAnim::CriticalTired => "CRIT:TIRED",
            DisplayAnim::CriticalHungry => "CRIT:HUNGRY",
            DisplayAnim::CriticalDrained => "CRIT:DRAINED",
            DisplayAnim::WarningSick => "WARN:SICK",
            DisplayAnim::WarningTired => "WARN:TIRED",
            DisplayAnim::WarningHungry => "WARN:HUNGRY",
            DisplayAnim::WarningDrained => "WARN:DRAINED",
            DisplayAnim::WarningMiserable => "WARN:MISER",
            DisplayAnim::Happy => "HAPPY",
            DisplayAnim::Idle => "IDLE",
        };
        let style = TextStyleBuilder::new()
            .baseline(Baseline::Top)
            .alignment(Alignment::Right)
            .build();
        let _ = Text::with_text_style(
            anim_name,
            Point::new(150, 36),
            MonoTextStyle::new(&FONT_7X13, Color::Black),
            style,
        )
        .draw(display);
    }

    if lifecycle::is_started() {
        lifecycle::save_if_needed().await;
    }
}

/// Text entry callback: player has submitted a pet name.
#[cfg(feature = "embassy-base")]
fn on_pet_named(name: &[u8]) {
    lifecycle::set_pet_name(name);
}

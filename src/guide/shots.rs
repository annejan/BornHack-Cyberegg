//! The booklet's screen shots (`guide shot NAME`). A shot is a list of steps
//! played on a fresh game against a fake clock, and an expectation that
//! checks the game is in the state the shot claims to show. A shot whose
//! expectation fails is an error, so a picture can never show something else
//! than its name says.
//!
//! The game's state is global: run one shot per process.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use crate::game::engine::doses::{Reason, Refusal};
use crate::game::engine::sim;
use crate::game::engine::scene::{FLOOR_ITEMS, Sky};
use crate::game::engine::status::{Condition, Word};
use crate::game::engine::thresholds::{HATCHING_TICKS, Mode, badge_preset};
use crate::game::engine::{DebugPoke, DisplayAnim, FoodKind, PetKind, PetLine};
use crate::game::modal::{self, ModalKind};
use crate::game::stats_pages::Page;
use crate::game::{
    NAME_PROMPT, Toast, current_toast, diary_view, lifecycle, pet_select, prompt_for_name,
    reason_toast,
};
use crate::menu::{ButtonId, dispatch_press};
use crate::platform::{GUIDE_NOON_UNIX, fake_uptime_ms, set_fake_unix, set_fake_uptime_ms};
use crate::sim_support;
use crate::text_entry;

/// The instant every shot starts at: 12:00 local, so the sky is a day sky
/// whatever the host's clock says (the same instant the lib tests use).
pub const SHOT_NOON_UNIX: u32 = GUIDE_NOON_UNIX;
/// 23:00 local the same day: a night sky.
pub const SHOT_NIGHT_UNIX: u32 = GUIDE_NOON_UNIX + 11 * 3600;

/// The name of the shot that mirrors an old badge (see `run`).
const REALM_SHOT: &str = "to-the-realm";

/// The pet kind the Jellyfish had, before its sprites were removed.
const JELLYFISH: PetKind = PetKind(8);

/// The pet's name in every shot, whatever kind it is. The guide's mascot (the
/// hero, `PetKind::roster()[0]`) is called the same.
pub const HERO_NAME: &str = "Bob";

/// Shots that show the same pet as an earlier shot, as `(shot, leader)`: the
/// book puts them side by side (day and night, starving and wasted ...) or in
/// one story (the egg, its naming, the hatched pet), and a pair of pictures of
/// two different pets would read as two different pets.
const WITH: &[(&str, &str)] = &[
    ("naming", "egg"),
    ("hatched", "egg"),
    // The sky test compares day and night with content: the same pet.
    ("day", "content"),
    ("night", "content"),
    // No pet on the pick screens; pairing keeps the rotation of the rest.
    ("select-hard", "select-mode"),
    ("wasted", "starving"),
    ("stats-body", "stats-needs"),
    ("refusal-water", "menu-drink"),
    ("diary", "cause-toast"),
    ("demo-help", "demo-note"),
    ("gone", "leaving"),
    ("asleep", "menu-rest"),
];

/// Shots that must show one given pet, as `(shot, pet prefix)`. None needs it
/// now: every shot's expectation holds for every roster pet.
const PET_OF: &[(&str, u8)] = &[];

/// The pet a shot shows, taken from the roster in rotation: every shot or
/// pair of shots in `SHOTS` order moves one pet on, so neighbours differ and
/// each pet comes back often. Fixed by the shot's place alone, so the PDF is
/// the same at every build.
pub fn pet_for(name: &str, roster: &[PetKind]) -> PetKind {
    assert!(!roster.is_empty(), "the roster is empty");
    if let Some(&(_, id)) = PET_OF.iter().find(|(n, _)| *n == name) {
        return roster
            .iter()
            .copied()
            .find(|k| k.0 == id)
            .unwrap_or_else(|| panic!("shot {name}: pet {id} is not in the roster"));
    }
    let lead = WITH.iter().find(|(n, _)| *n == name).map_or(name, |&(_, l)| l);
    let slot = SHOTS
        .iter()
        .filter(|s| !WITH.iter().any(|(n, _)| *n == s.name))
        .position(|s| s.name == lead)
        .unwrap_or_else(|| panic!("no shot named {name}"));
    roster[slot % roster.len()]
}

/// The kind `Start` begins with: set by `run` from `pet_for`; `NO_PET` before
/// (the first roster pet).
static SHOT_PET: AtomicU8 = AtomicU8::new(NO_PET);
const NO_PET: u8 = u8::MAX;

fn shot_pet() -> PetKind {
    match SHOT_PET.load(Ordering::Relaxed) {
        NO_PET => PetKind::roster()[0],
        id => PetKind(id),
    }
}

/// One game tick on the fake clock, in milliseconds.
const TICK_MS: u64 = 10_000;
/// Most half-hour skips a shot waits for the pet to leave, then to be gone.
const MAX_SKIPS: u32 = 1000;
/// Length of one waiting skip, in minutes.
const SKIP_MIN: u32 = 30;
/// Minutes between two renewals of the player's help (under the 30 minute pause).
const HOLD_OFF_MIN: u32 = 20;
/// Most pizzas a shot feeds before a cause shows.
const MAX_PIZZAS: u32 = 12;

#[derive(Clone, Copy)]
pub enum Step {
    Press(ButtonId),
    Start {
        money: bool,
        hard: bool,
    },
    Poke(DebugPoke),
    Skip(u32),
    Ticks(u32),
    Open(ModalKind),
    /// A loop or a shared set-up: the steps that cannot be a plain list.
    Do(fn()),
}

pub struct Shot {
    pub name: &'static str,
    pub steps: &'static [Step],
    pub expect: fn() -> bool,
}

pub struct Summary {
    pub w: u32,
    pub h: u32,
    pub non_white: usize,
}

use Step::{Do, Open, Poke, Press, Start};

// ── Shared set-ups ──────────────────────────────────────────────────────────

fn advance_ticks(n: u32) {
    set_fake_uptime_ms(fake_uptime_ms().unwrap_or(0) + n as u64 * TICK_MS);
}

fn play(steps: &[Step]) {
    for step in steps {
        match *step {
            Press(b) => dispatch_press(b),
            Start { money, hard } => {
                lifecycle::start_new_game(shot_pet(), money, hard);
                lifecycle::set_pet_name(HERO_NAME.as_bytes());
            }
            Poke(p) => lifecycle::debug_poke(p)
                .unwrap_or_else(|why| panic!("poke refused: {}", why.text())),
            Step::Skip(m) => lifecycle::debug_skip_minutes(m),
            Step::Ticks(n) => advance_ticks(n),
            Open(k) => modal::open(k),
            Do(f) => f(),
        }
        let _ = lifecycle::cycle();
    }
}

/// Wait out the egg: its countdown, and one tick more.
fn hatch() {
    advance_ticks(HATCHING_TICKS() as u32 + 1);
}

static HATCHED: &[Step] = &[
    Start {
        money: true,
        hard: false,
    },
    Do(hatch),
];
static CONTENT: &[Step] = &[Do(hatched), Poke(DebugPoke::Comfort)];
static STARVING: &[Step] = &[Do(content), Poke(DebugPoke::Hungry)];
static STATS_NEEDS: &[Step] = &[Do(starving), Open(ModalKind::Stats), Press(ButtonId::Fire)];
static CAUSE_TOAST: &[Step] = &[Do(content), Do(provoke_cause)];
static LEAVING: &[Step] = &[
    Do(content),
    Poke(DebugPoke::Hungry),
    Poke(DebugPoke::Thirsty),
    Do(wait_for_leaving),
];
static DEBUG_MENU: &[Step] = &[
    Do(content),
    Press(ButtonId::Up),
    Press(ButtonId::Up),
    Press(ButtonId::Down),
    Press(ButtonId::Down),
    Press(ButtonId::Left),
    Press(ButtonId::Right),
    Press(ButtonId::Left),
    Press(ButtonId::Right),
    Press(ButtonId::Fire),
];

/// Night: the fake clock moves to 23:00 (the uptime keeps running on it).
fn set_night() {
    set_fake_unix(Some(SHOT_NIGHT_UNIX));
}

/// A filthy floor: every item lies there.
fn very_dirty() {
    lifecycle::debug_set_dirt_pct(96);
}

/// A new Jellyfish, hatched (the badge it lives on has lost its sprites).
fn new_jellyfish() {
    lifecycle::start_new_game(JELLYFISH, false, false);
    lifecycle::set_pet_name(HERO_NAME.as_bytes());
}

/// What the badge does at boot for a pet whose sprites are gone.
fn retire() {
    assert_eq!(
        lifecycle::retire_hidden_pet(),
        lifecycle::Retired::ToRealm,
        "the jellyfish did not move to the Realm"
    );
}

fn bg() -> Option<crate::game::engine::scene::BackgroundState> {
    lifecycle::background_state()
}

fn hatched() {
    play(HATCHED);
}
fn content() {
    play(CONTENT);
}
fn starving() {
    play(STARVING);
}
fn stats_needs() {
    play(STATS_NEEDS);
}
fn cause_toast() {
    play(CAUSE_TOAST);
}
/// A made-up friend challenges the pet (the chapter on battles shows the
/// screen the player then sees).
fn bram_challenges() {
    crate::game::battle_link::inject_challenge_for_shot("Bram", 1);
}
fn leaving() {
    play(LEAVING);
}
fn debug_menu() {
    play(DEBUG_MENU);
}

/// What the firmware's render loop does once the egg has hatched.
fn name_prompt() {
    assert!(prompt_for_name(), "the egg has not hatched");
}

/// The naming prompt is up, with its title and the default name filled in.
fn naming_prompt_is_up() -> bool {
    let default = lifecycle::random_default_name(lifecycle::now_tick());
    text_entry::active_entry()
        .is_some_and(|(title, text)| title == NAME_PROMPT && text == default.as_bytes())
}

/// Rows of the open menu that carry the risk mark, counted by a step so the
/// expectation only reads.
static RISK_ROWS: AtomicUsize = AtomicUsize::new(0);

fn count_risk_rows() {
    let stats = lifecycle::cycle().expect("a game is running");
    RISK_ROWS.store(modal::risk_row_count(&stats), Ordering::Relaxed);
}

fn drink_water() {
    assert!(
        lifecycle::drink(crate::game::engine::DrinkKind::Water),
        "the pet would not drink"
    );
}

fn sleep() {
    assert!(lifecycle::sleep(), "the pet would not go to sleep");
}

fn hibernate() {
    assert!(lifecycle::hibernate(), "the pet would not hibernate");
}

/// Feed pizza until the pet is told why it feels bad (a cause toast).
fn provoke_cause() {
    for _ in 0..MAX_PIZZAS {
        if current_toast() == Some(Toast::Cause) {
            return;
        }
        lifecycle::feed(FoodKind::Pizza);
        // Let the meal finish, then look.
        advance_ticks(10);
        let _ = lifecycle::cycle();
    }
    assert!(toast_is(Toast::Cause), "no cause ever showed");
}

fn skip_until(done: fn() -> bool) {
    for _ in 0..MAX_SKIPS {
        if done() {
            return;
        }
        lifecycle::debug_skip_minutes(SKIP_MIN);
        let _ = lifecycle::cycle();
    }
    assert!(done(), "the wait for the pet never ended");
}

fn is_leaving() -> bool {
    matches!(lifecycle::display_anim(), DisplayAnim::Leaving { .. })
}

fn is_gone() -> bool {
    lifecycle::display_anim() == DisplayAnim::Gone
}

fn wait_for_leaving() {
    skip_until(is_leaving);
}

fn wait_for_gone() {
    skip_until(is_gone);
}

static GONE: &[Step] = &[Do(leaving), Do(wait_for_gone)];

/// Demo mode on, with the pet in fair shape (the comfort poke ran first).
fn demo_on() {
    lifecycle::toggle_demo().unwrap_or_else(|why| panic!("demo refused: {}", why.text()));
}

fn is_note() -> bool {
    matches!(lifecycle::pet_line(), Some(PetLine::Note(_)))
}

fn is_help() -> bool {
    matches!(lifecycle::pet_line(), Some(PetLine::Help))
}

/// Let the demo pet go on a minute at a time until `done`.
fn minutes_until(done: fn() -> bool) {
    for _ in 0..MAX_SKIPS {
        if done() {
            return;
        }
        lifecycle::debug_skip_minutes(1);
        let _ = lifecycle::cycle();
    }
    assert!(done(), "the demo pet never got there");
}

fn wait_for_note() {
    minutes_until(is_note);
}

static DEMO_NOTE: &[Step] = &[
    Do(content),
    Do(demo_on),
    Poke(DebugPoke::Hungry),
    Do(wait_for_note),
];

fn demo_note() {
    play(DEMO_NOTE);
}

/// A player's help pauses the demo pet for `DEMO_PAUSE_MIN` minutes. A
/// finished mini-game is such help and costs the pet nothing, so the shot
/// renews it just before it ends: the neglected pet is left to its troubles
/// until its health drops under the help line.
fn wait_for_help() {
    let game = crate::game::engine::MiniGame::ALL[0];
    for _ in 0..MAX_SKIPS {
        if is_help() {
            return;
        }
        lifecycle::award_inspiration(game, 0);
        lifecycle::debug_skip_minutes(HOLD_OFF_MIN);
        let _ = lifecycle::cycle();
    }
    assert!(is_help(), "the neglected pet never called for help");
}

static DEMO_HELP: &[Step] = &[
    Do(content),
    Do(demo_on),
    Poke(DebugPoke::Injured),
    Poke(DebugPoke::Drunk),
    Poke(DebugPoke::Hungry),
    Poke(DebugPoke::Thirsty),
    Poke(DebugPoke::Dirty),
    Do(wait_for_help),
];

fn demo_help() {
    play(DEMO_HELP);
}

fn gone() {
    play(GONE);
}

// ── Expectations ────────────────────────────────────────────────────────────

/// The status line under the pet; `None` while a demo note or help line has
/// its place (the shots start with demo off).
fn line() -> Option<crate::game::engine::status::StatusLine> {
    match lifecycle::pet_line()? {
        PetLine::Status(l) => Some(l),
        _ => None,
    }
}

fn first_is(w: Word) -> bool {
    line().is_some_and(|l| l.first == w)
}

fn has_word(w: Word) -> bool {
    line().is_some_and(|l| l.first == w || l.second == Some(w))
}

/// Whether the pet was hung over when `note_hung_over` ran. A step reads the
/// state (`cycle()` advances it), so the expectation only reads this flag.
static HUNG_OVER: AtomicBool = AtomicBool::new(false);

fn note_hung_over() {
    let has = lifecycle::cycle().is_some_and(|s| s.conditions.has(Condition::HungOver));
    HUNG_OVER.store(has, Ordering::Relaxed);
}

fn menu_is(kind: ModalKind) -> bool {
    modal::open_kind() == kind
}

fn toast_is(t: Toast) -> bool {
    current_toast() == Some(t)
}

macro_rules! shot {
    ($name:expr, [$($step:expr),* $(,)?], $expect:expr) => {
        Shot { name: $name, steps: &[$($step),*], expect: $expect }
    };
}

use ButtonId::{Down, Fire, Right};

pub static SHOTS: &[Shot] = &[
    shot!("start", [], || !lifecycle::is_started()),
    shot!("select-pet", [Press(Fire)], || {
        pet_select::is_active() && pet_select::phase() == pet_select::Phase::Pet
    }),
    shot!("select-mode", [Press(Fire), Press(Fire)], || {
        pet_select::is_active() && pet_select::phase() == pet_select::Phase::Money
    }),
    // The money question with the cursor on its third row, Hard mode (US).
    shot!(
        "select-hard",
        [
            Press(Fire),
            Press(Fire),
            Press(ButtonId::Down),
            Press(ButtonId::Down)
        ],
        || {
            pet_select::is_active()
                && pet_select::phase() == pet_select::Phase::Money
                && pet_select::selection() == 2
        }
    ),
    // The badge asks for the name once the egg has hatched (the firmware's
    // render loop does it; there is none here), so the shot plays that too.
    // It starts the game itself: the pick screens would choose the first pet.
    shot!(
        "naming",
        [
            Start {
                money: true,
                hard: false
            },
            Do(hatch),
            Do(name_prompt)
        ],
        naming_prompt_is_up
    ),
    shot!(
        "egg",
        [Start {
            money: true,
            hard: false
        }],
        || {
            matches!(
                lifecycle::display_anim(),
                DisplayAnim::Hatching { ticks_remaining } if ticks_remaining > 1
            )
        }
    ),
    shot!("hatched", [Do(hatched)], || {
        lifecycle::is_started()
            && !matches!(lifecycle::display_anim(), DisplayAnim::Hatching { .. })
    }),
    shot!("content", [Do(content)], || first_is(Word::Content)),
    shot!("starving", [Do(starving)], || {
        line().is_some_and(|l| l.first == Word::Starving && l.danger)
    }),
    shot!(
        "two-words",
        [Do(starving), Poke(DebugPoke::Thirsty)],
        || line().is_some_and(|l| l.second.is_some())
    ),
    shot!(
        "asleep",
        [Do(content), Poke(DebugPoke::Tired), Do(sleep)],
        || { first_is(Word::Asleep) }
    ),
    shot!(
        "hung-over",
        [Do(content), Poke(DebugPoke::HungOver), Do(note_hung_over)],
        || { HUNG_OVER.load(Ordering::Relaxed) }
    ),
    shot!("injured", [Do(content), Poke(DebugPoke::Injured)], || {
        first_is(Word::Injured)
    }),
    shot!("wasted", [Do(content), Poke(DebugPoke::Drunk)], || {
        line().is_some_and(|l| l.first == Word::Wasted && l.danger)
    }),
    // The onset alert takes the whole screen for a few seconds. These two let
    // it lapse (one tick on the fake clock; the draw drops an expired alert)
    // so the screen shows the status line the word lives on.
    shot!(
        "no-insulin",
        [Do(content), Poke(DebugPoke::Diabetic), Step::Ticks(1)],
        || { has_word(Word::NoInsulin) }
    ),
    shot!(
        "needs-rehab",
        [Do(content), Poke(DebugPoke::Dependent), Step::Ticks(1)],
        || { has_word(Word::NeedsRehab) }
    ),
    // The alert itself: the onset cycle runs on the fake clock and the clock
    // does not move, so the alert is still up when the screen is drawn.
    shot!(
        "alert-diabetes",
        [Do(content), Poke(DebugPoke::Diabetic)],
        crate::game::diabetes_alert_active
    ),
    shot!("stats-needs", [Do(stats_needs)], || {
        modal::stats_view_open() && modal::stats_page() == Page::Needs
    }),
    shot!("stats-body", [Do(stats_needs), Press(Right)], || {
        modal::stats_view_open() && modal::stats_page() == Page::Body
    }),
    shot!(
        "hint",
        [
            Do(starving),
            Open(ModalKind::Stats),
            Press(Down),
            Press(Fire)
        ],
        modal::hint_view_open
    ),
    shot!("cause-toast", [Do(cause_toast)], || toast_is(Toast::Cause)),
    shot!(
        "diary",
        [
            Do(cause_toast),
            Open(ModalKind::Stats),
            Press(Down),
            Press(Down),
            Press(Fire)
        ],
        diary_view::is_active
    ),
    shot!("menu-feed", [Do(content), Open(ModalKind::Feed)], || {
        menu_is(ModalKind::Feed)
    }),
    shot!("menu-drink", [Do(content), Open(ModalKind::Drink)], || {
        menu_is(ModalKind::Drink)
    }),
    shot!("menu-rest", [Do(content), Open(ModalKind::Rest)], || {
        menu_is(ModalKind::Rest)
    }),
    shot!("menu-heal", [Do(content), Open(ModalKind::Heal)], || {
        menu_is(ModalKind::Heal)
    }),
    shot!("menu-play", [Do(content), Open(ModalKind::Play)], || {
        menu_is(ModalKind::Play)
    }),
    shot!("menu-stats", [Do(content), Open(ModalKind::Stats)], || {
        menu_is(ModalKind::Stats)
    }),
    shot!(
        "menu-feed-hard",
        [
            Start {
                money: true,
                hard: true
            },
            Do(hatch),
            Poke(DebugPoke::Comfort),
            Open(ModalKind::Feed)
        ],
        || menu_is(ModalKind::Feed) && lifecycle::hard_mode()
    ),
    shot!(
        "risk-marker",
        [
            Do(content),
            Poke(DebugPoke::Stuffed),
            Open(ModalKind::Feed),
            Do(count_risk_rows)
        ],
        || menu_is(ModalKind::Feed) && RISK_ROWS.load(Ordering::Relaxed) > 0
    ),
    shot!(
        "refusal-water",
        [
            Do(content),
            Open(ModalKind::Drink),
            // The pet drinks while the menu is open: the cursor cannot reach
            // a shut row, so the row under it goes shut and Fire says why.
            Do(drink_water),
            Step::Ticks(3),
            Press(Fire)
        ],
        || reason_toast() == Some(Reason::Refused(Refusal::NotThirsty))
    ),
    shot!(
        "hibernating",
        [Do(content), Do(hibernate)],
        lifecycle::is_hibernating
    ),
    shot!("leaving", [Do(leaving)], is_leaving),
    shot!("gone", [Do(gone)], is_gone),
    shot!("demo-note", [Do(demo_note)], || {
        is_note() && lifecycle::is_demo()
    }),
    shot!("demo-help", [Do(demo_help)], || {
        is_help() && lifecycle::is_demo()
    }),
    shot!("debug-menu", [Do(debug_menu)], || menu_is(ModalKind::Debug)),
    shot!("engine-info", [Do(debug_menu), Press(Fire)], || {
        toast_is(Toast::DebugInfo)
    }),
    shot!(
        "pre-battle",
        [Do(content), Do(bram_challenges)],
        crate::game::battle_view::is_active
    ),
    // The sky and the floor (chapter 3). `content` is already a noon shot.
    shot!("day", [Do(content)], || {
        bg().is_some_and(|b| b.sky == Sky::Day)
    }),
    shot!("night", [Do(content), Do(set_night)], || {
        bg().is_some_and(|b| b.sky == Sky::Night)
    }),
    shot!("floor", [Do(content), Do(very_dirty)], || {
        bg().is_some_and(|b| b.floor as usize == FLOOR_ITEMS)
    }),
    // An old badge: PETS.CFG still lists the Jellyfish, its sprites are gone.
    shot!(
        "to-the-realm",
        [Do(new_jellyfish), Do(hatch), Do(retire)],
        || is_gone() && lifecycle::is_hidden_kind(lifecycle::pet_kind())
    ),
];

/// The clock and the numbers every shot starts from.
fn set_up() {
    set_fake_uptime_ms(0);
    set_fake_unix(Some(SHOT_NOON_UNIX));
    // The badge's numbers, never the bare simulator preset.
    sim::install_preset(badge_preset(Mode::Classic), Mode::Classic, false);
}

/// Any screen, not only the game's: a fresh badge at noon, the buttons in
/// `keys` pressed in order (`U` `D` `L` `R` up/down/left/right, `F` fire,
/// `C` cancel, `X` execute), then the screen saved to `out`. Used for the
/// README's pictures of the badge's other apps. The badge's node name is
/// [`HERO_NAME`], so the name tag screen is not empty. One per process.
pub fn screen(keys: &str, out: &Path) -> Result<Summary, String> {
    set_up();
    sim_support::install_pets_from_repo_cfg();
    crate::NODE_NAME.with_mut(|name| {
        name.clear();
        let _ = name.push_str(HERO_NAME);
    });
    for c in keys.chars() {
        let button = match c {
            'U' => ButtonId::Up,
            'D' => ButtonId::Down,
            'L' => ButtonId::Left,
            'R' => ButtonId::Right,
            'F' => ButtonId::Fire,
            'C' => ButtonId::Cancel,
            'X' => ButtonId::Execute,
            _ => return Err(format!("unknown key {c:?} in {keys:?}")),
        };
        dispatch_press(button);
    }
    let display = sim_support::render();
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    sim_support::save_png(&display, out)?;
    let size = embedded_graphics::prelude::OriginDimensions::size(&display);
    Ok(Summary {
        w: size.width * sim_support::PNG_SCALE,
        h: size.height * sim_support::PNG_SCALE,
        non_white: sim_support::non_white(&display),
    })
}

/// Play the shot `name` on a fresh game, check its expectation, save
/// `<out_dir>/<name>.png`. One shot per process.
pub fn run(name: &str, out_dir: &Path) -> Result<Summary, String> {
    let shot = SHOTS
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| format!("no shot named {name}"))?;
    set_up();
    // The roster installs once per process, and a shot is a process. The
    // Realm shot mirrors an old badge: its PETS.CFG was never edited, so it
    // still lists the Jellyfish, while its sprites are gone.
    if name == REALM_SHOT {
        sim_support::install_pets_from_old_badge_cfg();
    } else {
        sim_support::install_pets_from_repo_cfg();
    }
    SHOT_PET.store(pet_for(name, PetKind::roster()).0, Ordering::Relaxed);

    catch_unwind(AssertUnwindSafe(|| play(shot.steps))).map_err(|e| {
        let why = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        format!("shot {name}: a step failed: {why}")
    })?;
    if !(shot.expect)() {
        return Err(format!(
            "shot {name}: the game is not in the state the shot shows (status line {:?}, anim {:?})",
            lifecycle::pet_line(),
            lifecycle::display_anim(),
        ));
    }

    let display = sim_support::render();
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    sim_support::save_png(&display, &out_dir.join(format!("{name}.png")))?;
    let size = embedded_graphics::prelude::OriginDimensions::size(&display);
    Ok(Summary {
        w: size.width * sim_support::PNG_SCALE,
        h: size.height * sim_support::PNG_SCALE,
        non_white: sim_support::non_white(&display),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_up_pins_the_clock_to_noon() {
        let _g = crate::game::lock_test_state();
        set_fake_unix(None);
        set_up();
        assert_eq!(lifecycle::local_minute(), Some(720));
        set_fake_unix(Some(SHOT_NIGHT_UNIX));
        assert_eq!(lifecycle::local_minute(), Some(23 * 60));
        set_fake_unix(Some(GUIDE_NOON_UNIX));
    }

    fn cfg_roster() -> Vec<PetKind> {
        include_str!("../../assets/to-badge/PETS.CFG")
            .lines()
            .filter_map(|l| l.split_once('=')?.0.trim().parse().ok().map(PetKind))
            .collect()
    }

    #[test]
    fn shots_show_a_variety_of_pets_and_pairs_agree() {
        let roster = cfg_roster();
        assert!(roster.len() >= 4);
        let pets: Vec<PetKind> =
            SHOTS.iter().map(|s| pet_for(s.name, &roster)).collect();
        assert_eq!(
            pets,
            SHOTS.iter().map(|s| pet_for(s.name, &roster)).collect::<Vec<_>>()
        );
        let mut seen: Vec<u8> = pets.iter().map(|k| k.0).collect();
        seen.sort_unstable();
        seen.dedup();
        assert!(seen.len() >= 4, "only {} different pets", seen.len());
        assert!(pets.iter().all(|k| roster.contains(k) && k.0 != JELLYFISH.0));
        for (name, lead) in WITH {
            assert!(SHOTS.iter().any(|s| s.name == *name), "{name}");
            assert_eq!(pet_for(name, &roster), pet_for(lead, &roster), "{name}");
        }
        // Shots listed one after the other differ, unless one is in a pair.
        let in_pair = |n: &str| WITH.iter().any(|(a, b)| *a == n || *b == n);
        for w in SHOTS.windows(2) {
            if !in_pair(w[0].name) && !in_pair(w[1].name) {
                assert_ne!(pet_for(w[0].name, &roster), pet_for(w[1].name, &roster), "{}", w[1].name);
            }
        }
    }
}

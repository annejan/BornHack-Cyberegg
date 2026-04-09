use core::cell::RefCell;
use core::sync::atomic::Ordering;

use embedded_graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_7X13},
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
    text::{Alignment, Baseline, Text, TextStyleBuilder},
};

use crate::{BLACK, TriColor, WHITE};

// ── Screen identifiers ──────────────────────────────────────────────────────

/// Screen identifiers — array index into `DisplayState`.
/// Adding or reordering a variant automatically adjusts indices.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScreenId {
    Game       = 0,
    Main       = 1,
    Pm         = 2,
    Channel    = 3,
    Advert     = 4,
    Badgercorn = 5,
}

impl ScreenId {
    pub const fn index(self) -> u8 { self as u8 }
    pub const COUNT: usize = 6;
}

// ── Button identifiers ──────────────────────────────────────────────────────

/// Hardware button / joystick direction.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ButtonId {
    Cancel  = 0,
    Execute = 1,
    Up      = 2,
    Down    = 3,
    Left    = 4,
    Right   = 5,
    Fire    = 6,
}

impl ButtonId {
    pub fn from_index(i: usize) -> Option<Self> {
        match i {
            0 => Some(Self::Cancel),
            1 => Some(Self::Execute),
            2 => Some(Self::Up),
            3 => Some(Self::Down),
            4 => Some(Self::Left),
            5 => Some(Self::Right),
            6 => Some(Self::Fire),
            _ => None,
        }
    }
}

// ── Item kinds ────────────────────────────────────────────────────────────────

pub enum MenuItemKind {
    Action(fn()),
    Submenu(&'static [MenuItem]),
    Back,
    /// Visual divider — not selectable; navigation skips over it.
    Separator,
    /// Inline value selector. Cancel increments, execute decrements.
    /// The label function should return the current value as a string.
    Stepper { inc: fn(), dec: fn() },
}

pub struct MenuItem {
    pub label: fn() -> &'static str,
    pub kind: MenuItemKind,
}

// ── Per-screen navigation state ───────────────────────────────────────────────

/// Cursor position and optional active submenu for one screen.
pub struct ScreenState {
    root_items: &'static [MenuItem],
    root_pos: u8,
    /// `Some` while inside a submenu; `None` at the root level.
    sub_items: Option<&'static [MenuItem]>,
    sub_pos: u8,
}

impl ScreenState {
    pub const fn new(items: &'static [MenuItem]) -> Self {
        Self {
            root_items: items,
            root_pos: 0,
            sub_items: None,
            sub_pos: 0,
        }
    }

    pub fn current_items(&self) -> &'static [MenuItem] {
        match self.sub_items {
            Some(items) => items,
            None => self.root_items,
        }
    }

    pub fn current_pos(&self) -> usize {
        if self.sub_items.is_some() {
            self.sub_pos as usize
        } else {
            self.root_pos as usize
        }
    }

    fn current_pos_mut(&mut self) -> &mut u8 {
        if self.sub_items.is_some() {
            &mut self.sub_pos
        } else {
            &mut self.root_pos
        }
    }

    pub fn menu_up(&mut self) {
        let items = self.current_items();
        let pos = self.current_pos();
        if pos == 0 {
            return;
        }
        let mut prev = pos - 1;
        while prev > 0 && matches!(items[prev].kind, MenuItemKind::Separator) {
            prev -= 1;
        }
        if !matches!(items[prev].kind, MenuItemKind::Separator) {
            *self.current_pos_mut() = prev as u8;
        }
    }

    pub fn menu_down(&mut self) {
        let items = self.current_items();
        let len = items.len();
        let pos = self.current_pos();
        let mut next = pos + 1;
        while next < len && matches!(items[next].kind, MenuItemKind::Separator) {
            next += 1;
        }
        if next < len {
            *self.current_pos_mut() = next as u8;
        }
    }

    pub fn current_item(&self) -> &'static MenuItem {
        &self.current_items()[self.current_pos()]
    }

    /// Activate the currently selected item.
    ///
    /// - `Action` → call its function.
    /// - `Submenu` → push into the submenu, cursor reset to 0.
    /// - `Back` → pop back to the root menu.
    pub fn fire(&mut self) {
        match self.current_item().kind {
            MenuItemKind::Action(f) => f(),
            MenuItemKind::Submenu(items) => {
                self.sub_items = Some(items);
                self.sub_pos = 0;
            }
            MenuItemKind::Back => {
                self.sub_items = None;
            }
            MenuItemKind::Separator => {}
            MenuItemKind::Stepper { dec, .. } => dec(),
        }
    }

    /// Called when the cancel button is pressed.  Increments a focused stepper;
    /// no-op for all other item kinds.
    pub fn on_cancel(&mut self) {
        if let MenuItemKind::Stepper { inc, .. } = self.current_item().kind {
            inc();
        }
    }

    pub fn get_label(&self, index: usize) -> Option<&'static str> {
        self.current_items().get(index).map(|item| (item.label)())
    }

    pub fn get_current_label(&self) -> Option<&'static str> {
        Some((self.current_item().label)())
    }
}

// ── Top-level display state ───────────────────────────────────────────────────

/// `M` screens, each with their own item list and cursor.
/// Left/right switches screens, skipping disabled ones.
/// Up/down moves within the current screen's menu.
pub struct DisplayState<const M: usize> {
    active_screen: u8,
    screens: [ScreenState; M],
    enabled: [bool; M],
}

#[allow(dead_code)]
impl<const M: usize> DisplayState<M> {
    pub const fn new(screens: [ScreenState; M], enabled: [bool; M]) -> Self {
        // Start on the first enabled screen (or 0 if none enabled).
        let mut first = 0u8;
        while (first as usize) < M {
            if enabled[first as usize] { break; }
            first += 1;
        }
        if first as usize >= M { first = 0; }
        Self {
            active_screen: first,
            screens,
            enabled,
        }
    }

    /// Enable or disable a screen at runtime.  If the currently active
    /// screen becomes disabled, jumps to the nearest enabled screen.
    pub fn set_enabled(&mut self, screen: u8, on: bool) {
        if (screen as usize) < M {
            self.enabled[screen as usize] = on;
        }
        if !self.enabled[self.active_screen as usize] {
            if let Some(s) = self.next_enabled_right(self.active_screen)
                .or_else(|| self.next_enabled_left(self.active_screen))
            {
                self.active_screen = s;
            }
        }
    }

    fn next_enabled_right(&self, from: u8) -> Option<u8> {
        let mut s = from as usize + 1;
        while s < M {
            if self.enabled[s] { return Some(s as u8); }
            s += 1;
        }
        None
    }

    fn next_enabled_left(&self, from: u8) -> Option<u8> {
        if from == 0 { return None; }
        let mut s = from as usize - 1;
        loop {
            if self.enabled[s] { return Some(s as u8); }
            if s == 0 { return None; }
            s -= 1;
        }
    }

    pub fn screen_left(&mut self) {
        if let Some(s) = self.next_enabled_left(self.active_screen) {
            self.active_screen = s;
        }
    }

    pub fn screen_right(&mut self) {
        if let Some(s) = self.next_enabled_right(self.active_screen) {
            self.active_screen = s;
        }
    }

    pub fn active_screen(&self) -> u8 {
        self.active_screen
    }

    pub fn current_screen(&self) -> &ScreenState {
        &self.screens[self.active_screen as usize]
    }

    pub fn current_screen_mut(&mut self) -> &mut ScreenState {
        &mut self.screens[self.active_screen as usize]
    }

    pub fn menu_up(&mut self) {
        self.current_screen_mut().menu_up();
    }

    pub fn menu_down(&mut self) {
        self.current_screen_mut().menu_down();
    }

    pub fn fire(&mut self) {
        self.current_screen_mut().fire();
    }

    pub fn on_cancel(&mut self) {
        self.current_screen_mut().on_cancel();
    }

    /// Dispatch a button press to the menu layer.
    /// Called when the game layer did not consume the event.
    pub fn dispatch_button(&mut self, btn: ButtonId) {
        match btn {
            ButtonId::Cancel  => self.on_cancel(),
            ButtonId::Execute => {} // no menu role
            ButtonId::Up      => self.menu_up(),
            ButtonId::Down    => self.menu_down(),
            ButtonId::Left    => self.screen_left(),
            ButtonId::Right   => self.screen_right(),
            ButtonId::Fire    => self.fire(),
        }
    }

    pub fn get_current_menu_item(&self) -> Option<&'static str> {
        self.current_screen().get_current_label()
    }

    pub fn get_menu_item(&self, index: usize) -> Option<&'static str> {
        self.current_screen().get_label(index)
    }
}

// ── Action / label helpers ────────────────────────────────────────────────────

fn label_boost_rx() -> &'static str {
    if crate::BOOSTED_RX_GAIN.load(Ordering::Relaxed) {
        "Boost RX: ON"
    } else {
        "Boost RX: OFF"
    }
}

fn action_boost_rx() {
    let current = crate::BOOSTED_RX_GAIN.load(Ordering::Relaxed);
    crate::BOOSTED_RX_GAIN.store(!current, Ordering::Relaxed);
    #[cfg(feature = "mesh")]
    crate::BOOST_RX_CHANGED_SIGNAL.signal(());
}

fn action_reset_channels() {
    #[cfg(feature = "mesh")]
    crate::CHANNEL_RESET_SIGNAL.signal(());
}

fn action_reset_contacts() {
    #[cfg(feature = "mesh")]
    crate::CONTACT_RESET_SIGNAL.signal(());
}

static TZ_LABELS: [&str; 27] = [
    "UTC-12", "UTC-11", "UTC-10", "UTC-9",  "UTC-8",  "UTC-7",  "UTC-6",
    "UTC-5",  "UTC-4",  "UTC-3",  "UTC-2",  "UTC-1",  "UTC+0",
    "UTC+1",  "UTC+2",  "UTC+3",  "UTC+4",  "UTC+5",  "UTC+6",
    "UTC+7",  "UTC+8",  "UTC+9",  "UTC+10", "UTC+11", "UTC+12",
    "UTC+13", "UTC+14",
];

fn label_timezone() -> &'static str {
    let offset = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);
    TZ_LABELS[(offset.clamp(-12, 14) + 12) as usize]
}

fn action_tz_inc() {
    let v = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);
    if v < 14 {
        crate::TIMEZONE_OFFSET.store(v + 1, Ordering::Relaxed);
        #[cfg(feature = "embassy-base")]
        crate::TZ_CHANGED_SIGNAL.signal(());
    }
}

fn action_tz_dec() {
    let v = crate::TIMEZONE_OFFSET.load(Ordering::Relaxed);
    if v > -12 {
        crate::TIMEZONE_OFFSET.store(v - 1, Ordering::Relaxed);
        #[cfg(feature = "embassy-base")]
        crate::TZ_CHANGED_SIGNAL.signal(());
    }
}

fn action_melody_0() {
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(0);
}

fn action_melody_1() {
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(1);
}

fn action_melody_2() {
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(2);
}

fn action_melody_3() {
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(3);
}

// ── Static item arrays ────────────────────────────────────────────────────────

static MELODY_ITEMS: [MenuItem; 5] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: || "Startup",
        kind: MenuItemKind::Action(action_melody_0),
    },
    MenuItem {
        label: || "Rickroll",
        kind: MenuItemKind::Action(action_melody_1),
    },
    MenuItem {
        label: || "Imp. March",
        kind: MenuItemKind::Action(action_melody_2),
    },
    MenuItem {
        label: || "Sandstorm",
        kind: MenuItemKind::Action(action_melody_3),
    },
];

static SETTINGS_ITEMS: [MenuItem; 6] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: label_boost_rx,
        kind: MenuItemKind::Action(action_boost_rx),
    },
    MenuItem {
        label: label_timezone,
        kind: MenuItemKind::Stepper { inc: action_tz_inc, dec: action_tz_dec },
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::Separator,
    },
    MenuItem {
        label: || "Reset channels",
        kind: MenuItemKind::Action(action_reset_channels),
    },
    MenuItem {
        label: || "Reset contacts",
        kind: MenuItemKind::Action(action_reset_contacts),
    },
];

static BORNAGOTCHI_ITEMS: [MenuItem; 7] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: || "Mute",
        kind: MenuItemKind::Action(|| {}),
    },
    MenuItem {
        label: || "Disable Game",
        kind: MenuItemKind::Action(|| {}),
    },
    MenuItem {
        label: || "Set Name",
        kind: MenuItemKind::Action(|| {}),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::Separator,
    },
    MenuItem {
        label: || "Reset Pet",
        kind: MenuItemKind::Action(|| {}),
    },
    MenuItem {
        label: || "Unicorn Realm",
        kind: MenuItemKind::Action(|| {}),
    },
];

#[cfg(feature = "game")]
static GAME_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "BornPets",
    kind: MenuItemKind::Action(|| {}),
}];

static MAIN_ITEMS: [MenuItem; 4] = [
    MenuItem {
        label: || "Bornagotchi",
        kind: MenuItemKind::Submenu(&BORNAGOTCHI_ITEMS),
    },
    MenuItem {
        label: || "Play melodies",
        kind: MenuItemKind::Submenu(&MELODY_ITEMS),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::Separator,
    },
    MenuItem {
        label: || "Settings",
        kind: MenuItemKind::Submenu(&SETTINGS_ITEMS),
    },
];

static PM_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "PM",
    kind: MenuItemKind::Action(|| {}),
}];

static LORA_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "LoRa",
    kind: MenuItemKind::Action(|| {}),
}];

static ADVERT_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "Adverts",
    kind: MenuItemKind::Action(|| {}),
}];

static BADGERCORN_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "Badgercorn",
    kind: MenuItemKind::Action(|| {}),
}];

// ── DISPLAY_STATE ─────────────────────────────────────────────────────────────

pub const SCREEN_COUNT: usize = ScreenId::COUNT;

// The game screen placeholder when the feature is disabled — never navigated to.
#[cfg(not(feature = "game"))]
static GAME_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "BornPets",
    kind: MenuItemKind::Action(|| {}),
}];

#[cfg(feature = "game")]
const GAME_ENABLED: bool = true;
#[cfg(not(feature = "game"))]
const GAME_ENABLED: bool = false;

#[cfg(feature = "embassy-base")]
use embassy_sync::blocking_mutex::{Mutex, raw::ThreadModeRawMutex};

#[cfg(feature = "embassy-base")]
pub static DISPLAY_STATE: Mutex<ThreadModeRawMutex, RefCell<DisplayState<SCREEN_COUNT>>> =
    Mutex::new(RefCell::new(DisplayState::new(
        [
            ScreenState::new(&GAME_ITEMS),
            ScreenState::new(&MAIN_ITEMS),
            ScreenState::new(&PM_ITEMS),
            ScreenState::new(&LORA_ITEMS),
            ScreenState::new(&ADVERT_ITEMS),
            ScreenState::new(&BADGERCORN_ITEMS),
        ],
        [GAME_ENABLED, true, true, true, true, true],
    )));

#[cfg(feature = "simulator")]
use std::sync::Mutex;

#[cfg(feature = "simulator")]
pub static DISPLAY_STATE: Mutex<RefCell<DisplayState<SCREEN_COUNT>>> =
    Mutex::new(RefCell::new(DisplayState::new(
        [
            ScreenState::new(&GAME_ITEMS),
            ScreenState::new(&MAIN_ITEMS),
            ScreenState::new(&PM_ITEMS),
            ScreenState::new(&LORA_ITEMS),
            ScreenState::new(&ADVERT_ITEMS),
            ScreenState::new(&BADGERCORN_ITEMS),
        ],
        [GAME_ENABLED, true, true, true, true, true],
    )));

// ── Scrolling menu renderer ───────────────────────────────────────────────────

/// Geometry constants for the 152×152 display.
///
/// The menu occupies y = 38..106, leaving room for the header (dots, battery,
/// device ID) above and the status banner below.
const MENU_X: i32 = 4;
const MENU_Y: i32 = 38;
const MENU_W: u32 = 144;
const ROW_H: i32 = 22;
const NUM_ROWS: usize = 3; // one above, center, one below

/// Draw a scrolling 3-item menu centered on `pos`.
///
/// - Center row: black background, white text (inverted).
/// - Adjacent rows (if items exist): black text on white background.
/// - A 1 px border frames the entire menu area.
/// - Submenu items have " >" appended to their label.
pub fn draw_menu<D>(display: &mut D, items: &[MenuItem], pos: usize) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let text_style = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let menu_h = ROW_H * NUM_ROWS as i32 + 2;

    // Outer border
    Rectangle::new(Point::new(MENU_X, MENU_Y), Size::new(MENU_W, menu_h as u32))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
        .draw(display)?;

    for row in 0..NUM_ROWS {
        // item_idx: negative means "before the list" (no item to show).
        let item_idx = (pos as isize) + (row as isize) - 1;
        let row_y = MENU_Y + 1 + row as i32 * ROW_H;
        let text_y = row_y + ROW_H / 2;
        let is_center = row == 1;

        if is_center {
            Rectangle::new(
                Point::new(MENU_X + 1, row_y),
                Size::new(MENU_W - 2, ROW_H as u32),
            )
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        }

        if item_idx >= 0 {
            if let Some(item) = items.get(item_idx as usize) {
                let fg = if is_center { WHITE } else { BLACK };
                if matches!(item.kind, MenuItemKind::Separator) {
                    // Draw a thin horizontal rule across the row
                    Rectangle::new(Point::new(MENU_X + 8, text_y), Size::new(MENU_W - 16, 1))
                        .into_styled(PrimitiveStyle::with_fill(fg))
                        .draw(display)?;
                } else {
                    let mut label: heapless::String<24> = heapless::String::new();
                    if matches!(item.kind, MenuItemKind::Stepper { .. }) {
                        let _ = label.push_str("< ");
                    }
                    let _ = label.push_str((item.label)());
                    if matches!(item.kind, MenuItemKind::Submenu(_)) {
                        let _ = label.push_str(" >");
                    } else if matches!(item.kind, MenuItemKind::Stepper { .. }) {
                        let _ = label.push_str(" >");
                    }
                    Text::with_text_style(
                        &label,
                        Point::new(MENU_X + MENU_W as i32 / 2, text_y),
                        MonoTextStyle::new(&FONT_7X13, fg),
                        text_style,
                    )
                    .draw(display)?;
                }
            }
        }
    }

    Ok(())
}

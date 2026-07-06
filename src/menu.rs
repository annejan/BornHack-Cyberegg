use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::{BLACK, RED, TriColor, WHITE};

// ── Screen identifiers ──────────────────────────────────────────────────────

/// Screen identifiers — array index into `DisplayState`.
/// Adding or reordering a variant automatically adjusts indices.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScreenId {
    Game = 0,
    Main = 1,
    Pm = 2,
    Channel = 3,
    Advert = 4,
    Token = 5,
    Watch = 6,
    Calendar = 7,
    Name = 8,
    Qr = 9,
}

impl ScreenId {
    pub const fn index(self) -> u8 {
        self as u8
    }
    pub const COUNT: usize = 10;
}

// ── Button identifiers ──────────────────────────────────────────────────────

/// Hardware button / joystick direction.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ButtonId {
    Cancel = 0,
    Execute = 1,
    Up = 2,
    Down = 3,
    Left = 4,
    Right = 5,
    Fire = 6,
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

// ── Item kinds
// ────────────────────────────────────────────────────────────────

pub enum MenuItemKind {
    Action(fn()),
    Submenu(&'static [MenuItem]),
    Back,
    /// Visual divider — not selectable; navigation skips over it.
    Separator,
    /// Inline value selector. Cancel increments, execute decrements.
    /// The label function should return the current value as a string.
    Stepper {
        inc: fn(),
        dec: fn(),
    },
    /// Like `Stepper`, but renders its label by formatting into the menu
    /// buffer instead of returning a baked `&'static str`.  Use this for
    /// numeric value pickers (hour, minute, timezone, …) so we don't have
    /// to pre-bake `[&str; 24]` / `[&str; 60]` tables just to put a leading
    /// zero on 0–9.  `MenuItem::label` is ignored for this variant.
    ValueStepper {
        format: fn(&mut heapless::String<24>),
        inc: fn(),
        dec: fn(),
    },
    /// Read-only display row with a dynamically-formatted label.  Fire is a
    /// no-op; navigation skips no rows around it.  Used for live data the
    /// user can only observe — e.g. imported calendar-event alarm slots.
    /// `MenuItem::label` is ignored for this variant.
    Info {
        format: fn(&mut heapless::String<24>),
    },
    /// Like `Info`, but the formatter receives a `u8` slot index — lets one
    /// shared formatter render many similar rows (e.g. all 31 imported
    /// alarm slots) without per-slot boilerplate.  Optional per-row
    /// visibility: when `visible(slot)` returns false, the row is auto-
    /// skipped during nav and rendered as blank — handy for sparse lists
    /// where most slots are empty most of the time.  `MenuItem::label` is
    /// ignored for this variant.
    SlotInfo {
        format: fn(&mut heapless::String<24>, u8),
        visible: fn(u8) -> bool,
        slot: u8,
    },
    /// Destructive action that first shows a full-screen "Are you sure?"
    /// confirmation dialog. `prompt` is the action name shown in the dialog;
    /// `action` runs only if the user presses Fire/Execute to confirm.
    Confirm {
        prompt: &'static str,
        action: fn(),
    },
    /// Like `Confirm`, but the dialog is only shown when `needs_confirm()`
    /// returns true. Otherwise `action` runs immediately.
    ConditionalConfirm {
        prompt: &'static str,
        needs_confirm: fn() -> bool,
        action: fn(),
    },
}

pub struct MenuItem {
    pub label: fn() -> &'static str,
    pub kind: MenuItemKind,
}

/// Whether the cursor should skip past `kind` during Up/Down nav.
/// Separators are always skipped; `SlotInfo` rows are skipped when their
/// `visible` predicate currently returns false.
fn nav_skip(kind: &MenuItemKind) -> bool {
    match kind {
        MenuItemKind::Separator => true,
        MenuItemKind::SlotInfo { visible, slot, .. } => !visible(*slot),
        _ => false,
    }
}

// ── Per-screen navigation state
// ───────────────────────────────────────────────

/// Cursor position and optional active submenu for one screen.
pub struct ScreenState {
    root_items: &'static [MenuItem],
    root_pos: u8,
    /// `Some` while inside a submenu; `None` at the root level.
    sub_items: Option<&'static [MenuItem]>,
    sub_pos: u8,
    /// True when a `Stepper` item is being edited (Up/Down change its value).
    stepper_active: bool,
    /// Current page when the About screen is active.
    about_page: u8,
    /// Current preset index when the LoRa radio screen is active.
    /// Equals `LORA_PRESETS.len()` when the device is on a Custom preset.
    lora_page: u8,
    /// Pending "Are you sure?" confirmation. When `Some`, the screen renders
    /// the confirmation dialog instead of the regular menu and Fire/Cancel
    /// are routed to yes/no.
    confirm: Option<(&'static str, fn())>,
}

impl ScreenState {
    pub const fn new(items: &'static [MenuItem]) -> Self {
        Self {
            root_items: items,
            root_pos: 0,
            sub_items: None,
            sub_pos: 0,
            stepper_active: false,
            about_page: 0,
            lora_page: 0,
            confirm: None,
        }
    }

    /// Returns the confirmation dialog prompt when a confirm is pending.
    pub fn confirm_prompt(&self) -> Option<&'static str> {
        self.confirm.map(|(p, _)| p)
    }

    /// Current LoRa radio preset index.
    pub fn lora_page(&self) -> u8 {
        self.lora_page
    }

    /// Returns true when the LoRa radio preset screen is active.
    pub fn is_lora_radio(&self) -> bool {
        match self.sub_items {
            Some(items) => core::ptr::eq(items, &LORA_RADIO_ITEMS as &[MenuItem]),
            None => false,
        }
    }

    /// Current about page index.
    pub fn about_page(&self) -> u8 {
        self.about_page
    }

    /// Returns true when a stepper is being edited.
    pub fn is_stepper_active(&self) -> bool {
        self.stepper_active
    }

    /// Returns true when the About screen is active.
    pub fn is_about(&self) -> bool {
        match self.sub_items {
            Some(items) => core::ptr::eq(items, &ABOUT_ITEMS as &[MenuItem]),
            None => false,
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
        // While editing a stepper, Up increments the value.
        if self.stepper_active {
            match self.current_item().kind {
                MenuItemKind::Stepper { inc, .. } | MenuItemKind::ValueStepper { inc, .. } => inc(),
                _ => {}
            }
            return;
        }
        let items = self.current_items();
        let pos = self.current_pos();
        if pos == 0 {
            return;
        }
        let mut prev = pos - 1;
        while prev > 0 && nav_skip(&items[prev].kind) {
            prev -= 1;
        }
        if !nav_skip(&items[prev].kind) {
            *self.current_pos_mut() = prev as u8;
        }
    }

    pub fn menu_down(&mut self) {
        // While editing a stepper, Down decrements the value.
        if self.stepper_active {
            match self.current_item().kind {
                MenuItemKind::Stepper { dec, .. } | MenuItemKind::ValueStepper { dec, .. } => dec(),
                _ => {}
            }
            return;
        }
        let items = self.current_items();
        let len = items.len();
        let pos = self.current_pos();
        let mut next = pos + 1;
        while next < len && nav_skip(&items[next].kind) {
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
    /// - `Stepper` → toggle editing mode (Up/Down change value while active).
    pub fn fire(&mut self) {
        if self.stepper_active {
            // Deactivate stepper editing.
            self.stepper_active = false;
            return;
        }
        match self.current_item().kind {
            MenuItemKind::Action(f) => f(),
            MenuItemKind::Submenu(items) => {
                self.sub_items = Some(items);
                self.sub_pos = 0;
                if core::ptr::eq(items, &LORA_RADIO_ITEMS as &[MenuItem]) {
                    self.lora_page = current_lora_preset_index();
                }
            }
            MenuItemKind::Back => {
                self.sub_items = None;
            }
            MenuItemKind::Separator => {}
            // Info / SlotInfo rows are read-only — Fire is a no-op, like a
            // separator but with text.
            MenuItemKind::Info { .. } | MenuItemKind::SlotInfo { .. } => {}
            MenuItemKind::Stepper { .. } | MenuItemKind::ValueStepper { .. } => {
                self.stepper_active = true;
            }
            MenuItemKind::Confirm { prompt, action } => {
                self.confirm = Some((prompt, action));
            }
            MenuItemKind::ConditionalConfirm {
                prompt,
                needs_confirm,
                action,
            } => {
                if needs_confirm() {
                    self.confirm = Some((prompt, action));
                } else {
                    action();
                }
            }
        }
    }

    /// Called when the cancel button is pressed.
    ///
    /// - Deactivates an active stepper.
    /// - Pops out of a submenu back to the root menu.
    pub fn on_cancel(&mut self) {
        if self.stepper_active {
            self.stepper_active = false;
        } else if self.sub_items.is_some() {
            self.sub_items = None;
        }
    }
}

// ── Top-level display state
// ───────────────────────────────────────────────────

/// `M` screens, each with their own item list and cursor.
/// Left/right switches screens, skipping disabled ones.
/// Up/down moves within the current screen's menu.
pub struct DisplayState<const M: usize> {
    active_screen: u8,
    screens: [ScreenState; M],
    enabled: [bool; M],
}

impl<const M: usize> DisplayState<M> {
    pub const fn new(screens: [ScreenState; M], enabled: [bool; M]) -> Self {
        // Start on the first enabled screen (or 0 if none enabled).
        let mut first = 0u8;
        while (first as usize) < M {
            if enabled[first as usize] {
                break;
            }
            first += 1;
        }
        if first as usize >= M {
            first = 0;
        }
        Self {
            active_screen: first,
            screens,
            enabled,
        }
    }

    fn next_enabled_right(&self, from: u8) -> Option<u8> {
        let mut s = from as usize + 1;
        while s < M {
            if self.enabled[s] {
                return Some(s as u8);
            }
            s += 1;
        }
        None
    }

    fn next_enabled_left(&self, from: u8) -> Option<u8> {
        if from == 0 {
            return None;
        }
        let mut s = from as usize - 1;
        loop {
            if self.enabled[s] {
                return Some(s as u8);
            }
            if s == 0 {
                return None;
            }
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

    /// Programmatically jump to a specific screen (e.g. an NFC bonus
    /// hands focus back to the game screen so the toast is visible).
    /// Ignored if the requested screen is out of range or disabled.
    pub fn set_active_screen(&mut self, s: u8) {
        let idx = s as usize;
        if idx < M && self.enabled[idx] {
            self.active_screen = s;
        }
    }

    pub fn current_screen(&self) -> &ScreenState {
        &self.screens[self.active_screen as usize]
    }

    fn current_screen_mut(&mut self) -> &mut ScreenState {
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
        // A ringing alarm eats the first button press anywhere in the UI:
        // silence the buzzer and consume the event so the user has to press
        // again to actually navigate.
        #[cfg(all(feature = "watch", feature = "embassy-base"))]
        if crate::watch::dismiss_alarm_if_ringing() {
            return;
        }

        // Text entry intercepts all input when active.
        if crate::text_entry::is_active() {
            let done = {
                #[cfg(feature = "embassy-base")]
                {
                    crate::text_entry::TEXT_ENTRY.lock(|cell| {
                        let mut borrow = cell.borrow_mut();
                        if let Some(ref mut entry) = *borrow {
                            entry.dispatch(btn)
                        } else {
                            false
                        }
                    })
                }
                #[cfg(feature = "simulator")]
                {
                    let guard = crate::text_entry::TEXT_ENTRY.lock().unwrap();
                    let mut borrow = guard.borrow_mut();
                    if let Some(ref mut entry) = *borrow {
                        entry.dispatch(btn)
                    } else {
                        false
                    }
                }
            };
            if done {
                #[cfg(feature = "embassy-base")]
                crate::text_entry::TEXT_ENTRY.lock(|cell| cell.replace(None));
                #[cfg(feature = "simulator")]
                crate::text_entry::TEXT_ENTRY.lock().unwrap().replace(None);
            }
            return;
        }

        // Qwiic Scan intercepts all input when active — any button closes it.
        if crate::fw::qwiic::is_active() {
            crate::fw::qwiic::close();
            crate::FULL_REFRESH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
            return;
        }

        // Unicorn Realm intercepts all input when active.
        #[cfg(feature = "game")]
        if crate::game::realm_view::is_active() {
            match btn {
                ButtonId::Up => crate::game::realm_view::scroll_up(),
                ButtonId::Down => crate::game::realm_view::scroll_down(),
                _ => crate::game::realm_view::close(),
            }
            return;
        }

        // Channel browser intercepts input on the Channel screen.
        #[cfg(feature = "mesh")]
        if self.active_screen == crate::SCREEN_CHANNEL {
            let leave = crate::fw::mesh::channel_browser::dispatch(btn);
            if leave {
                // Cancel propagates: let the normal handler switch screens.
            } else {
                return;
            }
        }

        // Contacts (Advert) screen intercepts list/popup/detail input.
        // Returns true when Cancel/Left/Right should propagate to the
        // screen-swipe carousel; everything else stays inside the screen.
        #[cfg(feature = "mesh")]
        if self.active_screen == crate::SCREEN_ADVERT {
            let leave = crate::fw::mesh::contacts_screen::dispatch(btn);
            if leave {
                // Fall through to the screen-swipe / cancel handler.
            } else {
                return;
            }
        }

        // PM inbox/thread screen intercepts list + thread input.  Same
        // contract as the Contacts screen: returns true when
        // Cancel/Left/Right should propagate to the carousel.
        #[cfg(feature = "mesh")]
        if self.active_screen == crate::SCREEN_PM {
            let leave = crate::fw::mesh::pm_inbox::dispatch(btn);
            if leave {
                // Fall through.
            } else {
                return;
            }
        }

        // Clock screen consumes Up/Down to toggle digital/analog face.
        // Other buttons (Left/Right for screen nav, Cancel, etc.) fall through.
        #[cfg(feature = "watch")]
        if self.active_screen == crate::SCREEN_WATCH && crate::watch::dispatch(btn) {
            return;
        }

        // Calendar screen starts in a passive mode — arrows and Cancel fall
        // through here so screen-nav works.  Fire/Execute is the trigger
        // that flips it into Active mode, where the calendar's own
        // dispatcher takes over the arrow keys for cursor movement and
        // Cancel returns to passive.  See `crate::watch::calendar` for the
        // full mode table.
        #[cfg(feature = "watch")]
        if self.active_screen == crate::SCREEN_CALENDAR && crate::watch::calendar::dispatch(btn) {
            return;
        }

        let screen = &self.screens[self.active_screen as usize];
        // Confirmation dialog takes priority over any other screen mode.
        if screen.confirm.is_some() {
            let s = &mut self.screens[self.active_screen as usize];
            match btn {
                ButtonId::Execute | ButtonId::Fire => {
                    if let Some((_, action)) = s.confirm.take() {
                        action();
                    }
                }
                ButtonId::Cancel => {
                    s.confirm = None;
                }
                _ => {}
            }
            return;
        }
        if screen.is_lora_radio() {
            let s = &mut self.screens[self.active_screen as usize];
            let n = LORA_PRESETS.len() as u8;
            match btn {
                ButtonId::Left | ButtonId::Up => {
                    let mut p = if s.lora_page >= n { 0 } else { s.lora_page };
                    p = if p == 0 { n - 1 } else { p - 1 };
                    s.lora_page = p;
                }
                ButtonId::Right | ButtonId::Down => {
                    let p = if s.lora_page >= n { 0 } else { s.lora_page + 1 };
                    s.lora_page = if p >= n { 0 } else { p };
                }
                ButtonId::Execute | ButtonId::Fire => {
                    if (s.lora_page as usize) < LORA_PRESETS.len() {
                        apply_lora_preset(s.lora_page as usize);
                    }
                }
                ButtonId::Cancel => {
                    s.sub_items = None;
                }
            }
            return;
        }
        if screen.is_about() {
            match btn {
                ButtonId::Left => {
                    let s = &mut self.screens[self.active_screen as usize];
                    if s.about_page > 0 {
                        s.about_page -= 1;
                        // Full-frame redraw so the new page's text is clean
                        // (delta over text leaves fragments of the old page).
                        crate::FULL_REFRESH_PENDING
                            .store(true, core::sync::atomic::Ordering::Relaxed);
                    }
                }
                ButtonId::Right => {
                    let s = &mut self.screens[self.active_screen as usize];
                    if s.about_page < ABOUT_PAGES - 1 {
                        s.about_page += 1;
                        crate::FULL_REFRESH_PENDING
                            .store(true, core::sync::atomic::Ordering::Relaxed);
                    }
                }
                ButtonId::Cancel | ButtonId::Execute | ButtonId::Fire => {
                    let s = &mut self.screens[self.active_screen as usize];
                    s.about_page = 0;
                    s.sub_items = None;
                    // Leaving / resetting the About view also redraws fully.
                    crate::FULL_REFRESH_PENDING.store(true, core::sync::atomic::Ordering::Relaxed);
                }
                _ => {}
            }
            return;
        }
        match btn {
            ButtonId::Cancel => self.on_cancel(),
            ButtonId::Execute | ButtonId::Fire => self.fire(),
            ButtonId::Up => self.menu_up(),
            ButtonId::Down => self.menu_down(),
            ButtonId::Left => self.screen_left(),
            ButtonId::Right => self.screen_right(),
        }
    }
}

// ── Action / label helpers
// ────────────────────────────────────────────────────

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

fn action_factory_reset() {
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::FACTORY_RESET_SIGNAL.signal(());
}

// ── TX power ────────────────────────────────────────────────────────────────

fn fmt_tx_power(buf: &mut heapless::String<24>) {
    use core::fmt::Write;
    let v = crate::LORA_TX_POWER.load(Ordering::Relaxed).clamp(-9, 22);
    let _ = write!(buf, "TX: {} dBm", v);
}

fn action_tx_power_inc() {
    let v = crate::LORA_TX_POWER.load(Ordering::Relaxed);
    if v < 22 {
        crate::LORA_TX_POWER.store(v + 1, Ordering::Relaxed);
        #[cfg(all(feature = "mesh", feature = "embassy-base"))]
        crate::LORA_RADIO_CHANGED_SIGNAL.signal(());
    }
}

fn action_tx_power_dec() {
    let v = crate::LORA_TX_POWER.load(Ordering::Relaxed);
    if v > -9 {
        crate::LORA_TX_POWER.store(v - 1, Ordering::Relaxed);
        #[cfg(all(feature = "mesh", feature = "embassy-base"))]
        crate::LORA_RADIO_CHANGED_SIGNAL.signal(());
    }
}

// ── Client-repeat toggle ────────────────────────────────────────────────────

fn label_client_repeat() -> &'static str {
    if crate::LORA_CLIENT_REPEAT.load(Ordering::Relaxed) {
        "Repeat: ON"
    } else {
        "Repeat: OFF"
    }
}

fn action_client_repeat_toggle() {
    let cur = crate::LORA_CLIENT_REPEAT.load(Ordering::Relaxed);
    crate::LORA_CLIENT_REPEAT.store(!cur, Ordering::Relaxed);
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::LORA_RADIO_CHANGED_SIGNAL.signal(());
}

// ── Share location (advert_loc_policy) ─────────────────────────────────────

fn label_advert_loc() -> &'static str {
    if crate::ADVERT_LOC_POLICY.load(Ordering::Relaxed) {
        "Share Loc: ON"
    } else {
        "Share Loc: OFF"
    }
}

fn action_advert_loc() {
    let cur = crate::ADVERT_LOC_POLICY.load(Ordering::Relaxed);
    crate::ADVERT_LOC_POLICY.store(!cur, Ordering::Relaxed);
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::OTHER_PARAMS_CHANGED_SIGNAL.signal(());
}

// ── Multi-ACK stepper ──────────────────────────────────────────────────────

fn label_multi_acks() -> &'static str {
    match crate::MULTI_ACKS.load(Ordering::Relaxed) {
        2 => "Multi-ACK: 2",
        _ => "Multi-ACK: 1",
    }
}

fn action_multi_acks_inc() {
    let v = crate::MULTI_ACKS.load(Ordering::Relaxed);
    if v < 2 {
        crate::MULTI_ACKS.store(v + 1, Ordering::Relaxed);
        #[cfg(all(feature = "mesh", feature = "embassy-base"))]
        crate::OTHER_PARAMS_CHANGED_SIGNAL.signal(());
    }
}

fn action_multi_acks_dec() {
    let v = crate::MULTI_ACKS.load(Ordering::Relaxed);
    if v > 1 {
        crate::MULTI_ACKS.store(v - 1, Ordering::Relaxed);
        #[cfg(all(feature = "mesh", feature = "embassy-base"))]
        crate::OTHER_PARAMS_CHANGED_SIGNAL.signal(());
    }
}

// ── Path hash length stepper ───────────────────────────────────────────────

#[cfg(feature = "mesh")]
fn label_path_hash() -> &'static str {
    match crate::fw::mesh::PATH_HASH_MODE.load(Ordering::Relaxed) {
        0 => "Path Hash: 1B",
        1 => "Path Hash: 2B",
        _ => "Path Hash: 3B",
    }
}

#[cfg(not(feature = "mesh"))]
fn label_path_hash() -> &'static str {
    "Path Hash: 1B"
}

fn action_path_hash_inc() {
    #[cfg(feature = "mesh")]
    {
        let v = crate::fw::mesh::PATH_HASH_MODE.load(Ordering::Relaxed);
        if v < 2 {
            crate::fw::mesh::PATH_HASH_MODE.store(v + 1, Ordering::Relaxed);
            #[cfg(feature = "embassy-base")]
            crate::PATH_HASH_CHANGED_SIGNAL.signal(());
        }
    }
}

fn action_path_hash_dec() {
    #[cfg(feature = "mesh")]
    {
        let v = crate::fw::mesh::PATH_HASH_MODE.load(Ordering::Relaxed);
        if v > 0 {
            crate::fw::mesh::PATH_HASH_MODE.store(v - 1, Ordering::Relaxed);
            #[cfg(feature = "embassy-base")]
            crate::PATH_HASH_CHANGED_SIGNAL.signal(());
        }
    }
}

// ── EPD LUT speed stepper ─────────────────────────────────────────────────
//
// Scales the cycle-duration bytes in the SSD1675/B OTP LUT applied at
// every refresh.  100 = OEM duration, 30 = floor (any lower risks an
// unreadable display — user couldn't see menu to recover), 200 = double
// duration.  Step size 5 in 30..=200.
//
// Writes EPD_LUT_SPEED atomic + fires EPD_LUT_SPEED_DIRTY so the
// persister loop in fw/mesh/persister.rs writes the new value to the
// "settings" KV namespace alongside every other persisted setting.

const EPD_LUT_SPEED_STEP: u8 = 5;
const EPD_LUT_SPEED_MAX: u8 = 200;

fn fmt_epd_lut_speed(buf: &mut heapless::String<24>) {
    use core::fmt::Write;
    let v = crate::fw::epd::EPD_LUT_SPEED.load(Ordering::Relaxed);
    let _ = write!(buf, "EPD speed: {}%", v);
}

fn action_epd_lut_speed_inc() {
    let v = crate::fw::epd::EPD_LUT_SPEED.load(Ordering::Relaxed);
    let new = v.saturating_add(EPD_LUT_SPEED_STEP).min(EPD_LUT_SPEED_MAX);
    if new != v {
        crate::fw::epd::EPD_LUT_SPEED.store(new, Ordering::Relaxed);
        #[cfg(feature = "embassy-base")]
        crate::fw::epd::EPD_LUT_SPEED_DIRTY.signal(());
    }
}

fn action_epd_lut_speed_dec() {
    let v = crate::fw::epd::EPD_LUT_SPEED.load(Ordering::Relaxed);
    let new = v
        .saturating_sub(EPD_LUT_SPEED_STEP)
        .max(crate::fw::epd::EPD_LUT_SPEED_MIN);
    if new != v {
        crate::fw::epd::EPD_LUT_SPEED.store(new, Ordering::Relaxed);
        #[cfg(feature = "embassy-base")]
        crate::fw::epd::EPD_LUT_SPEED_DIRTY.signal(());
    }
}

// ── EPD temperature bias stepper ──────────────────────────────────────────
//
// User offset (°C × 10) added on top of the variant-aware self-heating
// bias when looking up the LUT table.  Range ±5 °C in 0.5 °C steps.
// Negative = treat panel as colder (picks a warmer / stronger WS),
// positive = treat panel as warmer (picks a milder WS).

fn fmt_epd_temp_bias(buf: &mut heapless::String<24>) {
    use core::fmt::Write;
    let v = crate::fw::epd::EPD_TEMP_BIAS_C10.load(Ordering::Relaxed) as i16;
    let whole = v / 10;
    let frac = (v.abs() % 10) as u8;
    let sign = if v >= 0 { '+' } else { '-' };
    let _ = write!(buf, "EPD bias: {}{}.{} C", sign, whole.abs(), frac);
}

fn action_epd_temp_bias_inc() {
    let v = crate::fw::epd::EPD_TEMP_BIAS_C10.load(Ordering::Relaxed);
    let new = v
        .saturating_add(crate::fw::epd::EPD_TEMP_BIAS_STEP)
        .min(crate::fw::epd::EPD_TEMP_BIAS_MAX);
    if new != v {
        crate::fw::epd::EPD_TEMP_BIAS_C10.store(new, Ordering::Relaxed);
        #[cfg(feature = "embassy-base")]
        crate::fw::epd::EPD_TEMP_BIAS_DIRTY.signal(());
    }
}

fn action_epd_temp_bias_dec() {
    let v = crate::fw::epd::EPD_TEMP_BIAS_C10.load(Ordering::Relaxed);
    let new = v
        .saturating_sub(crate::fw::epd::EPD_TEMP_BIAS_STEP)
        .max(crate::fw::epd::EPD_TEMP_BIAS_MIN);
    if new != v {
        crate::fw::epd::EPD_TEMP_BIAS_C10.store(new, Ordering::Relaxed);
        #[cfg(feature = "embassy-base")]
        crate::fw::epd::EPD_TEMP_BIAS_DIRTY.signal(());
    }
}

// ── Advert scheduling ──────────────────────────────────────────────────────

fn label_advert_enabled() -> &'static str {
    if crate::ADVERT_ENABLED.load(Ordering::Relaxed) {
        "Adverts: ON"
    } else {
        "Adverts: OFF"
    }
}

fn action_advert_toggle() {
    let cur = crate::ADVERT_ENABLED.load(Ordering::Relaxed);
    crate::ADVERT_ENABLED.store(!cur, Ordering::Relaxed);
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::ADVERT_CHANGED_SIGNAL.signal(());
}

/// Interval presets (hours): 2, 4, 8, 16, 32, 64, 96.
static ADVERT_INTERVAL_STEPS: [u8; 7] = [2, 4, 8, 16, 32, 64, 96];
static ADVERT_INTERVAL_LABELS: [&str; 7] = [
    "Interval: 2h",
    "Interval: 4h",
    "Interval: 8h",
    "Interval: 16h",
    "Interval: 32h",
    "Interval: 64h",
    "Interval: 96h",
];

fn advert_interval_idx() -> usize {
    let v = crate::ADVERT_INTERVAL_HOURS.load(Ordering::Relaxed);
    ADVERT_INTERVAL_STEPS
        .iter()
        .position(|&s| s == v)
        .unwrap_or(0)
}

fn label_advert_interval() -> &'static str {
    ADVERT_INTERVAL_LABELS[advert_interval_idx()]
}

fn action_advert_interval_inc() {
    let i = advert_interval_idx();
    if i + 1 < ADVERT_INTERVAL_STEPS.len() {
        crate::ADVERT_INTERVAL_HOURS.store(ADVERT_INTERVAL_STEPS[i + 1], Ordering::Relaxed);
        #[cfg(all(feature = "mesh", feature = "embassy-base"))]
        crate::ADVERT_CHANGED_SIGNAL.signal(());
    }
}

fn action_advert_interval_dec() {
    let i = advert_interval_idx();
    if i > 0 {
        crate::ADVERT_INTERVAL_HOURS.store(ADVERT_INTERVAL_STEPS[i - 1], Ordering::Relaxed);
        #[cfg(all(feature = "mesh", feature = "embassy-base"))]
        crate::ADVERT_CHANGED_SIGNAL.signal(());
    }
}

/// Send an advert immediately (flood-routed) regardless of the
/// scheduled interval.  Useful at events for quickly making yourself
/// visible to nearby badges without waiting up to 16 hours.
#[cfg(all(feature = "mesh", feature = "embassy-base"))]
fn action_advert_send_now() {
    let _ = crate::fw::mesh::tx_send(crate::fw::mesh::TxRequest::Advert(
        crate::fw::mesh::meshcore::AdvertMode::Flood,
    ));
    defmt::info!("menu: manual flood advert queued");
}

#[cfg(not(all(feature = "mesh", feature = "embassy-base")))]
fn action_advert_send_now() {}

// ── Telemetry share ────────────────────────────────────────────────────────
//
// Three-state, mirrors the MeshCore companion-app "Allow Telemetry Requests"
// setting (No / From Specific Contacts / Yes).  Encoded as 2 bits in
// `OtherParams.telemetry_mode_base`:
//   0 = TELEM_MODE_DENY        — never respond
//   1 = TELEM_MODE_ALLOW_FLAGS — respond when the requester's contact.flags
//                                bit 1 is set (per-contact opt-in)
//   2 = TELEM_MODE_ALLOW_ALL   — respond to every authenticated request

fn label_telemetry_share() -> &'static str {
    match crate::TELEMETRY_MODE_BASE.load(Ordering::Relaxed) {
        0 => "Telemetry: No",
        1 => "Telemetry: Contacts",
        _ => "Telemetry: Yes",
    }
}

// ── Ignore blink toggle ────────────────────────────────────────────────────

fn label_game_mute() -> &'static str {
    if crate::GAME_MUTE.load(Ordering::Relaxed) {
        "Mute (On)"
    } else {
        "Mute (Off)"
    }
}

fn action_game_mute_toggle() {
    let cur = crate::GAME_MUTE.load(Ordering::Relaxed);
    crate::GAME_MUTE.store(!cur, Ordering::Relaxed);
}

fn label_boot_chime() -> &'static str {
    if crate::BOOT_CHIME_ENABLED.load(Ordering::Relaxed) {
        "Boot chime: On"
    } else {
        "Boot chime: Off"
    }
}

fn action_boot_chime_toggle() {
    let cur = crate::BOOT_CHIME_ENABLED.load(Ordering::Relaxed);
    crate::BOOT_CHIME_ENABLED.store(!cur, Ordering::Relaxed);
    #[cfg(all(feature = "embassy-base", feature = "watch"))]
    crate::watch::SETTINGS_DIRTY_SIGNAL.signal(());
}

// ── Notification sound steppers (mesh only) ────────────────────────────────
//
// One row per `SoundEvent`, rendered as e.g. "PM: Nokia" with Up/Down
// cycling through `SOUND_TONES`.  All thunks forward to the
// `SoundEvent`-parameterised API in `fw::mesh::sounds`.
#[cfg(feature = "mesh")]
mod sound_menu {
    use core::fmt::Write;

    use crate::fw::mesh::sounds::{SoundEvent, tone_label, tone_step};

    fn fmt(buf: &mut heapless::String<24>, prefix: &str, event: SoundEvent) {
        let _ = write!(buf, "{}: {}", prefix, tone_label(event));
    }

    pub fn fmt_pm(buf: &mut heapless::String<24>) {
        fmt(buf, "PM", SoundEvent::PmReceived);
    }
    pub fn fmt_channel(buf: &mut heapless::String<24>) {
        fmt(buf, "Chan", SoundEvent::ChannelMsg);
    }
    pub fn fmt_contact(buf: &mut heapless::String<24>) {
        fmt(buf, "Disc", SoundEvent::ContactDiscovered);
    }

    pub fn pm_inc() {
        tone_step(SoundEvent::PmReceived, 1);
    }
    pub fn pm_dec() {
        tone_step(SoundEvent::PmReceived, -1);
    }
    pub fn channel_inc() {
        tone_step(SoundEvent::ChannelMsg, 1);
    }
    pub fn channel_dec() {
        tone_step(SoundEvent::ChannelMsg, -1);
    }
    pub fn contact_inc() {
        tone_step(SoundEvent::ContactDiscovered, 1);
    }
    pub fn contact_dec() {
        tone_step(SoundEvent::ContactDiscovered, -1);
    }
}

fn label_ignore_blink() -> &'static str {
    if crate::IGNORE_BLINK.load(Ordering::Relaxed) {
        "Ignore Blink: ON"
    } else {
        "Ignore Blink: OFF"
    }
}

fn action_ignore_blink() {
    let cur = crate::IGNORE_BLINK.load(Ordering::Relaxed);
    crate::IGNORE_BLINK.store(!cur, Ordering::Relaxed);
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::OTHER_PARAMS_CHANGED_SIGNAL.signal(());
}

fn action_telemetry_toggle() {
    // Cycle: No (0) → Contacts (1) → Yes (2) → No.
    let cur = crate::TELEMETRY_MODE_BASE.load(Ordering::Relaxed);
    let next = (cur + 1) % 3;
    crate::TELEMETRY_MODE_BASE.store(next, Ordering::Relaxed);
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::OTHER_PARAMS_CHANGED_SIGNAL.signal(());
}

// ── BLE submenu helpers ────────────────────────────────────────────────────

/// Static buffer holding the formatted BLE device name ("Cyber Ægg XXYY").
/// Initialised lazily on first render.
struct SyncBuf<const N: usize>(core::cell::UnsafeCell<[u8; N]>);
unsafe impl<const N: usize> Sync for SyncBuf<N> {}
impl<const N: usize> SyncBuf<N> {
    const fn new(val: [u8; N]) -> Self {
        Self(core::cell::UnsafeCell::new(val))
    }
}

static BLE_NAME_BUF: SyncBuf<15> = SyncBuf::new([
    b'C', b'y', b'b', b'e', b'r', b' ', 0xC3, 0x86, b'g', b'g', b' ', b'?', b'?', b'?', b'?',
]);
static BLE_NAME_INIT: AtomicBool = AtomicBool::new(false);

fn label_ble_name() -> &'static str {
    if !BLE_NAME_INIT.load(Ordering::Relaxed) {
        #[cfg(feature = "embassy-base")]
        {
            let id = crate::fw::device_id::get_bytes();
            let buf = unsafe { &mut *BLE_NAME_BUF.0.get() };
            buf[11] = id[0];
            buf[12] = id[1];
            buf[13] = id[2];
            buf[14] = id[3];
        }
        #[cfg(feature = "simulator")]
        {
            let buf = unsafe { &mut *BLE_NAME_BUF.0.get() };
            buf[11..15].copy_from_slice(b"A3F7");
        }
        BLE_NAME_INIT.store(true, Ordering::Relaxed);
    }
    unsafe { core::str::from_utf8_unchecked(&*BLE_NAME_BUF.0.get()) }
}

fn label_ble_enabled() -> &'static str {
    if crate::BLE_DISABLED.load(Ordering::Relaxed) {
        "BLE: OFF"
    } else {
        "BLE: ON"
    }
}

fn action_ble_toggle() {
    let cur = crate::BLE_DISABLED.load(Ordering::Relaxed);
    crate::BLE_DISABLED.store(!cur, Ordering::Relaxed);
    #[cfg(feature = "embassy-base")]
    crate::BLE_DISABLED_CHANGED.signal(());
}

fn action_clear_bonds() {
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::CLEAR_BONDS_SIGNAL.signal(());
}

// ── LoRa enable/disable ────────────────────────────────────────────────────

fn label_lora_enabled() -> &'static str {
    if crate::LORA_DISABLED.load(Ordering::Relaxed) {
        "LoRa: OFF"
    } else {
        "LoRa: ON"
    }
}

// ── Set Name (via text entry) ───────────────────────────────────────────────

fn on_name_complete(name: &[u8]) {
    crate::update_node_name(name);
    #[cfg(all(feature = "mesh", feature = "embassy-base"))]
    crate::NODE_NAME_CHANGED_SIGNAL.signal(());
}

/// Menu action: jump straight to the QR-share screen so a peer can scan
/// the meshcore://contact/add URL.
fn action_show_qr() {
    crate::with_display_state_mut!(|s| s.set_active_screen(crate::SCREEN_QR));
}

fn action_set_name() {
    #[cfg(feature = "embassy-base")]
    let prefill = crate::NODE_NAME.lock(|cell| {
        let s = cell.borrow();
        let mut buf = [0u8; 31];
        let n = s.len().min(31);
        buf[..n].copy_from_slice(s.as_bytes().get(..n).unwrap_or(&[]));
        (buf, n)
    });
    #[cfg(feature = "simulator")]
    let prefill = {
        let guard = crate::NODE_NAME.lock().unwrap();
        let s = guard.borrow();
        let mut buf = [0u8; 31];
        let n = s.len().min(31);
        buf[..n].copy_from_slice(s.as_bytes().get(..n).unwrap_or(&[]));
        (buf, n)
    };
    crate::text_entry::begin(
        &prefill.0[..prefill.1],
        31,
        on_name_complete,
        "Set Node Name",
    );
}

fn action_lora_toggle() {
    let cur = crate::LORA_DISABLED.load(Ordering::Relaxed);
    crate::LORA_DISABLED.store(!cur, Ordering::Relaxed);
    #[cfg(feature = "embassy-base")]
    crate::LORA_DISABLED_CHANGED.signal(());
}

fn fmt_timezone(buf: &mut heapless::String<24>) {
    use core::fmt::Write;
    let offset = crate::TIMEZONE_OFFSET
        .load(Ordering::Relaxed)
        .clamp(-12, 14);
    let _ = write!(buf, "UTC{:+}", offset);
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

#[cfg(feature = "watch")]
fn fmt_alarm_hour(buf: &mut heapless::String<24>) {
    use core::fmt::Write;
    let _ = write!(buf, "Hour: {:02}", crate::watch::alarm_hour().min(23));
}

#[cfg(feature = "watch")]
fn fmt_alarm_minute(buf: &mut heapless::String<24>) {
    use core::fmt::Write;
    let _ = write!(buf, "Min: {:02}", crate::watch::alarm_minute().min(59));
}

#[cfg(feature = "watch")]
fn label_alarm_enabled() -> &'static str {
    crate::watch::alarm_enabled_label()
}

#[cfg(feature = "watch")]
fn label_alarm_days() -> &'static str {
    crate::watch::alarm_days_label()
}

#[cfg(feature = "watch")]
fn fmt_alarm_tone(buf: &mut heapless::String<24>) {
    let _ = buf.push_str(&crate::watch::alarm_tone_label());
}

// Per-day toggle labels — one fn per day so the menu's `fn()`-typed action
// pointers can call them.
#[cfg(feature = "watch")]
mod alarm_day_actions {
    pub fn toggle_mon() {
        crate::watch::alarm_toggle_day(0)
    }
    pub fn toggle_tue() {
        crate::watch::alarm_toggle_day(1)
    }
    pub fn toggle_wed() {
        crate::watch::alarm_toggle_day(2)
    }
    pub fn toggle_thu() {
        crate::watch::alarm_toggle_day(3)
    }
    pub fn toggle_fri() {
        crate::watch::alarm_toggle_day(4)
    }
    pub fn toggle_sat() {
        crate::watch::alarm_toggle_day(5)
    }
    pub fn toggle_sun() {
        crate::watch::alarm_toggle_day(6)
    }
    pub fn label_mon() -> &'static str {
        if crate::watch::alarm_day_enabled(0) {
            "Mon: On"
        } else {
            "Mon: Off"
        }
    }
    pub fn label_tue() -> &'static str {
        if crate::watch::alarm_day_enabled(1) {
            "Tue: On"
        } else {
            "Tue: Off"
        }
    }
    pub fn label_wed() -> &'static str {
        if crate::watch::alarm_day_enabled(2) {
            "Wed: On"
        } else {
            "Wed: Off"
        }
    }
    pub fn label_thu() -> &'static str {
        if crate::watch::alarm_day_enabled(3) {
            "Thu: On"
        } else {
            "Thu: Off"
        }
    }
    pub fn label_fri() -> &'static str {
        if crate::watch::alarm_day_enabled(4) {
            "Fri: On"
        } else {
            "Fri: Off"
        }
    }
    pub fn label_sat() -> &'static str {
        if crate::watch::alarm_day_enabled(5) {
            "Sat: On"
        } else {
            "Sat: Off"
        }
    }
    pub fn label_sun() -> &'static str {
        if crate::watch::alarm_day_enabled(6) {
            "Sun: On"
        } else {
            "Sun: Off"
        }
    }
}

#[cfg(feature = "watch")]
static ALARM_DAYS_ITEMS: [MenuItem; 8] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: alarm_day_actions::label_mon,
        kind: MenuItemKind::Action(alarm_day_actions::toggle_mon),
    },
    MenuItem {
        label: alarm_day_actions::label_tue,
        kind: MenuItemKind::Action(alarm_day_actions::toggle_tue),
    },
    MenuItem {
        label: alarm_day_actions::label_wed,
        kind: MenuItemKind::Action(alarm_day_actions::toggle_wed),
    },
    MenuItem {
        label: alarm_day_actions::label_thu,
        kind: MenuItemKind::Action(alarm_day_actions::toggle_thu),
    },
    MenuItem {
        label: alarm_day_actions::label_fri,
        kind: MenuItemKind::Action(alarm_day_actions::toggle_fri),
    },
    MenuItem {
        label: alarm_day_actions::label_sat,
        kind: MenuItemKind::Action(alarm_day_actions::toggle_sat),
    },
    MenuItem {
        label: alarm_day_actions::label_sun,
        kind: MenuItemKind::Action(alarm_day_actions::toggle_sun),
    },
];

#[cfg(feature = "watch")]
static ALARM_ITEMS: [MenuItem; 6] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_alarm_hour,
            inc: crate::watch::alarm_inc_hour,
            dec: crate::watch::alarm_dec_hour,
        },
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_alarm_minute,
            inc: crate::watch::alarm_inc_minute,
            dec: crate::watch::alarm_dec_minute,
        },
    },
    MenuItem {
        label: label_alarm_days,
        kind: MenuItemKind::Submenu(&ALARM_DAYS_ITEMS),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_alarm_tone,
            inc: crate::watch::alarm_inc_melody,
            dec: crate::watch::alarm_dec_melody,
        },
    },
    MenuItem {
        label: label_alarm_enabled,
        kind: MenuItemKind::Action(crate::watch::alarm_toggle_enabled),
    },
];

// ── Events submenu (imported one-shot alarms in slots 1..N_ALARMS) ──────────

/// Format slot `slot`'s read-only label into the menu buffer.
///   * disabled         → `1: empty`     (rendered only via legacy callers; the
///     SlotInfo renderer hides empties via `slot_alarm_visible`.)
///   * recurring        → `1: 14:30 daily`
///   * one-shot         → `1: 14:30 08-16`
#[cfg(feature = "watch")]
fn fmt_alarm_slot(buf: &mut heapless::String<24>, slot: u8) {
    use core::fmt::Write;
    let s = slot as usize;
    if !crate::watch::alarm_enabled_n(s) {
        let _ = write!(buf, "{}: empty", slot);
        return;
    }
    let h = crate::watch::alarm_hour_n(s);
    let m = crate::watch::alarm_minute_n(s);
    if crate::watch::alarm_is_one_shot_n(s) {
        let _ = write!(
            buf,
            "{}: {:02}:{:02} {:02}-{:02}",
            slot,
            h,
            m,
            crate::watch::alarm_month_n(s),
            crate::watch::alarm_day_n(s),
        );
    } else {
        let _ = write!(buf, "{}: {:02}:{:02} daily", slot, h, m);
    }
}

/// Visibility predicate: only show enabled slots so the menu doesn't
/// scroll past 31 empties when only a couple of events are loaded.
#[cfg(feature = "watch")]
fn slot_alarm_visible(slot: u8) -> bool {
    crate::watch::alarm_enabled_n(slot as usize)
}

/// Action: drop a "Quick test" event 5 minutes from now in the first
/// empty slot.  Useful for verifying the alarm path without USB.
/// Silently no-ops if the wall clock isn't synced or all slots are
/// taken — the new event becomes visible on the Calendar grid (red
/// dot) and the Clock face (bell + HH:MM), so no toast confirmation
/// is needed.
#[cfg(all(feature = "watch", feature = "embassy-base"))]
fn action_add_quick_test() {
    let _ = crate::watch::add_quick_event(5, b"Quick test");
}
#[cfg(all(feature = "watch", not(feature = "embassy-base")))]
fn action_add_quick_test() {}

/// Expand a literal list of slot indices into menu rows wrapped by
/// Back / slot rows / Clear all.  Events are populated by
/// `import_alarms_from_fat12` at boot from `ALARMS.ICS` — there's no
/// on-device add path; this submenu is observe-only plus a "Clear all"
/// destructive action.  One shared formatter + visibility predicate
/// handles all 31 slot rows via `MenuItemKind::SlotInfo`.
#[cfg(feature = "watch")]
macro_rules! events_items {
    ($($n:literal),* $(,)?) => {
        [
            MenuItem { label: || "< Back", kind: MenuItemKind::Back },
            // Drops a "Quick test" event 5 min from now — silently
            // no-ops without a synced wall clock; the new event shows
            // up via the Calendar dot + Clock-face bell.
            MenuItem {
                label: || "Quick test +5min",
                kind: MenuItemKind::Action(action_add_quick_test),
            },
            MenuItem { label: || "", kind: MenuItemKind::Separator },
            $(MenuItem {
                label: || "",
                kind: MenuItemKind::SlotInfo {
                    format: fmt_alarm_slot,
                    visible: slot_alarm_visible,
                    slot: $n,
                },
            },)*
            MenuItem { label: || "", kind: MenuItemKind::Separator },
            MenuItem {
                label: || "Clear all",
                kind: MenuItemKind::Action(crate::watch::clear_imported_alarms),
            },
        ]
    };
}

// 1 (Back) + 1 (Quick test) + 1 (Sep) + 31 (slot rows) + 1 (Sep) + 1 (Clear) =
// 36.
#[cfg(feature = "watch")]
static EVENTS_ITEMS: [MenuItem; 36] = events_items!(
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31,
);

#[cfg(feature = "game")]
fn play_melody(_index: usize) {
    #[cfg(feature = "embassy-base")]
    crate::fw::buzzer::play(_index);
    crate::game::lifecycle::play();
}

// ── Static item arrays
// ────────────────────────────────────────────────────────

#[cfg(feature = "game")]
static MELODY_ITEMS: [MenuItem; 7] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: || "Startup",
        kind: MenuItemKind::Action(|| play_melody(0)),
    },
    MenuItem {
        label: || "Rickroll",
        kind: MenuItemKind::Action(|| play_melody(1)),
    },
    MenuItem {
        label: || "Imp. March",
        kind: MenuItemKind::Action(|| play_melody(2)),
    },
    MenuItem {
        label: || "Sandstorm",
        kind: MenuItemKind::Action(|| play_melody(3)),
    },
    MenuItem {
        label: || "Pink Panther",
        kind: MenuItemKind::Action(|| play_melody(4)),
    },
    MenuItem {
        label: || "Trololo",
        kind: MenuItemKind::Action(|| play_melody(5)),
    },
];

static BLE_ITEMS: [MenuItem; 4] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: label_ble_name,
        kind: MenuItemKind::Action(|| {}),
    },
    MenuItem {
        label: label_ble_enabled,
        kind: MenuItemKind::Action(action_ble_toggle),
    },
    MenuItem {
        label: || "Clear pairings",
        kind: MenuItemKind::Confirm {
            prompt: "Clear pairings",
            action: action_clear_bonds,
        },
    },
];

static ADVERTS_ITEMS: [MenuItem; 5] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: || "Send now",
        kind: MenuItemKind::Action(action_advert_send_now),
    },
    MenuItem {
        label: label_advert_enabled,
        kind: MenuItemKind::Action(action_advert_toggle),
    },
    MenuItem {
        label: label_advert_interval,
        kind: MenuItemKind::Stepper {
            inc: action_advert_interval_inc,
            dec: action_advert_interval_dec,
        },
    },
    MenuItem {
        label: label_advert_loc,
        kind: MenuItemKind::Action(action_advert_loc),
    },
];

static LORA_MENU_ITEMS: [MenuItem; 5] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: label_lora_enabled,
        kind: MenuItemKind::Action(action_lora_toggle),
    },
    MenuItem {
        label: label_boost_rx,
        kind: MenuItemKind::Action(action_boost_rx),
    },
    MenuItem {
        label: || "Radio Presets",
        kind: MenuItemKind::Submenu(&LORA_RADIO_ITEMS),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_tx_power,
            inc: action_tx_power_inc,
            dec: action_tx_power_dec,
        },
    },
];

static MESHCORE_MENU_ITEMS: [MenuItem; 12] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: || "Set Name",
        kind: MenuItemKind::Action(action_set_name),
    },
    MenuItem {
        label: || "My QR",
        kind: MenuItemKind::Action(action_show_qr),
    },
    MenuItem {
        label: label_client_repeat,
        kind: MenuItemKind::ConditionalConfirm {
            prompt: "Only enable repeating\nwhen no repeaters are\naround!",
            needs_confirm: || !crate::LORA_CLIENT_REPEAT.load(Ordering::Relaxed),
            action: action_client_repeat_toggle,
        },
    },
    MenuItem {
        label: || "Adverts",
        kind: MenuItemKind::Submenu(&ADVERTS_ITEMS),
    },
    MenuItem {
        label: label_telemetry_share,
        kind: MenuItemKind::Action(action_telemetry_toggle),
    },
    MenuItem {
        label: label_multi_acks,
        kind: MenuItemKind::Stepper {
            inc: action_multi_acks_inc,
            dec: action_multi_acks_dec,
        },
    },
    MenuItem {
        label: label_path_hash,
        kind: MenuItemKind::Stepper {
            inc: action_path_hash_inc,
            dec: action_path_hash_dec,
        },
    },
    MenuItem {
        label: label_ignore_blink,
        kind: MenuItemKind::Action(action_ignore_blink),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::Separator,
    },
    MenuItem {
        label: || "Reset channels",
        kind: MenuItemKind::Confirm {
            prompt: "Reset channels",
            action: action_reset_channels,
        },
    },
    MenuItem {
        label: || "Reset contacts",
        kind: MenuItemKind::Confirm {
            prompt: "Reset contacts",
            action: action_reset_contacts,
        },
    },
];

// ── Sounds submenu (notification tones per event, mesh only) ────────────────

#[cfg(feature = "mesh")]
static SOUNDS_ITEMS: [MenuItem; 4] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: sound_menu::fmt_pm,
            inc: sound_menu::pm_inc,
            dec: sound_menu::pm_dec,
        },
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: sound_menu::fmt_channel,
            inc: sound_menu::channel_inc,
            dec: sound_menu::channel_dec,
        },
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: sound_menu::fmt_contact,
            inc: sound_menu::contact_inc,
            dec: sound_menu::contact_dec,
        },
    },
];

const SETTINGS_ITEMS_LEN: usize =
    12 + if cfg!(feature = "watch") { 2 } else { 0 } + if cfg!(feature = "mesh") { 1 } else { 0 };

static SETTINGS_ITEMS: [MenuItem; SETTINGS_ITEMS_LEN] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: label_boot_chime,
        kind: MenuItemKind::Action(action_boot_chime_toggle),
    },
    MenuItem {
        label: || "Qwiic Scan",
        kind: MenuItemKind::Action(|| {
            crate::fw::qwiic::open();
        }),
    },
    MenuItem {
        label: || "Bluetooth",
        kind: MenuItemKind::Submenu(&BLE_ITEMS),
    },
    MenuItem {
        label: || "LoRa",
        kind: MenuItemKind::Submenu(&LORA_MENU_ITEMS),
    },
    MenuItem {
        label: || "MeshCore",
        kind: MenuItemKind::Submenu(&MESHCORE_MENU_ITEMS),
    },
    #[cfg(feature = "mesh")]
    MenuItem {
        label: || "Sounds",
        kind: MenuItemKind::Submenu(&SOUNDS_ITEMS),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_timezone,
            inc: action_tz_inc,
            dec: action_tz_dec,
        },
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_epd_lut_speed,
            inc: action_epd_lut_speed_inc,
            dec: action_epd_lut_speed_dec,
        },
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_epd_temp_bias,
            inc: action_epd_temp_bias_inc,
            dec: action_epd_temp_bias_dec,
        },
    },
    #[cfg(feature = "watch")]
    MenuItem {
        label: || "Alarm",
        kind: MenuItemKind::Submenu(&ALARM_ITEMS),
    },
    #[cfg(feature = "watch")]
    MenuItem {
        label: || "Events",
        kind: MenuItemKind::Submenu(&EVENTS_ITEMS),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::Separator,
    },
    MenuItem {
        label: || "Replay Sponsors",
        kind: MenuItemKind::Action(|| {
            #[cfg(feature = "embassy-base")]
            crate::fw::sponsors::request_clear();
        }),
    },
    MenuItem {
        label: || "Factory reset",
        kind: MenuItemKind::Confirm {
            prompt: "Factory reset",
            action: action_factory_reset,
        },
    },
];

static BORNAGOTCHI_ITEMS: [MenuItem; 7] = [
    MenuItem {
        label: || "< Back",
        kind: MenuItemKind::Back,
    },
    MenuItem {
        label: label_game_mute,
        kind: MenuItemKind::Action(action_game_mute_toggle),
    },
    MenuItem {
        label: || "Disable Game",
        kind: MenuItemKind::Action(|| {}),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::ValueStepper {
            format: fmt_game_mode,
            inc: action_game_mode_next,
            dec: action_game_mode_next,
        },
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::Separator,
    },
    MenuItem {
        label: || "Reset Pet",
        kind: MenuItemKind::Confirm {
            prompt: "Reset pet?",
            action: || {
                #[cfg(feature = "game")]
                crate::game::pet_select::open_new_generation();
            },
        },
    },
    MenuItem {
        label: || "Unicorn Realm",
        kind: MenuItemKind::Action(|| {
            #[cfg(feature = "game")]
            crate::game::realm_view::open();
        }),
    },
];

fn fmt_game_mode(buf: &mut heapless::String<24>) {
    use core::fmt::Write;
    #[cfg(feature = "game")]
    {
        let mode = crate::game::settings::pending_mode();
        let needs_reboot = crate::game::settings::pending_differs_from_active();
        let suffix = if needs_reboot { "*" } else { "" };
        let _ = write!(buf, "Mode: {}{}", mode.label(), suffix);
    }
    #[cfg(not(feature = "game"))]
    {
        let _ = write!(buf, "Mode: -");
    }
}

fn action_game_mode_next() {
    #[cfg(feature = "game")]
    {
        use crate::game::engine::thresholds::Mode;
        let next = match crate::game::settings::pending_mode() {
            Mode::Classic => Mode::Casual,
            Mode::Casual => Mode::Classic,
        };
        crate::game::settings::request_mode_change(next);
    }
}

#[cfg(feature = "game")]
static GAME_ITEMS: [MenuItem; 2] = [
    MenuItem {
        label: || "BornPets",
        kind: MenuItemKind::Action(|| {}),
    },
    MenuItem {
        label: || "Play Music",
        kind: MenuItemKind::Submenu(&MELODY_ITEMS),
    },
];

static ABOUT_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "< Back",
    kind: MenuItemKind::Back,
}];

/// Sentinel item list for the LoRa radio preset picker. The list is a single
/// `Back` entry — the submenu is rendered and dispatched entirely via the
/// custom `draw_lora_radio` path (see `DisplayState::dispatch_button`).
static LORA_RADIO_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "< Back",
    kind: MenuItemKind::Back,
}];

/// Community-suggested LoRa radio presets (matches the MeshCore app's
/// `suggested_radio_settings` list).
pub struct LoRaPreset {
    pub title: &'static str,
    pub freq_hz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    pub cr: u8,
}

pub static LORA_PRESETS: &[LoRaPreset] = &[
    // BornHack-event preset — ETSI g4 sub-band, spectrum-isolated from the
    // standard EU/UK Narrow channel at 869.618 MHz so badges on this preset
    // and stock MeshCore badges form physically separate networks.  TX power
    // is left at whatever the user has set; for full ETSI compliance with
    // 100 % duty cycle ops should set it to +7 dBm via the Power menu.
    LoRaPreset {
        title: "BornHack Turbo",
        freq_hz: 869_850_000,
        bw_hz: 250_000,
        sf: 8,
        cr: 5, // CR 4/5
    },
    LoRaPreset {
        title: "Australia",
        freq_hz: 915_800_000,
        bw_hz: 250_000,
        sf: 10,
        cr: 5,
    },
    LoRaPreset {
        title: "Australia (Narrow)",
        freq_hz: 916_575_000,
        bw_hz: 62_500,
        sf: 7,
        cr: 8,
    },
    LoRaPreset {
        title: "Australia: SA, WA",
        freq_hz: 923_125_000,
        bw_hz: 62_500,
        sf: 8,
        cr: 8,
    },
    LoRaPreset {
        title: "Australia: QLD",
        freq_hz: 923_125_000,
        bw_hz: 62_500,
        sf: 8,
        cr: 5,
    },
    LoRaPreset {
        title: "EU/UK (Narrow)",
        freq_hz: 869_618_000,
        bw_hz: 62_500,
        sf: 8,
        cr: 8,
    },
    LoRaPreset {
        title: "NL narrow (75)",
        freq_hz: 869_618_000,
        bw_hz: 62_500,
        sf: 7,
        cr: 5,
    },
    LoRaPreset {
        title: "EU/UK (Deprecated)",
        freq_hz: 869_525_000,
        bw_hz: 250_000,
        sf: 11,
        cr: 5,
    },
    LoRaPreset {
        title: "Czech (Narrow)",
        freq_hz: 869_432_000,
        bw_hz: 62_500,
        sf: 7,
        cr: 5,
    },
    LoRaPreset {
        title: "EU 433 (Long)",
        freq_hz: 433_650_000,
        bw_hz: 250_000,
        sf: 11,
        cr: 5,
    },
    LoRaPreset {
        title: "New Zealand",
        freq_hz: 917_375_000,
        bw_hz: 250_000,
        sf: 11,
        cr: 5,
    },
    LoRaPreset {
        title: "NZ (Narrow)",
        freq_hz: 917_375_000,
        bw_hz: 62_500,
        sf: 7,
        cr: 5,
    },
    LoRaPreset {
        title: "Portugal 433",
        freq_hz: 433_375_000,
        bw_hz: 62_500,
        sf: 9,
        cr: 6,
    },
    LoRaPreset {
        title: "Portugal 868",
        freq_hz: 869_618_000,
        bw_hz: 62_500,
        sf: 7,
        cr: 6,
    },
    LoRaPreset {
        title: "Switzerland",
        freq_hz: 869_618_000,
        bw_hz: 62_500,
        sf: 8,
        cr: 8,
    },
    LoRaPreset {
        title: "USA/Canada",
        freq_hz: 910_525_000,
        bw_hz: 62_500,
        sf: 7,
        cr: 5,
    },
    LoRaPreset {
        title: "Vietnam (Narrow)",
        freq_hz: 920_250_000,
        bw_hz: 62_500,
        sf: 8,
        cr: 5,
    },
    LoRaPreset {
        title: "Vietnam (Deprecated)",
        freq_hz: 920_250_000,
        bw_hz: 250_000,
        sf: 11,
        cr: 5,
    },
];

/// Returns the index of the preset that matches the current LoRa atomics, or
/// `LORA_PRESETS.len()` to indicate "Custom" (no matching preset).
fn current_lora_preset_index() -> u8 {
    let freq = crate::LORA_FREQ_HZ.load(Ordering::Relaxed);
    let bw = crate::LORA_BW_HZ.load(Ordering::Relaxed);
    let sf = crate::LORA_SF.load(Ordering::Relaxed);
    let cr = crate::LORA_CR.load(Ordering::Relaxed);
    for (i, p) in LORA_PRESETS.iter().enumerate() {
        if p.freq_hz == freq && p.bw_hz == bw && p.sf == sf && p.cr == cr {
            return i as u8;
        }
    }
    LORA_PRESETS.len() as u8
}

fn apply_lora_preset(idx: usize) {
    let p = &LORA_PRESETS[idx];
    crate::LORA_FREQ_HZ.store(p.freq_hz, Ordering::Relaxed);
    crate::LORA_BW_HZ.store(p.bw_hz, Ordering::Relaxed);
    crate::LORA_SF.store(p.sf, Ordering::Relaxed);
    crate::LORA_CR.store(p.cr, Ordering::Relaxed);
    #[cfg(feature = "embassy-base")]
    crate::LORA_RADIO_CHANGED_SIGNAL.signal(());
}

static MAIN_ITEMS: [MenuItem; 4] = [
    MenuItem {
        label: || "Bornagotchi",
        kind: MenuItemKind::Submenu(&BORNAGOTCHI_ITEMS),
    },
    MenuItem {
        label: || "Settings",
        kind: MenuItemKind::Submenu(&SETTINGS_ITEMS),
    },
    MenuItem {
        label: || "",
        kind: MenuItemKind::Separator,
    },
    MenuItem {
        label: || "About",
        kind: MenuItemKind::Submenu(&ABOUT_ITEMS),
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
    label: || "Advert",
    kind: MenuItemKind::Action(|| {}),
}];

static TOKEN_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "Token",
    kind: MenuItemKind::Action(|| {}),
}];

#[cfg(feature = "watch")]
static WATCH_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "Clock",
    kind: MenuItemKind::Action(|| {}),
}];

#[cfg(feature = "watch")]
static CALENDAR_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "Calendar",
    kind: MenuItemKind::Action(|| {}),
}];

/// Big-name conference-badge screen.  Pure renderer — no menu items
/// (the screen takes over the whole panel and arrows fall through to
/// the icon-grid screen-nav).  Always enabled.
static NAME_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "Name",
    kind: MenuItemKind::Action(|| {}),
}];

/// QR-share screen (mesh-only).  Single placeholder entry so the menu
/// button on the QR screen pops a no-op modal; the QR fills the panel
/// and arrow keys carousel away normally.
#[cfg(feature = "mesh")]
static QR_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "My QR",
    kind: MenuItemKind::Action(|| {}),
}];

#[cfg(not(feature = "mesh"))]
static QR_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "QR",
    kind: MenuItemKind::Action(|| {}),
}];

// ── DISPLAY_STATE
// ─────────────────────────────────────────────────────────────

pub const SCREEN_COUNT: usize = ScreenId::COUNT;

// The game screen placeholder when the feature is disabled — never navigated
// to.
#[cfg(not(feature = "game"))]
static GAME_ITEMS: [MenuItem; 1] = [MenuItem {
    label: || "BornPets",
    kind: MenuItemKind::Action(|| {}),
}];

#[cfg(feature = "game")]
const GAME_ENABLED: bool = true;
#[cfg(not(feature = "game"))]
const GAME_ENABLED: bool = false;

#[cfg(feature = "watch")]
const WATCH_ENABLED: bool = true;
#[cfg(not(feature = "watch"))]
const WATCH_ENABLED: bool = false;

#[cfg(feature = "simulator")]
use std::sync::Mutex;

#[cfg(feature = "embassy-base")]
use embassy_sync::blocking_mutex::{Mutex, raw::ThreadModeRawMutex};

#[cfg(feature = "embassy-base")]
type DisplayMutex = Mutex<ThreadModeRawMutex, RefCell<DisplayState<SCREEN_COUNT>>>;
#[cfg(feature = "simulator")]
type DisplayMutex = Mutex<RefCell<DisplayState<SCREEN_COUNT>>>;

pub static DISPLAY_STATE: DisplayMutex = DisplayMutex::new(RefCell::new(DisplayState::new(
    [
        ScreenState::new(&GAME_ITEMS),
        ScreenState::new(&MAIN_ITEMS),
        ScreenState::new(&PM_ITEMS),
        ScreenState::new(&LORA_ITEMS),
        ScreenState::new(&ADVERT_ITEMS),
        ScreenState::new(&TOKEN_ITEMS),
        #[cfg(feature = "watch")]
        ScreenState::new(&WATCH_ITEMS),
        #[cfg(feature = "watch")]
        ScreenState::new(&CALENDAR_ITEMS),
        ScreenState::new(&NAME_ITEMS),
        ScreenState::new(&QR_ITEMS),
    ],
    [
        GAME_ENABLED,
        true,
        true,
        true,
        true,
        true,
        #[cfg(feature = "watch")]
        WATCH_ENABLED,
        #[cfg(feature = "watch")]
        WATCH_ENABLED,
        true,
        // QR screen always enabled — when built without `mesh` the
        // module still exists but draws a "(key not ready)" placeholder.
        true,
    ],
)));

// ── Scrolling menu renderer
// ───────────────────────────────────────────────────

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
pub fn draw_menu<D>(
    display: &mut D,
    items: &[MenuItem],
    pos: usize,
    stepper_active: bool,
) -> Result<(), D::Error>
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

        if item_idx >= 0
            && let Some(item) = items.get(item_idx as usize)
        {
            let fg = if is_center { WHITE } else { BLACK };
            // Hidden SlotInfo: render nothing — the row stays blank
            // (the center row keeps its black highlight, but that's
            // an edge case nav can't normally land on).
            if matches!(
                item.kind,
                MenuItemKind::SlotInfo { visible, slot, .. } if !visible(slot)
            ) {
                continue;
            }
            if matches!(item.kind, MenuItemKind::Separator) {
                // Draw a thin horizontal rule across the row
                Rectangle::new(Point::new(MENU_X + 8, text_y), Size::new(MENU_W - 16, 1))
                    .into_styled(PrimitiveStyle::with_fill(fg))
                    .draw(display)?;
            } else {
                let mut label: heapless::String<24> = heapless::String::new();
                let is_stepper = matches!(
                    item.kind,
                    MenuItemKind::Stepper { .. } | MenuItemKind::ValueStepper { .. }
                );
                if is_stepper {
                    if stepper_active && is_center {
                        let _ = label.push_str("[ ");
                    } else {
                        let _ = label.push_str("< ");
                    }
                }
                // ValueStepper / Info write their own value via `format`;
                // every other kind uses the static `MenuItem::label`
                // callback.
                match item.kind {
                    MenuItemKind::ValueStepper { format, .. } | MenuItemKind::Info { format } => {
                        format(&mut label)
                    }
                    MenuItemKind::SlotInfo { format, slot, .. } => format(&mut label, slot),
                    _ => {
                        let _ = label.push_str((item.label)());
                    }
                }
                if matches!(item.kind, MenuItemKind::Submenu(_)) {
                    let _ = label.push_str(" >");
                } else if is_stepper {
                    if stepper_active && is_center {
                        let _ = label.push_str(" ]");
                    } else {
                        let _ = label.push_str(" >");
                    }
                }
                Text::with_text_style(
                    &label,
                    Point::new(MENU_X + MENU_W as i32 / 2, text_y),
                    MonoTextStyle::new(&FONT_7X13_BOLD, fg),
                    text_style,
                )
                .draw(display)?;
            }
        }
    }

    Ok(())
}

const ABOUT_PAGES: u8 = 8;

/// Draw the About / credits screen.  `page` selects which page to show.
pub fn draw_about<D>(display: &mut D, page: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let center = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();

    let x = 76; // center

    // Border
    Rectangle::new(Point::new(0, 0), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;
    Rectangle::new(Point::new(2, 2), Size::new(148, 148))
        .into_styled(PrimitiveStyle::with_stroke(RED, 1))
        .draw(display)?;

    // Title area — same on every page
    let mut y = 12;
    let lh = 16;
    Text::with_text_style("Badge Team 2026", Point::new(x, y), font, center).draw(display)?;
    y += lh;
    Text::with_text_style("Cyber AEgg", Point::new(x, y), font, center).draw(display)?;

    // Separator line
    Rectangle::new(Point::new(10, y + lh), Size::new(132, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    y += lh + 6;

    match page {
        0 => {
            Text::with_text_style("-- PCB --", Point::new(x, y), font, center).draw(display)?;
            y += lh + 4;
            Text::with_text_style("Ranzbak", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("Renze", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("CMPXCHG", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("PA3WEG", Point::new(x, y), font, center).draw(display)?;
        }
        1 => {
            Text::with_text_style("-- Firmware --", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh + 4;
            Text::with_text_style("Ranzbak", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("AnneJan", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("Orange_Murker", Point::new(x, y), font, center).draw(display)?;
        }
        2 => {
            Text::with_text_style("-- Case --", Point::new(x, y), font, center).draw(display)?;
            y += lh + 4;
            Text::with_text_style("bulbdk", Point::new(x, y), font, center).draw(display)?;
        }
        3 => {
            Text::with_text_style("-- Game --", Point::new(x, y), font, center).draw(display)?;
            y += lh + 4;
            Text::with_text_style("at-boy", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("Ranzbak", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("AnneJan", Point::new(x, y), font, center).draw(display)?;
        }
        4 => {
            Text::with_text_style("-- Graphics --", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh + 4;
            Text::with_text_style("Ankate", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("NightOwlNL", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("Lilium", Point::new(x, y), font, center).draw(display)?;
        }
        5 => {
            Text::with_text_style("-- Sponsors --", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh + 4;
            Text::with_text_style("Thank you to our", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh;
            Text::with_text_style("generous sponsors", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh;
            Text::with_text_style("for supporting the", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh;
            Text::with_text_style("Cyber AEgg badge!", Point::new(x, y), font, center)
                .draw(display)?;
        }
        6 => {
            Text::with_text_style("-- Sponsors --", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh + 4;
            Text::with_text_style("Nordic Semiconductor", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh;
            Text::with_text_style("nordicsemi.com", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh + 4;
            Text::with_text_style("Procolix", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("procolix.com", Point::new(x, y), font, center).draw(display)?;
        }
        _ => {
            Text::with_text_style("-- Sponsors --", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh + 4;
            Text::with_text_style("Allnet", Point::new(x, y), font, center).draw(display)?;
            y += lh;
            Text::with_text_style("allnet.de", Point::new(x, y), font, center).draw(display)?;
            y += lh + 4;
            Text::with_text_style("Mollerup Automation", Point::new(x, y), font, center)
                .draw(display)?;
            y += lh;
            Text::with_text_style("mollerup.info", Point::new(x, y), font, center).draw(display)?;
        }
    }

    // Page indicator at the bottom
    let mut indicator: heapless::String<8> = heapless::String::new();
    let _ = core::fmt::Write::write_fmt(
        &mut indicator,
        format_args!("< {}/{} >", page + 1, ABOUT_PAGES),
    );
    Text::with_text_style(&indicator, Point::new(x, 136), font, center).draw(display)?;

    Ok(())
}

/// Draw the LoRa radio preset picker.
///
/// `page` is the preset index into [`LORA_PRESETS`]. If it equals
/// `LORA_PRESETS.len()` the device is on a Custom (non-preset) setting and the
/// current atomics are displayed instead — Custom is not selectable via the
/// Left/Right navigation.
pub fn draw_lora_radio<D>(display: &mut D, page: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let font_bold = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let center = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let left = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Left)
        .build();

    let x = 76;

    // Borders
    Rectangle::new(Point::new(0, 0), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;
    Rectangle::new(Point::new(2, 2), Size::new(148, 148))
        .into_styled(PrimitiveStyle::with_stroke(RED, 1))
        .draw(display)?;

    // Title
    let mut y = 8;
    Text::with_text_style("LoRa Radio", Point::new(x, y), font_bold, center).draw(display)?;
    y += 16;

    // Separator
    Rectangle::new(Point::new(10, y), Size::new(132, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    y += 6;

    // Preset title row — highlighted black background, white text.
    let is_custom = (page as usize) >= LORA_PRESETS.len();
    let (title, freq, bw, sf, cr) = if is_custom {
        (
            "Custom",
            crate::LORA_FREQ_HZ.load(Ordering::Relaxed),
            crate::LORA_BW_HZ.load(Ordering::Relaxed),
            crate::LORA_SF.load(Ordering::Relaxed),
            crate::LORA_CR.load(Ordering::Relaxed),
        )
    } else {
        let p = &LORA_PRESETS[page as usize];
        (p.title, p.freq_hz, p.bw_hz, p.sf, p.cr)
    };

    // The displayed preset is "active" when its values match the current
    // atomics — i.e. the user pressed Fire on this preset. Custom is always
    // active (it reflects the live values by definition).
    let is_active = is_custom
        || (crate::LORA_FREQ_HZ.load(Ordering::Relaxed) == freq
            && crate::LORA_BW_HZ.load(Ordering::Relaxed) == bw
            && crate::LORA_SF.load(Ordering::Relaxed) == sf
            && crate::LORA_CR.load(Ordering::Relaxed) == cr);

    Rectangle::new(Point::new(6, y - 2), Size::new(140, 18))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let font_white = MonoTextStyle::new(&FONT_7X13_BOLD, WHITE);
    let mut title_line: heapless::String<40> = heapless::String::new();
    let _ = title_line.push_str(if is_active { "* " } else { "  " });
    let _ = title_line.push_str(title);
    Text::with_text_style(&title_line, Point::new(x, y), font_white, center).draw(display)?;
    y += 22;

    // Settings lines
    let lh = 14;
    let mut line: heapless::String<32> = heapless::String::new();
    let mhz_int = freq / 1_000_000;
    let mhz_frac = (freq % 1_000_000) / 1_000; // kHz component, 0..999
    let _ = core::fmt::Write::write_fmt(
        &mut line,
        format_args!("Freq: {}.{:03} MHz", mhz_int, mhz_frac),
    );
    Text::with_text_style(&line, Point::new(10, y), font_bold, left).draw(display)?;
    y += lh;

    line.clear();
    if bw >= 1000 {
        let khz_int = bw / 1000;
        let khz_frac = (bw % 1000) / 100;
        if khz_frac == 0 {
            let _ = core::fmt::Write::write_fmt(&mut line, format_args!("BW:   {} kHz", khz_int));
        } else {
            let _ = core::fmt::Write::write_fmt(
                &mut line,
                format_args!("BW:   {}.{} kHz", khz_int, khz_frac),
            );
        }
    } else {
        let _ = core::fmt::Write::write_fmt(&mut line, format_args!("BW:   {} Hz", bw));
    }
    Text::with_text_style(&line, Point::new(10, y), font_bold, left).draw(display)?;
    y += lh;

    line.clear();
    let _ = core::fmt::Write::write_fmt(&mut line, format_args!("SF:   {}", sf));
    Text::with_text_style(&line, Point::new(10, y), font_bold, left).draw(display)?;
    y += lh;

    line.clear();
    let _ = core::fmt::Write::write_fmt(&mut line, format_args!("CR:   4/{}", cr));
    Text::with_text_style(&line, Point::new(10, y), font_bold, left).draw(display)?;

    // Footer: navigation indicator + hint
    let total = LORA_PRESETS.len() as u8;
    let mut indicator: heapless::String<24> = heapless::String::new();
    if is_custom {
        let _ = core::fmt::Write::write_fmt(&mut indicator, format_args!("< Custom >"));
    } else {
        let _ =
            core::fmt::Write::write_fmt(&mut indicator, format_args!("< {}/{} >", page + 1, total));
    }
    Text::with_text_style(&indicator, Point::new(x, 120), font_bold, center).draw(display)?;
    Text::with_text_style("Fire: apply", Point::new(x, 134), font_bold, center).draw(display)?;

    Ok(())
}

/// Draw a full-screen "Are you sure?" confirmation dialog.
pub fn draw_confirm<D>(display: &mut D, prompt: &str) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let font = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
    let center = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    let x = 76;

    // Full-screen border — red inner frame to signal a destructive action.
    Rectangle::new(Point::new(0, 0), Size::new(152, 152))
        .into_styled(PrimitiveStyle::with_stroke(BLACK, 2))
        .draw(display)?;
    Rectangle::new(Point::new(2, 2), Size::new(148, 148))
        .into_styled(PrimitiveStyle::with_stroke(RED, 2))
        .draw(display)?;

    Text::with_text_style("Are you sure?", Point::new(x, 24), font, center).draw(display)?;

    // Separator
    Rectangle::new(Point::new(10, 46), Size::new(132, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Prompt text — supports multi-line via '\n'. Rendered as white-on-black
    // block if single line, or plain black text if multi-line.
    let line_count = prompt.split('\n').count();
    if line_count <= 1 {
        Rectangle::new(Point::new(6, 56), Size::new(140, 20))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        let font_white = MonoTextStyle::new(&FONT_7X13_BOLD, WHITE);
        Text::with_text_style(prompt, Point::new(x, 60), font_white, center).draw(display)?;
    } else {
        let mut y = 54;
        for line in prompt.split('\n') {
            Text::with_text_style(line, Point::new(x, y), font, center).draw(display)?;
            y += 16;
        }
    }

    // Hints
    let hint_y = if line_count <= 1 {
        96
    } else {
        54 + line_count as i32 * 16 + 8
    };
    Text::with_text_style("Fire  = Yes", Point::new(x, hint_y), font, center).draw(display)?;
    Text::with_text_style("Cancel = No", Point::new(x, hint_y + 18), font, center).draw(display)?;

    Ok(())
}

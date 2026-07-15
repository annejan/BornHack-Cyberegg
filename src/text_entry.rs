//! Joystick-driven text entry for small screens.
//!
//! The screen is split: top half shows the entered text, bottom half shows a
//! hierarchical joystick navigator.
//!
//! # Navigation hierarchy
//!
//! **Root** (center dot, 4 labeled arrows):
//!   ←A-I  ↑J-R  →S-Z  ↓CMD
//!
//! **Letter quadrant** (opposite direction = back):
//!   A-I: ↓abc ←def ↑ghi
//!   J-R: ←jkl ↑mno →pqr
//!   S-Z: ↓stu ↑vwx →yz
//!
//! **CMD** (↑ = back):
//!   ←Backspace  ↓Specials  →Nums/Shift/Clear
//!
//! **Nums/Shift/Clear** (← = back):
//!   ↑Shift  ↓Clear  →Enter/Submit

use core::cell::RefCell;

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_1::FONT_7X13_BOLD;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{Circle, PrimitiveStyle, Rectangle, Triangle};
use embedded_graphics::text::{Alignment, Baseline, Text, TextStyleBuilder};

use crate::menu::ButtonId;
use crate::{BLACK, TriColor, WHITE};

// ── Character tables ─────────────────────────────────────────────────────────

static CHARS_ABC: &[u8] = b"abc";
static CHARS_DEF: &[u8] = b"def";
static CHARS_GHI: &[u8] = b"ghi";
static CHARS_JKL: &[u8] = b"jkl";
static CHARS_MNO: &[u8] = b"mno";
static CHARS_PQR: &[u8] = b"pqr";
static CHARS_STU: &[u8] = b"stu";
static CHARS_VWX: &[u8] = b"vwx";
static CHARS_YZ: &[u8] = b"yz";

const BKSP: u8 = 0x08;
static SPACE_BKSP: &[u8] = &[b' ', BKSP];
static SPECIAL_CHARS: &[u8] = b"_.,()*/+-?#";
static NUMBER_CHARS: &[u8] = b"0123456789";

// ── State machine ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Quadrant {
    Left,
    Up,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputState {
    Root,
    LetterQuad(Quadrant),
    CharPick { table_id: u8, cursor: u8 },
    CmdHub,
    CmdRightHub,
    SpaceBkspPick { cursor: u8 },
    SpecialPick { cursor: u8 },
    NumberPick { cursor: u8 },
}

const MAX_TEXT_LEN: usize = 160;

/// A key from an external (I2C) keyboard, decoded to a text-entry action.
/// Kept hardware-free so this module still builds on the simulator; the
/// keyboard driver ([`crate::fw::i2c_keyboard`]) produces these.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExtKey {
    /// A printable ASCII byte (letters arrive lowercase; Shift upper-cases,
    /// Alt maps to the alt-layer symbol).
    Char(u8),
    Space,
    Backspace,
    Enter,
    /// Toggle the one-shot shift (next letter upper-cased / alt char shifted).
    Shift,
    /// Toggle the one-shot alt (next letter → its alt-layer symbol).
    Alt,
}

pub struct TextEntry {
    text: heapless::Vec<u8, MAX_TEXT_LEN>,
    max_len: u8,
    shift: bool,
    alt: bool,
    state: InputState,
    on_complete: fn(&[u8]),
    /// Optional title shown above the text area (e.g. "Name your Pet").
    title: &'static str,
}

fn char_table(id: u8) -> &'static [u8] {
    match id {
        0 => CHARS_ABC,
        1 => CHARS_DEF,
        2 => CHARS_GHI,
        3 => CHARS_JKL,
        4 => CHARS_MNO,
        5 => CHARS_PQR,
        6 => CHARS_STU,
        7 => CHARS_VWX,
        8 => CHARS_YZ,
        _ => CHARS_ABC,
    }
}

/// Alt-layer mapping for a base letter → `(alt, alt+shift)`, transcribed from
/// the keyboard silkscreen.  Top row = digits (shift → symbols); home row =
/// brackets/punctuation (no distinct shift — those symbols each have their own
/// key, so shift repeats the base); bottom row per the panel (`<` `>` are their
/// own keys, `c` is `_`/`-`).  `None` = key has no alt char (stays a letter).
fn alt_pair(ch: u8) -> Option<(u8, u8)> {
    Some(match ch {
        // Top row Q..P → digit / shifted symbol
        b'q' => (b'1', b'!'),
        b'w' => (b'2', b'@'),
        b'e' => (b'3', b'#'),
        b'r' => (b'4', b'$'),
        b't' => (b'5', b'%'),
        b'y' => (b'6', b'^'),
        b'u' => (b'7', b'&'),
        b'i' => (b'8', b'*'),
        b'o' => (b'9', b'('),
        b'p' => (b'0', b')'),
        // Home row A..L → brackets / punctuation (shift repeats the base)
        b'a' => (b'[', b'['),
        b's' => (b']', b']'),
        b'd' => (b'{', b'{'),
        b'f' => (b'}', b'}'),
        b'g' => (b'\\', b'\\'),
        b'h' => (b'|', b'|'),
        b'j' => (b';', b';'),
        b'k' => (b':', b':'),
        b'l' => (b'\'', b'"'),
        // Bottom row Z..M
        b'z' => (b'=', b'+'),
        b'x' => (b'`', b'~'),
        b'c' => (b'_', b'-'),
        b'v' => (b',', b','),
        b'b' => (b'.', b'.'),
        b'n' => (b'<', b'<'),
        b'm' => (b'>', b'>'),
        _ => return None,
    })
}

impl TextEntry {
    pub fn new(prefill: &[u8], max_len: u8, on_complete: fn(&[u8]), title: &'static str) -> Self {
        let mut text = heapless::Vec::new();
        let n = prefill.len().min(max_len as usize).min(MAX_TEXT_LEN);
        let _ = text.extend_from_slice(&prefill[..n]);
        Self {
            text,
            max_len,
            // First letter auto-capitalises via `text.is_empty()` in
            // push_char (phone-style), NOT via a default Shift — a default
            // Shift would also shift the first Alt-layer char (Alt+Z → "+"
            // instead of "=").  So Shift/Alt both start unarmed.
            shift: false,
            alt: false,
            state: InputState::Root,
            on_complete,
            title,
        }
    }

    fn push_char(&mut self, ch: u8) {
        if ch == BKSP {
            self.text.pop();
        } else if self.text.len() < self.max_len as usize {
            let c = if self.alt {
                // Alt-layer symbol, shifted variant when Shift is also armed.
                match alt_pair(ch) {
                    Some((base, shifted)) => {
                        if self.shift {
                            shifted
                        } else {
                            base
                        }
                    }
                    None => ch, // no alt char for this key → base letter
                }
            } else if self.shift || self.text.is_empty() {
                // Explicit Shift capitalises any letter; an empty buffer
                // auto-capitalises the first letter (phone-style).
                ch.to_ascii_uppercase()
            } else {
                ch
            };
            let _ = self.text.push(c);
            self.shift = false;
            self.alt = false;
        }
        self.state = InputState::Root;
    }

    fn clear(&mut self) {
        self.text.clear();
        self.state = InputState::Root;
    }

    /// Inject a key from an external keyboard.  Returns `true` when the entry
    /// completed (Enter → `on_complete` fired) and should be removed.
    pub fn inject(&mut self, key: ExtKey) -> bool {
        match key {
            ExtKey::Char(c) => self.push_char(c),
            ExtKey::Space => self.push_char(b' '),
            ExtKey::Backspace => self.push_char(BKSP),
            ExtKey::Shift => self.shift = !self.shift,
            ExtKey::Alt => self.alt = !self.alt,
            ExtKey::Enter => {
                // Alt turns Enter into the "/" key (Alt+Shift → "?"); only a
                // plain Enter submits the entry.
                if self.alt {
                    self.push_char(if self.shift { b'?' } else { b'/' });
                } else {
                    (self.on_complete)(&self.text);
                    return true;
                }
            }
        }
        false
    }

    /// Returns `true` when the session is complete (submitted or cancelled)
    /// and should be removed.
    pub fn dispatch(&mut self, btn: ButtonId) -> bool {
        match self.state {
            InputState::Root => match btn {
                ButtonId::Left => {
                    self.state = InputState::LetterQuad(Quadrant::Left);
                }
                ButtonId::Up => {
                    self.state = InputState::LetterQuad(Quadrant::Up);
                }
                ButtonId::Right => {
                    self.state = InputState::LetterQuad(Quadrant::Right);
                }
                ButtonId::Down => {
                    self.state = InputState::CmdHub;
                }
                ButtonId::Cancel => return true,
                ButtonId::Execute | ButtonId::Fire => {
                    (self.on_complete)(&self.text);
                    return true;
                }
            },

            InputState::LetterQuad(q) => {
                // Opposite direction = back to root.
                let back_dir = match q {
                    Quadrant::Left => ButtonId::Right,
                    Quadrant::Up => ButtonId::Down,
                    Quadrant::Right => ButtonId::Left,
                };
                if btn == back_dir || btn == ButtonId::Cancel {
                    self.state = InputState::Root;
                } else {
                    let table_id = match (q, btn) {
                        // A-I: ↓abc ←def ↑ghi
                        (Quadrant::Left, ButtonId::Down) => Some(0u8),
                        (Quadrant::Left, ButtonId::Left) => Some(1),
                        (Quadrant::Left, ButtonId::Up) => Some(2),
                        // J-R: ←jkl ↑mno →pqr
                        (Quadrant::Up, ButtonId::Left) => Some(3),
                        (Quadrant::Up, ButtonId::Up) => Some(4),
                        (Quadrant::Up, ButtonId::Right) => Some(5),
                        // S-Z: ↓stu ↑vwx →yz
                        (Quadrant::Right, ButtonId::Down) => Some(6),
                        (Quadrant::Right, ButtonId::Up) => Some(7),
                        (Quadrant::Right, ButtonId::Right) => Some(8),
                        _ => None,
                    };
                    if let Some(id) = table_id {
                        let mid = (char_table(id).len() / 2) as u8;
                        self.state = InputState::CharPick {
                            table_id: id,
                            cursor: mid,
                        };
                    }
                }
            }

            InputState::CharPick { table_id, cursor } => {
                let chars = char_table(table_id);
                match btn {
                    ButtonId::Left => {
                        if cursor > 0 {
                            self.state = InputState::CharPick {
                                table_id,
                                cursor: cursor - 1,
                            };
                        }
                    }
                    ButtonId::Right => {
                        if (cursor + 1) < chars.len() as u8 {
                            self.state = InputState::CharPick {
                                table_id,
                                cursor: cursor + 1,
                            };
                        }
                    }
                    ButtonId::Execute | ButtonId::Fire => {
                        self.push_char(chars[cursor as usize]);
                    }
                    ButtonId::Cancel => {
                        self.state = InputState::Root;
                    }
                    _ => {}
                }
            }

            InputState::CmdHub => match btn {
                ButtonId::Up | ButtonId::Cancel => {
                    self.state = InputState::Root;
                }
                ButtonId::Left => {
                    self.state = InputState::SpecialPick {
                        cursor: (SPECIAL_CHARS.len() / 2) as u8,
                    };
                }
                ButtonId::Down => {
                    self.state = InputState::SpaceBkspPick { cursor: 0 };
                }
                ButtonId::Right => {
                    self.state = InputState::CmdRightHub;
                }
                _ => {}
            },

            InputState::CmdRightHub => match btn {
                ButtonId::Left | ButtonId::Cancel => {
                    self.state = InputState::CmdHub;
                }
                ButtonId::Up => {
                    self.shift = !self.shift;
                    self.state = InputState::Root;
                }
                ButtonId::Down => {
                    self.clear();
                }
                ButtonId::Right => {
                    self.state = InputState::NumberPick {
                        cursor: (NUMBER_CHARS.len() / 2) as u8,
                    };
                }
                _ => {}
            },

            InputState::SpaceBkspPick { cursor } => match btn {
                ButtonId::Left => {
                    if cursor > 0 {
                        self.state = InputState::SpaceBkspPick { cursor: cursor - 1 };
                    }
                }
                ButtonId::Right => {
                    if (cursor + 1) < SPACE_BKSP.len() as u8 {
                        self.state = InputState::SpaceBkspPick { cursor: cursor + 1 };
                    }
                }
                ButtonId::Execute | ButtonId::Fire => {
                    self.push_char(SPACE_BKSP[cursor as usize]);
                }
                ButtonId::Cancel => {
                    self.state = InputState::CmdHub;
                }
                _ => {}
            },

            InputState::SpecialPick { cursor } => match btn {
                ButtonId::Left => {
                    if cursor > 0 {
                        self.state = InputState::SpecialPick { cursor: cursor - 1 };
                    }
                }
                ButtonId::Right => {
                    if (cursor + 1) < SPECIAL_CHARS.len() as u8 {
                        self.state = InputState::SpecialPick { cursor: cursor + 1 };
                    }
                }
                ButtonId::Execute | ButtonId::Fire => {
                    self.push_char(SPECIAL_CHARS[cursor as usize]);
                }
                ButtonId::Cancel => {
                    self.state = InputState::CmdHub;
                }
                _ => {}
            },

            InputState::NumberPick { cursor } => match btn {
                ButtonId::Left => {
                    if cursor > 0 {
                        self.state = InputState::NumberPick { cursor: cursor - 1 };
                    }
                }
                ButtonId::Right => {
                    if (cursor + 1) < NUMBER_CHARS.len() as u8 {
                        self.state = InputState::NumberPick { cursor: cursor + 1 };
                    }
                }
                ButtonId::Execute | ButtonId::Fire => {
                    self.push_char(NUMBER_CHARS[cursor as usize]);
                }
                ButtonId::Cancel => {
                    self.state = InputState::CmdRightHub;
                }
                _ => {}
            },
        }
        false
    }
}

// ── Rendering ────────────────────────────────────────────────────────────────

const FONT: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13_BOLD, BLACK);
const FONT_INV: MonoTextStyle<'static, TriColor> = MonoTextStyle::new(&FONT_7X13_BOLD, WHITE);

const CHAR_W: i32 = 7;
const LINE_H: i32 = 14;
const DISPLAY_W: i32 = 152;
const DISPLAY_H: i32 = 152;
const CHARS_PER_LINE: usize = 20;

const TEXT_AREA_Y: i32 = 2;
const TEXT_AREA_H: i32 = 68;
const KB_Y: i32 = 76;
const KB_CX: i32 = 76;
const KB_CY: i32 = 116;

pub fn draw_text_entry<D>(display: &mut D, entry: &TextEntry) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Clear the full screen so sprite graphics don't bleed through.
    Rectangle::new(Point::zero(), Size::new(DISPLAY_W as u32, DISPLAY_H as u32))
        .into_styled(PrimitiveStyle::with_fill(WHITE))
        .draw(display)?;

    // Title bar (if set).
    if !entry.title.is_empty() {
        Rectangle::new(Point::zero(), Size::new(DISPLAY_W as u32, 18))
            .into_styled(PrimitiveStyle::with_fill(BLACK))
            .draw(display)?;
        let ts = TextStyleBuilder::new()
            .baseline(Baseline::Middle)
            .alignment(Alignment::Center)
            .build();
        Text::with_text_style(
            entry.title,
            Point::new(DISPLAY_W / 2, 9),
            MonoTextStyle::new(&FONT_7X13_BOLD, WHITE),
            ts,
        )
        .draw(display)?;
    }

    let text_offset = if entry.title.is_empty() { 0 } else { 18 };
    draw_text_area(display, entry, text_offset)?;

    // Divider
    Rectangle::new(Point::new(0, KB_Y - 2), Size::new(DISPLAY_W as u32, 1))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;

    // Shift / Alt indicators (top-right of the keyboard area) — the armed
    // one-shot toggles (same state for joystick and I2C keyboard).
    if entry.shift {
        draw_mod_badge(display, "S", DISPLAY_W - 14)?;
    }
    if entry.alt {
        draw_mod_badge(display, "A", DISPLAY_W - 30)?;
    }

    // With an external keyboard driving entry, the joystick char-picker is
    // just clutter — show a plain hint so it's clear the keyboard is live.
    if keyboard_active() {
        draw_keyboard_hint(display)?;
    } else {
        match entry.state {
            InputState::Root => draw_hub_root(display)?,
            InputState::LetterQuad(q) => draw_hub_letter_quad(display, q)?,
            InputState::CharPick { table_id, cursor } => {
                draw_char_picker(display, char_table(table_id), cursor)?;
            }
            InputState::CmdHub => draw_hub_cmd(display)?,
            InputState::CmdRightHub => draw_hub_cmd_right(display)?,
            InputState::SpaceBkspPick { cursor } => {
                draw_char_picker(display, SPACE_BKSP, cursor)?;
            }
            InputState::SpecialPick { cursor } => {
                draw_char_picker(display, SPECIAL_CHARS, cursor)?;
            }
            InputState::NumberPick { cursor } => {
                draw_char_picker(display, NUMBER_CHARS, cursor)?;
            }
        }
    }

    Ok(())
}

/// A small filled badge with an inverted single-char label (e.g. "S", "A")
/// at `x` on the divider row — used for the Shift / Alt indicators.
fn draw_mod_badge<D>(display: &mut D, label: &str, x: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Rectangle::new(Point::new(x, KB_Y), Size::new(14, 14))
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)?;
    let ts = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(label, Point::new(x + 7, KB_Y + 1), FONT_INV, ts).draw(display)?;
    Ok(())
}

/// Centred hint shown in the keyboard area when an external I2C keyboard is
/// driving text entry (instead of the joystick char-picker).
fn draw_keyboard_hint<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let ts = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();
    let cx = DISPLAY_W / 2;
    Text::with_text_style("- Keyboard -", Point::new(cx, KB_Y + 16), FONT, ts).draw(display)?;
    Text::with_text_style("type to enter", Point::new(cx, KB_Y + 34), FONT, ts).draw(display)?;
    Text::with_text_style("Enter = save", Point::new(cx, KB_Y + 52), FONT, ts).draw(display)?;
    Text::with_text_style("Cancel = quit", Point::new(cx, KB_Y + 70), FONT, ts).draw(display)?;
    Ok(())
}

fn draw_text_area<D>(display: &mut D, entry: &TextEntry, y_offset: i32) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    // Border — clamp height so it doesn't overlap the keyboard area.
    let border_h = (TEXT_AREA_H + 4).min(KB_Y - 2 - y_offset) as u32;
    Rectangle::new(
        Point::new(0, y_offset),
        Size::new(DISPLAY_W as u32, border_h),
    )
    .into_styled(PrimitiveStyle::with_stroke(BLACK, 1))
    .draw(display)?;

    let ts = TextStyleBuilder::new()
        .baseline(Baseline::Top)
        .alignment(Alignment::Left)
        .build();

    let text = core::str::from_utf8(&entry.text).unwrap_or("");

    // Wrap and draw lines
    let max_lines = (TEXT_AREA_H / LINE_H) as usize;
    let total_lines = (text.len() + CHARS_PER_LINE - 1).max(1) / CHARS_PER_LINE.max(1);
    let start_line = total_lines.saturating_sub(max_lines);

    for i in 0..max_lines {
        let line_idx = start_line + i;
        let byte_start = line_idx * CHARS_PER_LINE;
        if byte_start >= text.len() {
            // Draw cursor on the first empty line after text
            if byte_start == text.len() {
                Text::with_text_style(
                    "_",
                    Point::new(4, y_offset + TEXT_AREA_Y + 2 + i as i32 * LINE_H),
                    FONT,
                    ts,
                )
                .draw(display)?;
            }
            break;
        }
        let mut byte_end = (byte_start + CHARS_PER_LINE).min(text.len());
        while byte_end > byte_start && !text.is_char_boundary(byte_end) {
            byte_end -= 1;
        }
        let line_str = &text[byte_start..byte_end];

        Text::with_text_style(
            line_str,
            Point::new(4, y_offset + TEXT_AREA_Y + 2 + i as i32 * LINE_H),
            FONT,
            ts,
        )
        .draw(display)?;

        // Cursor after the last char of the last visible line
        if byte_end == text.len() && line_str.len() < CHARS_PER_LINE {
            Text::with_text_style(
                "_",
                Point::new(
                    4 + line_str.len() as i32 * CHAR_W,
                    y_offset + TEXT_AREA_Y + 2 + i as i32 * LINE_H,
                ),
                FONT,
                ts,
            )
            .draw(display)?;
        }
    }

    Ok(())
}

// ── Hub renderers ────────────────────────────────────────────────────────────

fn draw_center_dot<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Circle::with_center(Point::new(KB_CX, KB_CY), 10)
        .into_styled(PrimitiveStyle::with_fill(BLACK))
        .draw(display)
}

fn draw_arrow_up<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Triangle::new(
        Point::new(KB_CX, KB_CY - 20),
        Point::new(KB_CX - 6, KB_CY - 12),
        Point::new(KB_CX + 6, KB_CY - 12),
    )
    .into_styled(PrimitiveStyle::with_fill(BLACK))
    .draw(display)
}

fn draw_arrow_down<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Triangle::new(
        Point::new(KB_CX, KB_CY + 20),
        Point::new(KB_CX - 6, KB_CY + 12),
        Point::new(KB_CX + 6, KB_CY + 12),
    )
    .into_styled(PrimitiveStyle::with_fill(BLACK))
    .draw(display)
}

fn draw_arrow_left<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Triangle::new(
        Point::new(KB_CX - 20, KB_CY),
        Point::new(KB_CX - 12, KB_CY - 6),
        Point::new(KB_CX - 12, KB_CY + 6),
    )
    .into_styled(PrimitiveStyle::with_fill(BLACK))
    .draw(display)
}

fn draw_arrow_right<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    Triangle::new(
        Point::new(KB_CX + 20, KB_CY),
        Point::new(KB_CX + 12, KB_CY - 6),
        Point::new(KB_CX + 12, KB_CY + 6),
    )
    .into_styled(PrimitiveStyle::with_fill(BLACK))
    .draw(display)
}

fn draw_label<D>(
    display: &mut D,
    text: &str,
    x: i32,
    y: i32,
    align: Alignment,
) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let ts = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(align)
        .build();
    Text::with_text_style(text, Point::new(x, y), FONT, ts).draw(display)?;
    Ok(())
}

fn draw_hub_root<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_center_dot(display)?;
    draw_arrow_left(display)?;
    draw_arrow_right(display)?;
    draw_arrow_up(display)?;
    draw_arrow_down(display)?;
    draw_label(display, "A-I", KB_CX - 34, KB_CY, Alignment::Right)?;
    draw_label(display, "J-R", KB_CX, KB_CY - 28, Alignment::Center)?;
    draw_label(display, "S-Z", KB_CX + 34, KB_CY, Alignment::Left)?;
    draw_label(display, "CMD", KB_CX, KB_CY + 28, Alignment::Center)?;
    Ok(())
}

fn draw_hub_letter_quad<D>(display: &mut D, q: Quadrant) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_center_dot(display)?;

    let (labels, back_label, dirs) = match q {
        // A-I: right=back, ↓abc ←def ↑ghi
        Quadrant::Left => (
            ["def", "abc", "ghi"],
            ("->", Alignment::Left, KB_CX + 34, KB_CY),
            [
                (ButtonId::Left, KB_CX - 34, KB_CY, Alignment::Right),
                (ButtonId::Down, KB_CX, KB_CY + 28, Alignment::Center),
                (ButtonId::Up, KB_CX, KB_CY - 28, Alignment::Center),
            ],
        ),
        // J-R: down=back, ←jkl ↑mno →pqr
        Quadrant::Up => (
            ["jkl", "mno", "pqr"],
            ("v", Alignment::Center, KB_CX, KB_CY + 28),
            [
                (ButtonId::Left, KB_CX - 34, KB_CY, Alignment::Right),
                (ButtonId::Up, KB_CX, KB_CY - 28, Alignment::Center),
                (ButtonId::Right, KB_CX + 34, KB_CY, Alignment::Left),
            ],
        ),
        // S-Z: left=back, ↓stu ↑vwx →yz
        Quadrant::Right => (
            ["stu", "vwx", "yz"],
            ("<-", Alignment::Right, KB_CX - 34, KB_CY),
            [
                (ButtonId::Down, KB_CX, KB_CY + 28, Alignment::Center),
                (ButtonId::Up, KB_CX, KB_CY - 28, Alignment::Center),
                (ButtonId::Right, KB_CX + 34, KB_CY, Alignment::Left),
            ],
        ),
    };

    // Draw back arrow + label
    match q {
        Quadrant::Left => draw_arrow_right(display)?,
        Quadrant::Up => draw_arrow_down(display)?,
        Quadrant::Right => draw_arrow_left(display)?,
    }
    draw_label(
        display,
        back_label.0,
        back_label.2,
        back_label.3,
        back_label.1,
    )?;

    // Draw sub-group arrows + labels
    for (i, &(dir, x, y, align)) in dirs.iter().enumerate() {
        match dir {
            ButtonId::Left => draw_arrow_left(display)?,
            ButtonId::Right => draw_arrow_right(display)?,
            ButtonId::Up => draw_arrow_up(display)?,
            ButtonId::Down => draw_arrow_down(display)?,
            _ => {}
        }
        draw_label(display, labels[i], x, y, align)?;
    }

    Ok(())
}

fn draw_hub_cmd<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_center_dot(display)?;
    draw_arrow_up(display)?;
    draw_arrow_left(display)?;
    draw_arrow_down(display)?;
    draw_arrow_right(display)?;
    draw_label(display, "<-", KB_CX, KB_CY - 28, Alignment::Center)?;
    draw_label(display, "!@#", KB_CX - 34, KB_CY, Alignment::Right)?;
    draw_label(display, "SP/BS", KB_CX, KB_CY + 28, Alignment::Center)?;
    draw_label(display, "More", KB_CX + 34, KB_CY, Alignment::Left)?;
    Ok(())
}

fn draw_hub_cmd_right<D>(display: &mut D) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    draw_center_dot(display)?;
    draw_arrow_up(display)?;
    draw_arrow_left(display)?;
    draw_arrow_down(display)?;
    draw_arrow_right(display)?;
    draw_label(display, "<-", KB_CX - 34, KB_CY, Alignment::Right)?;
    draw_label(display, "Shift", KB_CX, KB_CY - 28, Alignment::Center)?;
    draw_label(display, "Clear", KB_CX, KB_CY + 28, Alignment::Center)?;
    draw_label(display, "0-9", KB_CX + 34, KB_CY, Alignment::Left)?;
    Ok(())
}

/// Draw a horizontal character picker with the cursor-th character highlighted.
fn draw_char_picker<D>(display: &mut D, chars: &[u8], cursor: u8) -> Result<(), D::Error>
where
    D: DrawTarget<Color = TriColor>,
{
    let ts = TextStyleBuilder::new()
        .baseline(Baseline::Middle)
        .alignment(Alignment::Center)
        .build();

    let total_w = chars.len() as i32 * 16;
    let start_x = KB_CX - total_w / 2 + 8;

    for (i, &ch) in chars.iter().enumerate() {
        let x = start_x + i as i32 * 16;
        let is_sel = i == cursor as usize;

        if is_sel {
            Rectangle::new(Point::new(x - 7, KB_CY - 9), Size::new(14, 18))
                .into_styled(PrimitiveStyle::with_fill(BLACK))
                .draw(display)?;
        }

        let label = if ch == b' ' {
            "SP"
        } else if ch == BKSP {
            "BS"
        } else {
            unsafe { core::str::from_utf8_unchecked(core::slice::from_ref(&ch)) }
        };
        let font = if is_sel { FONT_INV } else { FONT };
        Text::with_text_style(label, Point::new(x, KB_CY), font, ts).draw(display)?;
    }

    // Hint at the bottom — `FONT_7X13_BOLD` is 13 px tall, display
    // is 152 px tall, so a `Baseline::Top` text starting at y=144
    // would draw down to y=157 and clip.  Use `Baseline::Bottom` at
    // y=151 instead so the glyphs sit flush against the bottom edge
    // with one pixel of breathing room.
    let hint_ts = TextStyleBuilder::new()
        .baseline(Baseline::Bottom)
        .alignment(Alignment::Center)
        .build();
    Text::with_text_style(
        "</>: move  Fire: select",
        Point::new(KB_CX, 151),
        FONT,
        hint_ts,
    )
    .draw(display)?;

    Ok(())
}

// ── Global session ───────────────────────────────────────────────────────────

#[cfg(feature = "embassy-base")]
use embassy_sync::blocking_mutex::{Mutex, raw::CriticalSectionRawMutex};

#[cfg(feature = "embassy-base")]
pub static TEXT_ENTRY: Mutex<CriticalSectionRawMutex, RefCell<Option<TextEntry>>> =
    Mutex::new(RefCell::new(None));

#[cfg(feature = "simulator")]
pub static TEXT_ENTRY: std::sync::Mutex<RefCell<Option<TextEntry>>> =
    std::sync::Mutex::new(RefCell::new(None));

/// Start a text entry session. `prefill` is the initial text, `max_len` the
/// maximum number of characters, `on_complete` is called with the final
/// text bytes when the user submits, and `title` is shown above the text area.
pub fn begin(prefill: &[u8], max_len: u8, on_complete: fn(&[u8]), title: &'static str) {
    let entry = TextEntry::new(prefill, max_len, on_complete, title);
    #[cfg(feature = "embassy-base")]
    TEXT_ENTRY.lock(|cell| cell.replace(Some(entry)));
    #[cfg(feature = "simulator")]
    TEXT_ENTRY.lock().unwrap().replace(Some(entry));
}

/// Returns true when text entry is active.
pub fn is_active() -> bool {
    #[cfg(feature = "embassy-base")]
    return TEXT_ENTRY.lock(|cell| cell.borrow().is_some());
    #[cfg(feature = "simulator")]
    return TEXT_ENTRY.lock().unwrap().borrow().is_some();
}

/// True while an external I2C keyboard is driving the active entry — set by
/// the display loop.  Switches the on-screen view from the joystick picker to
/// a plain "type here" hint.
static KBD_ACTIVE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Set by the display loop when an I2C keyboard is present for this entry.
pub fn set_keyboard_active(on: bool) {
    KBD_ACTIVE.store(on, core::sync::atomic::Ordering::Relaxed);
}

fn keyboard_active() -> bool {
    KBD_ACTIVE.load(core::sync::atomic::Ordering::Relaxed)
}

/// Inject an external-keyboard key into the active entry (no-op if none).
/// On Enter the entry completes and is removed (matching the button path).
#[cfg(feature = "embassy-base")]
pub fn inject(key: ExtKey) {
    TEXT_ENTRY.lock(|cell| {
        let done = cell.borrow_mut().as_mut().map(|e| e.inject(key)).unwrap_or(false);
        if done {
            cell.replace(None);
        }
    });
}

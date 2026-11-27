//! Companion text CLI — `CMD_RUN_CLI_COMMAND` (0x42) → `RESP_CODE_CLI_REPLY`
//! (0x1D), MeshCore 1.18 / `FIRMWARE_VER_CODE` 14.
//!
//! Mirrors the subset of the reference `MyMesh::handleCommand()` and
//! `CommonRadioPrefs::handleCommand()` that maps onto badge settings:
//!
//! | Command                          | Reply                              |
//! |----------------------------------|------------------------------------|
//! | `get name` / `set name <n>`      | `> <n>` / `OK`, `Error, bad chars` |
//! | `get radio`                      | `> <MHz>,<kHz>,<sf>,<cr>`          |
//! | `set radio <MHz>,<kHz>,<sf>,<cr>`| `OK`, `Error, invalid radio params`|
//! | `get freq`                       | `> <MHz>`                          |
//! | `get tx` / `set tx <dBm>`        | `> <dBm>` / `OK`                   |
//! | `get tz.offset` / `set tz.offset`| `> <h.h>` / `OK` (quarter hours)   |
//! | `ver`, `board`                   | version / model string             |
//! | `reboot`                         | reboots after the reply            |
//! | `poweroff`, `shutdown`           | `Error: unsupported` (no System OFF)|
//!
//! Anything else gets the reference's `Unknown command`. An optional
//! `XX|` prefix is echoed back in front of the reply, as upstream does.
//!
//! Pure logic, no embassy: the caller passes a [`Settings`] snapshot and
//! applies the returned [`Action`] once the reply has been sent. The
//! simulator build compiles this file too so the tests run on the host.

use core::fmt::Write;

/// `FIRMWARE_VER_CODE` reported in `RESP_CODE_DEVICE_INFO`.
pub const FIRMWARE_VER_CODE: u8 = 14;
/// `FIRMWARE_VERSION` — MeshCore release whose companion protocol we speak.
pub const FIRMWARE_VERSION: &str = "v1.18.0";
/// `FIRMWARE_BUILD_DATE` slot.
pub const FIRMWARE_BUILD: &str = "dev";
/// `board.getManufacturerName()`.
pub const BOARD_NAME: &str = "BornHack Cyber\u{c6}gg";

/// Node-name limit — same as `settings::MAX_NODE_NAME` (`node_name[32]`).
pub const MAX_NAME: usize = crate::MAX_NODE_NAME;
/// Reply buffer size; a reply never comes close.
pub const MAX_REPLY: usize = 160;
pub type Reply = heapless::String<MAX_REPLY>;

/// Settings the CLI can read.
#[derive(Clone, Copy)]
pub struct Settings<'a> {
    pub name: &'a [u8],
    pub freq_hz: u32,
    pub bw_hz: u32,
    pub sf: u8,
    pub cr: u8,
    pub tx_power: i8,
    /// UTC offset in quarter hours (`TZ_OFFSET_QH`).
    pub tz_offset_qh: i8,
}

/// Side effect for the caller to apply after the reply is sent.
#[cfg_attr(test, derive(Debug, PartialEq))]
pub enum Action<'a> {
    None,
    /// Valid UTF-8, at most [`MAX_NAME`] bytes.
    SetName(&'a [u8]),
    SetRadio {
        freq_hz: u32,
        bw_hz: u32,
        sf: u8,
        cr: u8,
    },
    SetTxPower(i8),
    /// New UTC offset in quarter hours.
    SetTzOffset(i8),
    Reboot,
}

/// Run one command line, writing the reply text into `reply`.
pub fn run<'a>(line: &'a [u8], cur: &Settings, reply: &mut Reply) -> Action<'a> {
    reply.clear();
    // The app sends no terminator; stop at a NUL like the reference's C string.
    let line = &line[..line.iter().position(|&b| b == 0).unwrap_or(line.len())];
    let mut cmd = line;
    while let [b' ', rest @ ..] = cmd {
        cmd = rest;
    }
    // Optional `XX|` prefix (companion CLI): reflected back on the reply.
    if cmd.len() > 4 && cmd[2] == b'|' {
        for &b in &cmd[..3] {
            let _ = reply.push(b as char);
        }
        cmd = &cmd[3..];
    }
    match handle(cmd, cur, reply) {
        Some(action) => action,
        None => {
            let _ = reply.push_str("Unknown command");
            Action::None
        }
    }
}

fn handle<'a>(cmd: &'a [u8], cur: &Settings, reply: &mut Reply) -> Option<Action<'a>> {
    let action = if cmd == b"get radio" {
        let _ = reply.push_str("> ");
        push_fixed(reply, cur.freq_hz, 1_000_000, 1);
        let _ = reply.push(',');
        push_fixed(reply, cur.bw_hz, 1_000, 0);
        let _ = write!(reply, ",{},{}", cur.sf, cur.cr);
        Action::None
    } else if let Some(arg) = cmd.strip_prefix(b"set radio ") {
        let mut parts = arg.splitn(4, |&b| b == b',');
        let freq_hz = parts.next().and_then(|p| parse_fixed(p, 6)).unwrap_or(0);
        let bw_hz = parts.next().and_then(|p| parse_fixed(p, 3)).unwrap_or(0);
        let sf = parts.next().and_then(parse_int).unwrap_or(0);
        // Reference: when 4 parts are taken, the last stops at the next ','.
        let cr = parts
            .next()
            .and_then(|p| parse_int(p.split(|&b| b == b',').next().unwrap_or(p)))
            .unwrap_or(0);
        if (150_000_000..=2_500_000_000).contains(&freq_hz)
            && (7_000..=500_000).contains(&bw_hz)
            && (5..=12).contains(&sf)
            && (5..=8).contains(&cr)
        {
            // The badge reprograms the SX1262 live, so no "reboot to apply".
            let _ = reply.push_str("OK");
            Action::SetRadio {
                freq_hz: freq_hz as u32,
                bw_hz: bw_hz as u32,
                sf: sf as u8,
                cr: cr as u8,
            }
        } else {
            let _ = reply.push_str("Error, invalid radio params");
            Action::None
        }
    } else if cmd == b"get freq" {
        let _ = reply.push_str("> ");
        push_fixed(reply, cur.freq_hz, 1_000_000, 1);
        Action::None
    } else if cmd.starts_with(b"get tx") && matches!(cmd.get(6), None | Some(b' ')) {
        let _ = write!(reply, "> {}", cur.tx_power);
        Action::None
    } else if let Some(arg) = cmd.strip_prefix(b"set tx ") {
        match parse_int(arg) {
            Some(p @ -9..=22) => {
                let _ = reply.push_str("OK");
                Action::SetTxPower(p as i8)
            }
            _ => {
                let _ = reply.push_str("Error, must be -9 to 22");
                Action::None
            }
        }
    } else if cmd == b"get name" {
        let _ = reply.push_str("> ");
        push_utf8(reply, cur.name);
        Action::None
    } else if let Some(arg) = cmd.strip_prefix(b"set name ") {
        match core::str::from_utf8(arg) {
            Ok(s) if !s.is_empty() && !s.contains(['[', ']', '\\', ':', ',', '?', '*']) => {
                let _ = reply.push_str("OK");
                let mut end = s.len().min(MAX_NAME);
                while !s.is_char_boundary(end) {
                    end -= 1;
                }
                Action::SetName(&arg[..end])
            }
            _ => {
                let _ = reply.push_str("Error, bad chars");
                Action::None
            }
        }
    } else if cmd == b"get tz.offset" {
        // Reference (MeshCore 1.18 dev): `float tz_offset`, printed "%.2f"
        // for an odd quarter hour and "%.1f" otherwise.
        let qh = cur.tz_offset_qh;
        let sign = if qh < 0 { "-" } else { "" };
        let frac = [".0", ".25", ".5", ".75"][(qh.unsigned_abs() % 4) as usize];
        let _ = write!(reply, "> {}{}{}", sign, qh.unsigned_abs() / 4, frac);
        Action::None
    } else if let Some(arg) = cmd.strip_prefix(b"set tz.offset ") {
        // Reference: `float tz = atof(...)` in -12..=14. The badge keeps
        // quarter hours, so the value rounds to the nearest one.
        match parse_hundredths(arg) {
            Some(h100 @ -1200..=1400) => {
                let _ = reply.push_str("OK");
                Action::SetTzOffset(((h100 + h100.signum() * 12) / 25) as i8)
            }
            _ => {
                let _ = reply.push_str("Error, must be from -12 to +14");
                Action::None
            }
        }
    } else if cmd == b"ver" {
        let _ = write!(reply, "{} (Build: {})", FIRMWARE_VERSION, FIRMWARE_BUILD);
        Action::None
    } else if cmd == b"board" {
        let _ = reply.push_str(BOARD_NAME);
        Action::None
    } else if cmd == b"reboot" {
        // The reference reboots without replying; we reply first.
        let _ = reply.push_str("OK - rebooting");
        Action::Reboot
    } else if cmd == b"poweroff" || cmd == b"shutdown" {
        // The badge has no System OFF wake source wired up.
        let _ = reply.push_str("Error: unsupported");
        Action::None
    } else {
        return None;
    };
    Some(action)
}

/// Append `value / scale` as a decimal, trailing fraction zeros trimmed but at
/// least `min_frac` fraction digits kept (`min_frac = 0` also drops the '.').
/// Matches the reference's `ftoa` (freq, `min_frac = 1`) and `ftoa3` (bw).
fn push_fixed(reply: &mut Reply, value: u32, scale: u32, min_frac: usize) {
    let _ = write!(reply, "{}", value / scale);
    let mut frac: heapless::String<8> = heapless::String::new();
    let digits = scale.ilog10() as usize;
    let _ = write!(frac, "{:0width$}", value % scale, width = digits);
    let keep = frac.trim_end_matches('0').len().max(min_frac);
    if keep > 0 {
        let _ = reply.push('.');
        let _ = reply.push_str(&frac[..keep]);
    }
}

/// Parse `"869.618"` as a fixed-point integer with `frac_digits` decimals
/// (`869_618_000` for 6). Leading/trailing spaces are ignored.
fn parse_fixed(s: &[u8], frac_digits: u32) -> Option<u64> {
    let s = trim(s);
    let mut parts = s.splitn(2, |&b| b == b'.');
    let whole = parts.next()?;
    let frac = parts.next().unwrap_or(b"");
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &b in whole {
        v = v.checked_mul(10)?.checked_add(digit(b)?)?;
    }
    for i in 0..frac_digits as usize {
        let d = match frac.get(i) {
            Some(&b) => digit(b)?,
            None => 0,
        };
        v = v.checked_mul(10)?.checked_add(d)?;
    }
    // Extra precision beyond `frac_digits` must still be digits.
    frac.iter()
        .skip(frac_digits as usize)
        .try_for_each(|&b| digit(b).map(|_| ()))?;
    Some(v)
}

/// Parse a signed decimal such as `"-5.75"` as hundredths (`-575`).
fn parse_hundredths(s: &[u8]) -> Option<i32> {
    let s = trim(s);
    let (neg, digits) = match s {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, s),
    };
    let v = i32::try_from(parse_fixed(digits, 2)?).ok()?;
    Some(if neg { -v } else { v })
}

/// Parse a signed decimal integer, ignoring surrounding spaces.
fn parse_int(s: &[u8]) -> Option<i32> {
    core::str::from_utf8(trim(s)).ok()?.parse().ok()
}

fn digit(b: u8) -> Option<u64> {
    b.is_ascii_digit().then(|| (b - b'0') as u64)
}

fn trim(mut s: &[u8]) -> &[u8] {
    while let [b' ', rest @ ..] = s {
        s = rest;
    }
    while let [rest @ .., b' '] = s {
        s = rest;
    }
    s
}

fn push_utf8(reply: &mut Reply, bytes: &[u8]) {
    for chunk in bytes.utf8_chunks() {
        let _ = reply.push_str(chunk.valid());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUR: Settings<'static> = Settings {
        name: b"Aegg",
        freq_hz: 869_618_000,
        bw_hz: 62_500,
        sf: 8,
        cr: 8,
        tx_power: 22,
        tz_offset_qh: 8,
    };

    fn exec(line: &str) -> (Reply, Action<'_>) {
        let mut r = Reply::new();
        let a = run(line.as_bytes(), &CUR, &mut r);
        (r, a)
    }

    #[test]
    fn get_name_and_radio() {
        assert_eq!(exec("get name").0.as_str(), "> Aegg");
        assert_eq!(exec("get radio").0.as_str(), "> 869.618,62.5,8,8");
        assert_eq!(exec("get freq").0.as_str(), "> 869.618");
        assert_eq!(exec("get tx").0.as_str(), "> 22");
        assert_eq!(exec("get tz.offset").0.as_str(), "> 2.0");
    }

    #[test]
    fn radio_formats_like_ftoa() {
        let cur = Settings {
            freq_hz: 915_000_000,
            bw_hz: 125_000,
            ..CUR
        };
        let mut r = Reply::new();
        run(b"get radio", &cur, &mut r);
        assert_eq!(r.as_str(), "> 915.0,125,8,8");
    }

    #[test]
    fn set_radio_valid_and_invalid() {
        let (r, a) = exec("set radio 869.525,250,11,5");
        assert_eq!(r.as_str(), "OK");
        assert_eq!(
            a,
            Action::SetRadio {
                freq_hz: 869_525_000,
                bw_hz: 250_000,
                sf: 11,
                cr: 5
            }
        );
        let (r, a) = exec("set radio 869.525,62.5,13,5");
        assert_eq!(r.as_str(), "Error, invalid radio params");
        assert_eq!(a, Action::None);
        assert_eq!(
            exec("set radio junk").0.as_str(),
            "Error, invalid radio params"
        );
    }

    #[test]
    fn set_tx_and_tz() {
        assert_eq!(exec("set tx 14").1, Action::SetTxPower(14));
        assert_eq!(exec("set tx 30").0.as_str(), "Error, must be -9 to 22");
        assert_eq!(exec("set tz.offset -5").1, Action::SetTzOffset(-20));
        assert_eq!(exec("set tz.offset 5.5").1, Action::SetTzOffset(22));
        assert_eq!(exec("set tz.offset 5.75").1, Action::SetTzOffset(23));
        assert_eq!(exec("set tz.offset -3.5").1, Action::SetTzOffset(-14));
        assert_eq!(exec("set tz.offset 14").1, Action::SetTzOffset(56));
        assert_eq!(exec("set tz.offset -12").1, Action::SetTzOffset(-48));
        assert_eq!(exec("set tz.offset 1.1").1, Action::SetTzOffset(4));
        assert_eq!(
            exec("set tz.offset -12.5").0.as_str(),
            "Error, must be from -12 to +14"
        );
        assert_eq!(
            exec("set tz.offset 15").0.as_str(),
            "Error, must be from -12 to +14"
        );
    }

    #[test]
    fn get_tz_offset_prints_quarter_hours_like_upstream() {
        let get = |qh| {
            let cur = Settings { tz_offset_qh: qh, ..CUR };
            let mut r = Reply::new();
            run(b"get tz.offset", &cur, &mut r);
            r
        };
        assert_eq!(get(22).as_str(), "> 5.5");
        assert_eq!(get(23).as_str(), "> 5.75");
        assert_eq!(get(-2).as_str(), "> -0.5");
        assert_eq!(get(-48).as_str(), "> -12.0");
        assert_eq!(get(0).as_str(), "> 0.0");
    }

    #[test]
    fn set_name_validates_and_truncates() {
        assert_eq!(exec("set name Bob").1, Action::SetName(b"Bob"));
        assert_eq!(exec("set name a,b").0.as_str(), "Error, bad chars");
        let long = "set name ÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆÆ"; // 16 × 2 bytes
        match exec(long).1 {
            Action::SetName(n) => assert_eq!(n.len(), 30),
            other => panic!("unexpected {:?}", other),
        }
    }

    #[test]
    fn ver_board_power() {
        assert_eq!(exec("ver").0.as_str(), "v1.18.0 (Build: dev)");
        assert_eq!(exec("board").0.as_str(), "BornHack Cyber\u{c6}gg");
        assert_eq!(exec("reboot").1, Action::Reboot);
        assert_eq!(exec("poweroff").0.as_str(), "Error: unsupported");
    }

    #[test]
    fn unknown_and_prefix() {
        assert_eq!(exec("frobnicate").0.as_str(), "Unknown command");
        assert_eq!(exec("  get name").0.as_str(), "> Aegg");
        assert_eq!(exec("01|get name").0.as_str(), "01|> Aegg");
        assert_eq!(exec("01|nope").0.as_str(), "01|Unknown command");
        assert_eq!(exec("get name\0junk").0.as_str(), "> Aegg");
    }
}

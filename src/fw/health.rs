use core::cell::RefCell;

use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use heapless::String;

#[derive(Copy, Clone)]
pub struct PeripheralHealth {
    pub healthy: bool,
    pub error: Option<&'static str>,
    pub msg: Option<&'static str>,
}

impl PeripheralHealth {
    const fn default() -> Self {
        Self {
            healthy: false,
            error: None,
            msg: None,
        }
    }

    pub fn set_err(&mut self, err: &'static str) {
        self.healthy = false;
        self.error = Some(err);
    }

    pub fn set_ok(&mut self, msg: &'static str) {
        self.healthy = true;
        self.error = None;
        self.msg = Some(msg);
    }

    pub fn char(&self, upper: char, lower: char) -> char {
        if self.healthy { upper } else { lower }
    }
}

pub struct ButtonHealth {
    pub seen_high: bool,
    pub seen_low: bool,
}

impl ButtonHealth {
    const fn default() -> Self {
        Self {
            seen_high: false,
            seen_low: false,
        }
    }

    pub fn char(&self, upper: char, lower: char) -> char {
        if self.seen_high && self.seen_low {
            upper
        } else {
            lower
        }
    }
}

pub struct ButtonsHealth {
    pub up: ButtonHealth,
    pub down: ButtonHealth,
    pub left: ButtonHealth,
    pub right: ButtonHealth,
    pub fire: ButtonHealth,
    pub cancel: ButtonHealth,
    pub execute: ButtonHealth,
}

impl ButtonsHealth {
    const fn default() -> Self {
        Self {
            up: ButtonHealth::default(),
            down: ButtonHealth::default(),
            left: ButtonHealth::default(),
            right: ButtonHealth::default(),
            fire: ButtonHealth::default(),
            cancel: ButtonHealth::default(),
            execute: ButtonHealth::default(),
        }
    }

    pub fn to_string(&self) -> String<7> {
        macro_rules! btn {
            ($s:ident, $val:expr, $c:literal) => {
                let _ = $s.push($val.char($c, $c.to_ascii_lowercase()));
            };
        }

        let mut s: String<7> = String::new();
        btn!(s, self.up, 'U');
        btn!(s, self.down, 'D');
        btn!(s, self.left, 'L');
        btn!(s, self.right, 'R');
        btn!(s, self.fire, 'F');
        btn!(s, self.cancel, 'C');
        btn!(s, self.execute, 'E');
        s
    }
}

pub struct SystemHealth {
    pub lora: PeripheralHealth,
    pub nfc: PeripheralHealth,
    pub epd: PeripheralHealth,
    pub buttons: ButtonsHealth,
}

impl SystemHealth {
    pub const fn new() -> Self {
        Self {
            lora: PeripheralHealth::default(),
            nfc: PeripheralHealth::default(),
            epd: PeripheralHealth::default(),
            buttons: ButtonsHealth::default(),
        }
    }

    pub fn to_string(&self) -> String<11> {
        let mut s: String<11> = String::new();
        let _ = s.push(self.lora.char('L', 'l'));
        let _ = s.push(self.nfc.char('N', 'n'));
        let _ = s.push(self.epd.char('E', 'e'));
        let _ = s.push('\n');
        let _ = s.push_str(&self.buttons.to_string());
        s
    }
}

pub static SYSTEM_HEALTH: Mutex<ThreadModeRawMutex, RefCell<SystemHealth>> =
    Mutex::new(RefCell::new(SystemHealth::new()));

/// Read-only access to system health.
/// Usage: with_health!(|h| defmt::info!("lora ok: {}", h.lora.healthy))
#[macro_export]
macro_rules! with_health {
    (| $h:ident | $body:expr) => {
        $crate::fw::health::SYSTEM_HEALTH.lock(|cell| {
            let $h: &$crate::fw::health::SystemHealth = &cell.borrow();
            $body
        })
    };
}

/// Mutable access to system health.
/// Usage: update_health!(|h| h.lora.set_err("TX timeout"))
/// Usage: update_health!(|h| h.lora.set_ok("Sent"))
/// Usage: update_health!(|h| h.buttons.up.seen_low = true)
#[macro_export]
macro_rules! update_health {
    (| $h:ident | $body:expr) => {
        $crate::fw::health::SYSTEM_HEALTH.lock(|cell| {
            let $h: &mut $crate::fw::health::SystemHealth = &mut cell.borrow_mut();
            $body
        })
    };
}

/// Set a peripheral health error.
/// Usage: health_err!(epd, "Failed to initialize")
#[macro_export]
macro_rules! health_err {
    ($peripheral:ident, $msg:expr) => {
        $crate::update_health!(|h| h.$peripheral.set_err($msg))
    };
}

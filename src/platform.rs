//! Firmware-vs-simulator shims shared by several modules: uptime, entropy,
//! the xorshift32 step, battery level and wall-clock seconds.  Keeps the
//! `cfg(embassy-core)` / `cfg(simulator)` split in one place.

/// The simulator's fake clock; `u64::MAX` means "use the real one".
#[cfg(feature = "simulator")]
static FAKE_UPTIME_MS: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(u64::MAX);

/// Make [`uptime_ms`] return `ms` from now on (`u64::MAX` goes back to the
/// real clock). Host-only: the guide's headless shots drive time by hand.
#[cfg(feature = "simulator")]
pub fn set_fake_uptime_ms(ms: u64) {
    FAKE_UPTIME_MS.store(ms, core::sync::atomic::Ordering::Relaxed);
}

/// The fake clock's value, or `None` while the real clock is in use.
#[cfg(feature = "simulator")]
pub fn fake_uptime_ms() -> Option<u64> {
    match FAKE_UPTIME_MS.load(core::sync::atomic::Ordering::Relaxed) {
        u64::MAX => None,
        ms => Some(ms),
    }
}

/// The host's pinned wall clock: unix seconds, or `i64::MIN` for the system
/// time.
#[cfg(feature = "simulator")]
static FAKE_UNIX: core::sync::atomic::AtomicI64 = core::sync::atomic::AtomicI64::new(i64::MIN);

/// 2026-08-01 12:00 local at the default `TZ_OFFSET_QH` (8, +2 h): the instant
/// lib tests and guide shots start from.
#[cfg(not(feature = "embassy-core"))]
pub const GUIDE_NOON_UNIX: u32 = 1_785_578_400;

/// Pin the host wall clock: `unix_secs()` = base + fake uptime / 1000 while
/// `Some`; `None` restores system time.
#[cfg(feature = "simulator")]
pub fn set_fake_unix(base: Option<u32>) {
    FAKE_UNIX.store(
        base.map_or(i64::MIN, i64::from),
        core::sync::atomic::Ordering::Relaxed,
    );
}

/// Milliseconds of uptime: embassy clock on firmware, process uptime on the
/// simulator.
pub fn uptime_ms() -> u64 {
    #[cfg(feature = "embassy-core")]
    {
        embassy_time::Instant::now().as_millis()
    }
    #[cfg(not(feature = "embassy-core"))]
    {
        use std::sync::OnceLock;
        use std::time::Instant;
        #[cfg(feature = "simulator")]
        if let Some(ms) = fake_uptime_ms() {
            return ms;
        }
        static START: OnceLock<Instant> = OnceLock::new();
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_millis() as u64
    }
}

/// Low 32 bits of [`uptime_ms`].
pub fn uptime_ms_u32() -> u32 {
    uptime_ms() as u32
}

/// Pseudo-random byte from the clock: low byte of the embassy tick counter on
/// firmware, of the UNIX-epoch nanoseconds on the simulator.
pub fn entropy_byte() -> u8 {
    #[cfg(feature = "embassy-core")]
    {
        embassy_time::Instant::now().as_ticks() as u8
    }
    #[cfg(not(feature = "embassy-core"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u8)
            .unwrap_or(0xCB)
    }
}

/// Seed for a puzzle generator: low 32 bits of the embassy tick counter on
/// firmware, a fixed constant on the simulator (deterministic puzzles).
pub fn seed_ticks_or_fixed() -> u32 {
    #[cfg(feature = "embassy-core")]
    {
        embassy_time::Instant::now().as_ticks() as u32
    }
    #[cfg(not(feature = "embassy-core"))]
    {
        0xCAFE_BABE
    }
}

/// One xorshift32 step (13/17/5).
#[inline]
pub fn xorshift32(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Battery level in percent; 100 on the simulator.
pub fn battery_pct() -> u8 {
    #[cfg(feature = "embassy-core")]
    {
        crate::fw::battery::read_pct()
    }
    #[cfg(not(feature = "embassy-core"))]
    {
        100
    }
}

/// `true` while the battery charger is active; always `false` on the simulator.
pub fn is_charging() -> bool {
    #[cfg(feature = "embassy-core")]
    {
        crate::fw::battery::is_charging()
    }
    #[cfg(not(feature = "embassy-core"))]
    {
        false
    }
}

/// UNIX seconds if the wall clock is known (always on the simulator).
pub fn unix_secs() -> Option<u32> {
    #[cfg(feature = "embassy-core")]
    {
        crate::unix_now()
    }
    #[cfg(not(feature = "embassy-core"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        #[cfg(feature = "simulator")]
        match FAKE_UNIX.load(core::sync::atomic::Ordering::Relaxed) {
            i64::MIN => {}
            base => return Some((base + (fake_uptime_ms().unwrap_or(0) / 1000) as i64) as u32),
        }
        Some(SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as u32)
    }
}

/// The "Cyber Ægg <id>" device label (title bar, name-entry footer): the
/// 4-hex-digit device ID on firmware, `A3F7` on the simulator.
pub fn device_label() -> heapless::String<16> {
    let mut s = heapless::String::new();
    let _ = s.push_str("Cyber \u{00C6}gg ");
    #[cfg(feature = "embassy-core")]
    {
        let id = crate::fw::device_id::get_bytes();
        let _ = s.push_str(core::str::from_utf8(&id).unwrap_or("????"));
    }
    #[cfg(not(feature = "embassy-core"))]
    {
        let _ = s.push_str("A3F7");
    }
    s
}

#[cfg(all(test, feature = "simulator"))]
mod tests {
    use super::*;

    #[test]
    fn unix_secs_follows_the_fake_clock() {
        let _guard = crate::game::lock_test_state();
        set_fake_unix(Some(1000));
        set_fake_uptime_ms(5_000);
        assert_eq!(unix_secs(), Some(1005));
        set_fake_unix(None);
        assert!(unix_secs().unwrap() > 1_700_000_000, "system time again");
        set_fake_uptime_ms(u64::MAX);
        set_fake_unix(None);
    }
}

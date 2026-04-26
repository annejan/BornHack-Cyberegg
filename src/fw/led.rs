//! LED driver — generic blink patterns for the RGB LED.
//!
//! Each LED colour has an atomic state that any task can set via
//! [`set_led`].  A single [`led_task`] reads all three states and drives
//! the GPIO pins.  All patterns use a fixed 1-second period.
//!
//! When all LEDs are off the task sleeps on a signal — no timer ticks,
//! no wasted CPU.  When active, the task wakes at most 3 times per
//! second (at 0 ms, 50 ms, and 500 ms within each period).

use core::sync::atomic::{AtomicU8, Ordering};

use embassy_nrf::gpio::Output;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;

/// LED pattern with a fixed 1-second duty cycle.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LedState {
    /// LED off.
    Off = 0,
    /// LED on continuously.
    On = 1,
    /// 50 % duty cycle (500 ms on, 500 ms off), repeating.
    Duty50 = 2,
    /// Short blink (50 ms on, 950 ms off), repeating.
    Blink = 3,
    /// Single 500 ms pulse, then auto-resets to Off.
    Duty50Once = 4,
    /// Single 50 ms blink, then auto-resets to Off.
    BlinkOnce = 5,
}

impl LedState {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::On,
            2 => Self::Duty50,
            3 => Self::Blink,
            4 => Self::Duty50Once,
            5 => Self::BlinkOnce,
            _ => Self::Off,
        }
    }
}

// One atomic per colour — writable from any task.
pub static LED_RED: AtomicU8 = AtomicU8::new(0);
pub static LED_GREEN: AtomicU8 = AtomicU8::new(0);
pub static LED_BLUE: AtomicU8 = AtomicU8::new(0);

/// Signalled by [`set_led`] / [`all_off`] to wake the task from sleep.
static WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Set a single LED's pattern.
pub fn set_led(led: &AtomicU8, state: LedState) {
    led.store(state as u8, Ordering::Relaxed);
    WAKE.signal(());
}

/// Turn all LEDs off.
pub fn all_off() {
    LED_RED.store(0, Ordering::Relaxed);
    LED_GREEN.store(0, Ordering::Relaxed);
    LED_BLUE.store(0, Ordering::Relaxed);
    WAKE.signal(());
}

fn any_active() -> bool {
    LED_RED.load(Ordering::Relaxed) != 0
        || LED_GREEN.load(Ordering::Relaxed) != 0
        || LED_BLUE.load(Ordering::Relaxed) != 0
}

/// Drive three LED pins according to their state.
///
/// Spawned once from main.  LEDs are active-low (set_low = on).
///
/// The 1-second period is split into three phases to minimise wake-ups:
///   Phase 0 (0–50 ms):   Blink + Duty50 + On are on.
///   Phase 1 (50–500 ms): Duty50 + On stay on, Blink turns off.
///   Phase 2 (500–1000 ms): Only On stays on, Duty50 turns off.
#[embassy_executor::task]
pub async fn led_task(
    mut red: Output<'static>,
    mut green: Output<'static>,
    mut blue: Output<'static>,
) -> ! {
    use embassy_futures::select::{Either, select};

    loop {
        if !any_active() {
            red.set_high();
            green.set_high();
            blue.set_high();
            WAKE.wait().await;
            continue;
        }

        // Phase 0: Blink/BlinkOnce + Duty50/Duty50Once + On turn on.
        drive(&mut red, &LED_RED, 0);
        drive(&mut green, &LED_GREEN, 0);
        drive(&mut blue, &LED_BLUE, 0);
        if matches!(
            select(Timer::after_millis(50), WAKE.wait()).await,
            Either::Second(_)
        ) {
            continue; // new state arrived — restart cycle
        }

        // Phase 1: Blink variants turn off; BlinkOnce auto-resets.
        auto_reset(&LED_RED, LedState::BlinkOnce);
        auto_reset(&LED_GREEN, LedState::BlinkOnce);
        auto_reset(&LED_BLUE, LedState::BlinkOnce);
        drive(&mut red, &LED_RED, 1);
        drive(&mut green, &LED_GREEN, 1);
        drive(&mut blue, &LED_BLUE, 1);
        if matches!(
            select(Timer::after_millis(450), WAKE.wait()).await,
            Either::Second(_)
        ) {
            continue;
        }

        // Phase 2: Duty50 variants turn off; Duty50Once auto-resets.
        auto_reset(&LED_RED, LedState::Duty50Once);
        auto_reset(&LED_GREEN, LedState::Duty50Once);
        auto_reset(&LED_BLUE, LedState::Duty50Once);
        drive(&mut red, &LED_RED, 2);
        drive(&mut green, &LED_GREEN, 2);
        drive(&mut blue, &LED_BLUE, 2);
        if matches!(
            select(Timer::after_millis(500), WAKE.wait()).await,
            Either::Second(_)
        ) {
            continue;
        }
    }
}

/// Set pin for the given phase.  Active-low: set_low = on.
fn drive(pin: &mut Output<'_>, state: &AtomicU8, phase: u8) {
    let on = match LedState::from_u8(state.load(Ordering::Relaxed)) {
        LedState::Off => false,
        LedState::On => true,
        LedState::Duty50 | LedState::Duty50Once => phase < 2,
        LedState::Blink | LedState::BlinkOnce => phase == 0,
    };
    if on {
        pin.set_low();
    } else {
        pin.set_high();
    }
}

/// If the LED is in `target` state, reset it to Off.
fn auto_reset(led: &AtomicU8, target: LedState) {
    if LedState::from_u8(led.load(Ordering::Relaxed)) == target {
        led.store(0, Ordering::Relaxed);
    }
}

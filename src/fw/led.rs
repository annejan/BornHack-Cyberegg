//! RGB LED driver — hardware PWM colour mixing.
//!
//! The three LED channels share one PWM instance, so they run off one
//! counter period and differ only in duty.  Any task sets the colour with
//! [`set_color`]: the colour holds for `duration`, then the LED turns off.
//! The latest call always wins and replaces a running one straight away,
//! so an animation is a series of `set_color` calls, each made before the
//! previous one's duration runs out (see [`about_rainbow_task`]).
//!
//! While the LED is off the PWM peripheral is disabled — no HFCLK request,
//! no timer wake-ups.  The pins then idle high, which is off for this
//! active-low LED.

use embassy_futures::select::{Either, select};
use embassy_nrf::gpio::Level;
use embassy_nrf::pwm::{DutyCycle, Prescaler, SimpleConfig, SimplePwm};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};

/// PWM counter top.  10 bits of duty so the gamma curve keeps its low end;
/// with the Div4 prescaler (4 MHz) this is a ~3.9 kHz PWM.  A short period
/// also keeps `SimplePwm::set_all_duties` quick: it busy-waits for one
/// period while the PWM runs.
const PWM_TOP: u16 = 1023;

/// Hold a colour until the next [`set_color`] call.
pub const FOREVER: Duration = Duration::MAX;

/// An LED colour, 8 bits per channel.  The driver applies a gamma of 2,
/// so 128 looks about half as bright as 255.
#[derive(Clone, Copy, PartialEq, Eq, defmt::Format)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Self = Self::new(0, 0, 0);
    pub const RED: Self = Self::new(255, 0, 0);
    pub const GREEN: Self = Self::new(0, 255, 0);
    pub const BLUE: Self = Self::new(0, 0, 255);
    pub const ORANGE: Self = Self::new(255, 96, 0);
    pub const WHITE: Self = Self::new(255, 255, 255);

    /// Number of steps in one trip round the colour wheel ([`Self::from_hue`]).
    pub const HUE_STEPS: u16 = 6 * 256;

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Fully saturated colour on the wheel red → yellow → green → cyan →
    /// blue → magenta → red.  `hue` wraps at [`Self::HUE_STEPS`].
    pub const fn from_hue(hue: u16) -> Self {
        let h = hue % Self::HUE_STEPS;
        let x = (h % 256) as u8;
        match h / 256 {
            0 => Self::new(255, x, 0),
            1 => Self::new(255 - x, 255, 0),
            2 => Self::new(0, 255, x),
            3 => Self::new(0, 255 - x, 255),
            4 => Self::new(x, 0, 255),
            _ => Self::new(255, 0, 255 - x),
        }
    }
}

/// Latest colour request.  A `Signal` keeps only the newest value, which is
/// exactly "latest call wins".
static CMD: Signal<CriticalSectionRawMutex, (Rgb, Duration)> = Signal::new();

/// Show `color` for `duration`, then turn the LED off.  Replaces any colour
/// still showing.  Pass [`FOREVER`] to hold until the next call.
pub fn set_color(color: Rgb, duration: Duration) {
    CMD.signal((color, duration));
}

/// Turn the LED off now.
pub fn off() {
    set_color(Rgb::OFF, Duration::from_ticks(0));
}

/// PWM configuration for the LED: pins idle high (LED off) while the PWM is
/// disabled.
pub fn pwm_config() -> SimpleConfig {
    let mut config = SimpleConfig::default();
    config.max_duty = PWM_TOP;
    config.prescaler = Prescaler::Div4;
    config.ch0_idle_level = Level::High;
    config.ch1_idle_level = Level::High;
    config.ch2_idle_level = Level::High;
    config.ch3_idle_level = Level::High;
    config
}

/// Gamma-2 duty for an 8-bit channel value.  Normal polarity drives the pin
/// low while the counter is below the duty, and low is on for this LED, so
/// the on-time is `duty / PWM_TOP`.
fn duty(v: u8) -> DutyCycle {
    let v = v as u32;
    let d = (v * v * PWM_TOP as u32 + 255 * 255 / 2) / (255 * 255);
    DutyCycle::normal(d as u16)
}

/// Drive the LED PWM.  Spawned once from main with the PWM built from
/// [`pwm_config`], channels 0/1/2 = red/green/blue.
#[embassy_executor::task]
pub async fn led_task(mut pwm: SimplePwm<'static>) -> ! {
    pwm.disable();
    let mut cmd = CMD.wait().await;
    loop {
        let (color, duration) = cmd;
        if color == Rgb::OFF || duration == Duration::from_ticks(0) {
            pwm.disable();
            cmd = CMD.wait().await;
            continue;
        }

        if !pwm.is_enabled() {
            pwm.enable();
        }
        pwm.set_all_duties([
            duty(color.r),
            duty(color.g),
            duty(color.b),
            DutyCycle::normal(0),
        ]);

        cmd = match Instant::now().checked_add(duration) {
            Some(deadline) => match select(Timer::at(deadline), CMD.wait()).await {
                Either::First(()) => (Rgb::OFF, Duration::from_ticks(0)),
                Either::Second(next) => next,
            },
            // FOREVER (or anything past the end of time): hold until the
            // next call.
            None => CMD.wait().await,
        };
    }
}

/// Signalled each time the About screen is drawn.
pub static ABOUT_RAINBOW: Signal<CriticalSectionRawMutex, ()> = Signal::new();

/// Fade the LED round the colour wheel while the About screen is open.
///
/// Updates the colour at 50 Hz.  Each call holds for three steps, so a late
/// tick never blanks the LED, and the LED goes dark by itself if this task
/// stops calling.
#[embassy_executor::task]
pub async fn about_rainbow_task() -> ! {
    const STEP_MS: u64 = 20;
    // 1536 / 5 steps × 20 ms ≈ 6 s per trip round the wheel.
    const HUE_STEP: u16 = 5;

    loop {
        ABOUT_RAINBOW.wait().await;
        let mut hue = 0u16;
        let mut ran = false;
        while crate::about_active() {
            ran = true;
            set_color(Rgb::from_hue(hue), Duration::from_millis(3 * STEP_MS));
            hue = (hue + HUE_STEP) % Rgb::HUE_STEPS;
            Timer::after_millis(STEP_MS).await;
        }
        // Drop the signal from the last About redraw so it doesn't restart
        // the loop just to find About closed.
        ABOUT_RAINBOW.reset();
        if ran {
            off();
        }
    }
}

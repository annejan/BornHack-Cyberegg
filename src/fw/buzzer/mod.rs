pub mod melodies;

use embassy_nrf::pwm::{DutyCycle, SimplePwm};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::signal::Signal;
use embassy_time::Timer;

/// All available melodies, addressable by index.
pub const MELODIES: &[&[Tone]] = &[
    melodies::STARTUP,        // 0
    melodies::RICK_INTRO,     // 1
    melodies::IMPERIAL_MARCH, // 2
    melodies::SANDSTORM,      // 3
    melodies::PINK_PANTHER,   // 4
];

/// Signal a melody index to the buzzer task.
/// If a melody is already playing it will be interrupted at the next note boundary.
static MELODY_SIGNAL: Signal<CriticalSectionRawMutex, usize> = Signal::new();

/// Trigger melody `index` (see [`MELODIES`]) without blocking the caller.
/// Out-of-range indices are silently ignored.
pub fn play(index: usize) {
    if index < MELODIES.len() {
        MELODY_SIGNAL.signal(index);
    }
}

/// Embassy task that owns the buzzer and plays melodies on demand.
/// Spawn once from `main`; use [`play`] to trigger melodies from anywhere.
#[embassy_executor::task]
pub async fn buzzer_task(mut buzzer: Buzzer<'static>) {
    loop {
        let index = MELODY_SIGNAL.wait().await;
        if let Some(melody) = MELODIES.get(index) {
            for &tone in *melody {
                // A new melody arrived — finish this tone then switch.
                buzzer.play(tone).await;
                if MELODY_SIGNAL.signaled() {
                    break;
                }
            }
            buzzer.pwm.disable();
        }
    }
}

/// Musical note — either a MIDI note number or silence.
///
/// Named constants are provided for convenience (C2–B5), but any MIDI note
/// (0–127) can be used via `Note::Midi(n)`.  The PWM buzzer has no inherent
/// range limit; audibility depends on the piezo element (~100 Hz – 5 kHz).
#[derive(Clone, Copy, PartialEq)]
pub enum Note {
    Midi(u8),
    Rest,
}

#[allow(dead_code, non_upper_case_globals)]
impl Note {
    // Octave 2
    pub const C2: Self = Self::Midi(36); pub const Cs2: Self = Self::Midi(37);
    pub const D2: Self = Self::Midi(38); pub const Ds2: Self = Self::Midi(39);
    pub const E2: Self = Self::Midi(40); pub const F2: Self = Self::Midi(41);
    pub const Fs2: Self = Self::Midi(42); pub const G2: Self = Self::Midi(43);
    pub const Gs2: Self = Self::Midi(44); pub const A2: Self = Self::Midi(45);
    pub const As2: Self = Self::Midi(46); pub const B2: Self = Self::Midi(47);
    // Octave 3
    pub const C3: Self = Self::Midi(48); pub const Cs3: Self = Self::Midi(49);
    pub const D3: Self = Self::Midi(50); pub const Ds3: Self = Self::Midi(51);
    pub const E3: Self = Self::Midi(52); pub const F3: Self = Self::Midi(53);
    pub const Fs3: Self = Self::Midi(54); pub const G3: Self = Self::Midi(55);
    pub const Gs3: Self = Self::Midi(56); pub const A3: Self = Self::Midi(57);
    pub const As3: Self = Self::Midi(58); pub const B3: Self = Self::Midi(59);
    // Octave 4
    pub const C4: Self = Self::Midi(60); pub const Cs4: Self = Self::Midi(61);
    pub const D4: Self = Self::Midi(62); pub const Ds4: Self = Self::Midi(63);
    pub const E4: Self = Self::Midi(64); pub const F4: Self = Self::Midi(65);
    pub const Fs4: Self = Self::Midi(66); pub const G4: Self = Self::Midi(67);
    pub const Gs4: Self = Self::Midi(68); pub const A4: Self = Self::Midi(69);
    pub const As4: Self = Self::Midi(70); pub const B4: Self = Self::Midi(71);
    // Octave 5
    pub const C5: Self = Self::Midi(72); pub const Cs5: Self = Self::Midi(73);
    pub const D5: Self = Self::Midi(74); pub const Ds5: Self = Self::Midi(75);
    pub const E5: Self = Self::Midi(76); pub const F5: Self = Self::Midi(77);
    pub const Fs5: Self = Self::Midi(78); pub const G5: Self = Self::Midi(79);
    pub const Gs5: Self = Self::Midi(80); pub const A5: Self = Self::Midi(81);
    pub const As5: Self = Self::Midi(82); pub const B5: Self = Self::Midi(83);
    // Octave 6
    pub const C6: Self = Self::Midi(84); pub const Cs6: Self = Self::Midi(85);
    pub const D6: Self = Self::Midi(86); pub const Ds6: Self = Self::Midi(87);
    pub const E6: Self = Self::Midi(88); pub const F6: Self = Self::Midi(89);
    pub const Fs6: Self = Self::Midi(90); pub const G6: Self = Self::Midi(91);
    pub const Gs6: Self = Self::Midi(92); pub const A6: Self = Self::Midi(93);
    pub const As6: Self = Self::Midi(94); pub const B6: Self = Self::Midi(95);

    /// Frequency in Hz from MIDI note number.
    /// Uses a lookup table for the top octave (MIDI 60–71 = C4–B4) and
    /// shifts for other octaves to stay in integer arithmetic.
    pub const fn freq_hz(self) -> u32 {
        match self {
            Note::Rest => 0,
            Note::Midi(m) => {
                // C4..B4 base frequencies (×16 for sub-octave shifting precision)
                const BASE: [u32; 12] = [
                    4186, 4435, 4699, 4978, 5274, 5588,
                    5920, 6272, 6645, 7040, 7459, 7902,
                ];
                let semitone = (m % 12) as usize;
                let octave = (m / 12) as i32 - 9; // -4 extra to divide out the ×16 factor
                let f16 = BASE[semitone]; // frequency × 16
                if octave >= 0 {
                    f16 << (octave as u32)
                } else {
                    f16 >> ((-octave) as u32)
                }
            }
        }
    }
}

/// A single step in a melody: a note and how long to play it (ms)
#[derive(Clone, Copy)]
pub struct Tone {
    pub note: Note,
    pub duration_ms: u32,
}

impl Tone {
    pub const fn new(note: Note, duration_ms: u32) -> Self {
        Self { note, duration_ms }
    }
}

/// Convenience shorthand: `tone!(A4, 200)` → `Tone::new(Note::A4, 200)`
#[macro_export]
macro_rules! tone {
    ($note:ident, $ms:expr) => {
        $crate::fw::buzzer::Tone::new($crate::fw::buzzer::Note::$note, $ms)
    };
}

/// PWM-driven passive buzzer.
///
/// The PWM peripheral generates the tone waveform autonomously — no
/// per-half-period timer wakes needed. For each note `set_period` loads the
/// correct COUNTERTOP, a 50% `DutyCycle` is set, and `enable()`/`disable()`
/// bookend the `Timer::after_millis` wait. The idle pin level is LOW
/// (configured via [`SimpleConfig`]), so silence and rests keep the pin low.
pub struct Buzzer<'d> {
    pwm: SimplePwm<'d>,
}

impl<'d> Buzzer<'d> {
    /// Take ownership of a [`SimplePwm`] configured for buzzer use.
    pub fn new(pwm: SimplePwm<'d>) -> Self {
        // PWM starts disabled; idle level is LOW (SimpleConfig default).
        Self { pwm }
    }

    /// Play `freq_hz` for `duration_ms` milliseconds. `freq_hz = 0` is silence.
    pub async fn play_freq(&mut self, freq_hz: u32, duration_ms: u32) {
        if freq_hz == 0 {
            // Rest: pin stays LOW (PWM disabled), just wait.
            Timer::after_millis(duration_ms as u64).await;
            return;
        }

        // set_period computes COUNTERTOP = PWM_CLK / freq (default Div16 → 1 MHz base).
        self.pwm.set_period(freq_hz);
        // Enable before set_duty so that sync_duty_cyles_to_peripheral fires SEQSTART
        // while the peripheral is enabled — it then waits for SEQEND, guaranteeing the
        // waveform is loaded before the timer starts.  (new_inner already enables, but
        // disable() in a previous note turns it off.)
        self.pwm.enable();
        // 50% square wave: duty = COUNTERTOP / 2.
        // DutyCycle::normal(v): output HIGH when counter >= v.
        let duty = DutyCycle::normal(self.pwm.max_duty() / 2);
        self.pwm.set_duty(0, duty);

        Timer::after_millis(duration_ms as u64).await;

        self.pwm.disable(); // pin returns to idle LOW
    }

    /// Play a single [`Tone`].
    pub async fn play(&mut self, tone: Tone) {
        self.play_freq(tone.note.freq_hz(), tone.duration_ms).await;
    }

    /// Play a slice of [`Tone`]s in order.
    pub async fn play_melody(&mut self, melody: &[Tone]) {
        for &tone in melody {
            self.play(tone).await;
        }
    }
}

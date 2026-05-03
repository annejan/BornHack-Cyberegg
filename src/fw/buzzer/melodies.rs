#![allow(dead_code)]

use super::{Note, Tone};

// BPM 100 note durations:
//   quarter       (Q)  = 600ms
//   dotted-eighth (D8) = 450ms
//   eighth        (E)  = 300ms
//   sixteenth     (S)  = 150ms
//   half          (H)  = 1200ms
//
// Original key: G minor.  Transposed DOWN one octave so every note fits
// within the C3-B4 range supported by the Buzzer driver.
//
// Rhythm confirmed from sheet music: the opening motif is
//   G(Q) G(Q) G(Q) Eb(D8) Bb(S)  — three equal quarters, then dotted-eighth +
// sixteenth NOT three dotted-quarters as commonly misremembered.
//
// P = inter-note pause inserted between consecutive same-pitch notes.
// The note before is shortened by P to keep bar length correct.

const Q: u32 = 600;
const D8: u32 = 450;
const E: u32 = 300;
const S: u32 = 150;
const H: u32 = 1200;
const P: u32 = 50;

/// Pet severity alert: two short A5 beeps with a brief gap.  Played
/// whenever the pet state machine transitions upward between neutral,
/// warning, severe-warning and leaving.  The two notes are currently
/// identical — the structure is already in place if you want to swap the
/// second note for a different pitch later.
pub const PET_WARN: &[Tone] = &[
    Tone::new(Note::A5, 120),
    Tone::new(Note::Rest, 80),
    Tone::new(Note::A5, 120),
];

/// "funny ending" (composed by LK) — a brief comedic send-off played
/// when the pet finally leaves (transitions to `Phase::Gone`).
/// Time signature 4/4 at ♩ = 180, so one beat = 333 ms.
///
/// The score's measure 2 ends on a stacked chord; the buzzer is
/// monophonic so that chord is rendered as a quick Eb → G → Bb arpeggio
/// into a held Bb for the remainder of the measure.
pub const FUNNY_ENDING: &[Tone] = &[
    // Measure 1: G | G A | Bb G | F — playful descending motif.
    Tone::new(Note::G4, 333),
    Tone::new(Note::G4, 166),
    Tone::new(Note::A4, 166),
    Tone::new(Note::As4, 166),
    Tone::new(Note::G4, 166),
    Tone::new(Note::F4, 333),
    // Measure 2: rest, then arpeggiated Eb-G-Bb chord landing on a
    // half-note Bb.
    Tone::new(Note::Rest, 333),
    Tone::new(Note::Ds4, 166),
    Tone::new(Note::G4, 166),
    Tone::new(Note::As4, 666),
];

pub const STARTUP: &[Tone] = &[
    Tone::new(Note::A3, 120),
    Tone::new(Note::C4, 120),
    Tone::new(Note::E4, 120),
    Tone::new(Note::A4, 300),
];

// "Never Gonna Give You Up" chorus – from rick.mid
// Tempo 508474 µs/beat (~118 BPM), 128 ppq → 3.97 ms/tick
// Key: Ab major (MIDI 56=Ab3, 58=Bb3, 61=Db4, 63=Eb4, 65=F4)
// Note durations = slot size (next_on − this_on × 3.97 ms).
// RP separates consecutive same-pitch notes.
const RI16: u32 = 127; // sixteenth  (32 ticks)
const RI8: u32 = 254; // eighth     (64 ticks)
const RIQ: u32 = 508; // quarter   (128 ticks)
const RID8: u32 = 381; // dotted-eighth (96 ticks)
const RIDQ: u32 = 763; // dotted-quarter (192 ticks)
const RIP: u32 = 20; // inter-same-note pause

pub const RICK_INTRO: &[Tone] = &[
    // "Ne-ver-gon-na" pickup (4× sixteenth): Ab3 Bb3 Db4 Bb3
    Tone::new(Note::Gs3, RI16),
    Tone::new(Note::As3, RI16),
    Tone::new(Note::Cs4, RI16),
    Tone::new(Note::As3, RI16),
    // "give you up": F4(Q) F4(E) Eb4(D.)
    Tone::new(Note::F4, RIQ - RIP),
    Tone::new(Note::Rest, RIP),
    Tone::new(Note::F4, RI8),
    Tone::new(Note::Ds4, RIDQ),
    // "Ne-ver-gon-na" pickup (4× sixteenth): Ab3 Bb3 Db4 Bb3
    Tone::new(Note::Gs3, RI16),
    Tone::new(Note::As3, RI16),
    Tone::new(Note::Cs4, RI16),
    Tone::new(Note::As3, RI16),
    // "let you down": Eb4(Q) Eb4(E) Db4(D8) C4(S) Bb3(Q)
    Tone::new(Note::Ds4, RIQ - RIP),
    Tone::new(Note::Rest, RIP),
    Tone::new(Note::Ds4, RI8),
    Tone::new(Note::Cs4, RID8),
    Tone::new(Note::C4, RI16),
    Tone::new(Note::As3, RIQ),
];

// Darude – Sandstorm (main riff)
// Tempo: 428571 µs/beat = 140 BPM, 96 ppq → 4.464 ms/tick
// Sixteenth (S) = 24 ticks = 107 ms  |  Eighth (E) = 48 ticks = 214 ms
// Notes: C4(60) Ds4(63) F4(65) As3(58)  — all within C3-B4 range
// SP separates consecutive same-pitch notes.
const SS: u32 = 107; // sixteenth
const SE: u32 = 214; // eighth
const SP: u32 = 30; // inter-same-note gap

pub const SANDSTORM: &[Tone] = &[
    // ── A: 4×C4(S) C4(E) 6×C4(S) C4(E) ─────────────────────────────────────
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SE - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SE),
    // ── B: 6×F4(S) F4(E) ─────────────────────────────────────────────────────
    Tone::new(Note::F4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::F4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::F4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::F4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::F4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::F4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::F4, SE),
    // ── C: 6×Eb4(S) Eb4(E) ───────────────────────────────────────────────────
    Tone::new(Note::Ds4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::Ds4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::Ds4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::Ds4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::Ds4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::Ds4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::Ds4, SE),
    // ── D: Bb3(E) ─────────────────────────────────────────────────────────────
    Tone::new(Note::As3, SE),
    // ── A ────────────────────────────────────────────────────────────────────
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SE - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SE),
    // ── E: F4(E) ──────────────────────────────────────────────────────────────
    Tone::new(Note::F4, SE),
    // ── A ────────────────────────────────────────────────────────────────────
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SE - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SS - SP),
    Tone::new(Note::Rest, SP),
    Tone::new(Note::C4, SE),
    // ── E: F4(E) ──────────────────────────────────────────────────────────────
    Tone::new(Note::F4, SE),
];

pub const IMPERIAL_MARCH: &[Tone] = &[
    // ── Phrase 1 ────────────────────────────────────────────────────
    // G(Q) G(Q) G(Q) Eb(D8) Bb(S) | G(Q) Eb(D8) Bb(S) G(H)
    Tone::new(Note::G3, Q - P),
    Tone::new(Note::Rest, P), // G  quarter
    Tone::new(Note::G3, Q - P),
    Tone::new(Note::Rest, P), // G  quarter
    Tone::new(Note::G3, Q),   // G  quarter
    Tone::new(Note::Ds3, D8), // Eb dotted-eighth
    Tone::new(Note::As3, S),  // Bb sixteenth
    Tone::new(Note::G3, Q),   // G  quarter
    Tone::new(Note::Ds3, D8), // Eb dotted-eighth
    Tone::new(Note::As3, S),  // Bb sixteenth
    Tone::new(Note::G3, H),   // G  half
    // ── Phrase 2 (a fifth higher) ────────────────────────────────────
    // D(Q) D(Q) D(Q) Eb(D8) Bb(S) | Gb(Q) Eb(D8) Bb(S) G(H)
    Tone::new(Note::D4, Q - P),
    Tone::new(Note::Rest, P), // D  quarter
    Tone::new(Note::D4, Q - P),
    Tone::new(Note::Rest, P), // D  quarter
    Tone::new(Note::D4, Q),   // D  quarter
    Tone::new(Note::Ds4, D8), // Eb dotted-eighth
    Tone::new(Note::As3, S),  // Bb sixteenth
    Tone::new(Note::Fs3, Q),  // Gb quarter
    Tone::new(Note::Ds3, D8), // Eb dotted-eighth
    Tone::new(Note::As3, S),  // Bb sixteenth
    Tone::new(Note::G3, H),   // G  half
    // ── Section B – part 1 ───────────────────────────────────────────
    // G4(Q) G3(D8) G3(S) G4(Q) F#4(D8) F4(S)
    Tone::new(Note::G4, Q), // G4 quarter
    Tone::new(Note::G3, D8 - P),
    Tone::new(Note::Rest, P), // G3 dotted-eighth
    Tone::new(Note::G3, S),   // G3 sixteenth
    Tone::new(Note::G4, Q),   // G4 quarter
    Tone::new(Note::Fs4, D8), // F# dotted-eighth
    Tone::new(Note::F4, S),   // F  sixteenth
    // ── Section B – part 2 ───────────────────────────────────────────
    // E(S) Eb(S) E(E) rest(E) Ab(E) Db(Q) C(D8) B(S)
    Tone::new(Note::E4, S),   // E  sixteenth
    Tone::new(Note::Ds4, S),  // Eb sixteenth
    Tone::new(Note::E4, E),   // E  eighth
    Tone::new(Note::Rest, E), // - eighth rest
    Tone::new(Note::Gs3, E),  // Ab eighth
    Tone::new(Note::Cs4, Q),  // Db quarter
    Tone::new(Note::C4, D8),  // C  dotted-eighth
    Tone::new(Note::B3, S),   // B  sixteenth
    // ── Section B – part 3 ───────────────────────────────────────────
    // Bb(S) A(S) Bb(E) rest(E) Eb(E) Gb(Q) Eb(D8) Bb(S)
    Tone::new(Note::As3, S),  // Bb sixteenth
    Tone::new(Note::A3, S),   // A  sixteenth
    Tone::new(Note::As3, E),  // Bb eighth
    Tone::new(Note::Rest, E), // - eighth rest
    Tone::new(Note::Ds3, E),  // Eb eighth
    Tone::new(Note::Fs3, Q),  // Gb quarter
    Tone::new(Note::Ds3, D8), // Eb dotted-eighth
    Tone::new(Note::As3, S),  // Bb sixteenth
    // ── Phrase 1 reprise ─────────────────────────────────────────────
    // G(Q) Eb(D8) Bb(S) G(H)
    Tone::new(Note::G3, Q),   // G  quarter
    Tone::new(Note::Ds3, D8), // Eb dotted-eighth
    Tone::new(Note::As3, S),  // Bb sixteenth
    Tone::new(Note::G3, H),   // G  half
];

// "The Pink Panther Theme" — Henry Mancini, 1963.
// Transcribed for PWM buzzer (monophonic) from the "Moderately Mysterious"
// piano arrangement: three statements of the sneaky chromatic motif with
// variations, ending on the fermata-held tonic (mirroring the `pp` fade
// in the final measure of the score).
//
// Tempo: ♩ = 120 (swing feel).  Note durations in milliseconds:
//   sixteenth = 125  eighth        = 250
//   quarter   = 500  dotted qtr    = 750
//   half      = 1000 fermata       = 1500
pub const PINK_PANTHER: &[Tone] = &[
    // ── Statement 1 — sneaky motif with descending tag ─────────────
    Tone::new(Note::Cs5, 125),
    Tone::new(Note::D5, 750),
    Tone::new(Note::E5, 125),
    Tone::new(Note::F5, 750),
    Tone::new(Note::Cs5, 125),
    Tone::new(Note::D5, 250),
    Tone::new(Note::E5, 125),
    Tone::new(Note::F5, 250),
    Tone::new(Note::As5, 125),
    Tone::new(Note::A5, 250),
    Tone::new(Note::D5, 125),
    Tone::new(Note::F5, 250),
    Tone::new(Note::A5, 125),
    Tone::new(Note::Gs5, 750),
    // Descending run
    Tone::new(Note::G5, 125),
    Tone::new(Note::F5, 125),
    Tone::new(Note::D5, 125),
    Tone::new(Note::C5, 125),
    Tone::new(Note::D5, 1000),
    Tone::new(Note::Rest, 250),
    // ── Statement 2 — motif climbs to Cs6 instead of falling ───────
    Tone::new(Note::Cs5, 125),
    Tone::new(Note::D5, 750),
    Tone::new(Note::E5, 125),
    Tone::new(Note::F5, 750),
    Tone::new(Note::Cs5, 125),
    Tone::new(Note::D5, 250),
    Tone::new(Note::E5, 125),
    Tone::new(Note::F5, 250),
    Tone::new(Note::As5, 125),
    Tone::new(Note::A5, 250),
    Tone::new(Note::F5, 125),
    Tone::new(Note::A5, 250),
    Tone::new(Note::D6, 125),
    Tone::new(Note::Cs6, 1000),
    Tone::new(Note::Rest, 250),
    // ── Statement 3 — mirror of statement 1, ends on the fermata ───
    Tone::new(Note::Cs5, 125),
    Tone::new(Note::D5, 750),
    Tone::new(Note::E5, 125),
    Tone::new(Note::F5, 750),
    Tone::new(Note::Cs5, 125),
    Tone::new(Note::D5, 250),
    Tone::new(Note::E5, 125),
    Tone::new(Note::F5, 250),
    Tone::new(Note::As5, 125),
    Tone::new(Note::A5, 250),
    Tone::new(Note::D5, 125),
    Tone::new(Note::F5, 250),
    Tone::new(Note::A5, 125),
    Tone::new(Note::Gs5, 750),
    Tone::new(Note::G5, 125),
    Tone::new(Note::F5, 125),
    Tone::new(Note::D5, 125),
    Tone::new(Note::C5, 125),
    // Final held tonic — fermata + pp fade.
    Tone::new(Note::D5, 1500),
];

// Eduard Khil – "Trololo" (vocal melody, first two phrases)
// Converted from MIDI: Tempo 333333 µs/beat (180 BPM), 480 ppq → 0.694 ms/tick
// Track 1 (melody), ~29 seconds. RP separates consecutive same-pitch notes.
pub const TROLOLO: &[Tone] = &[
    // ── Phrase 1: "Ohhh-oh-oh-ohhh..." ──────────────────────────────────
    Tone::new(Note::C4, 1999),
    Tone::new(Note::G3, 333),
    Tone::new(Note::F3, 333),
    Tone::new(Note::G3, 1999),
    Tone::new(Note::C3, 333),
    Tone::new(Note::D3, 333),
    Tone::new(Note::E3, 1333),
    Tone::new(Note::G3, 1333),
    Tone::new(Note::E3, 333),
    Tone::new(Note::D3, 333),
    Tone::new(Note::Rest, 1333),
    // ── Pickup: ascending run ────────────────────────────────────────────
    Tone::new(Note::G3, 166),
    Tone::new(Note::Gs3, 166),
    Tone::new(Note::A3, 166),
    Tone::new(Note::B3, 166),
    // ── Phrase 2: repeat with variation ──────────────────────────────────
    Tone::new(Note::C4, 1999),
    Tone::new(Note::G3, 499),
    Tone::new(Note::F3, 166),
    Tone::new(Note::G3, 1999),
    Tone::new(Note::C3, 333),
    Tone::new(Note::D3, 333),
    Tone::new(Note::E3, 2666),
    Tone::new(Note::D3, 333),
    Tone::new(Note::C3, 333),
    Tone::new(Note::Rest, 1334),
    // ── Bridge A: descending arpeggios ───────────────────────────────────
    Tone::new(Note::C3, 166),
    Tone::new(Note::E3, 166),
    Tone::new(Note::G3, 166),
    Tone::new(Note::C4, 166),
    Tone::new(Note::B3, 333),
    Tone::new(Note::Rest, RIP),
    Tone::new(Note::B3, 146),
    Tone::new(Note::G3, 166),
    Tone::new(Note::A3, 333),
    Tone::new(Note::Rest, RIP),
    Tone::new(Note::A3, 146),
    Tone::new(Note::F3, 166),
    Tone::new(Note::G3, 666),
    Tone::new(Note::G2, 166),
    Tone::new(Note::B2, 166),
    Tone::new(Note::D3, 166),
    Tone::new(Note::F3, 166),
    Tone::new(Note::E3, 1333),
    Tone::new(Note::Rest, 667),
    // ── Bridge B: repeat ─────────────────────────────────────────────────
    Tone::new(Note::C3, 166),
    Tone::new(Note::E3, 166),
    Tone::new(Note::G3, 166),
    Tone::new(Note::C4, 166),
    Tone::new(Note::B3, 333),
    Tone::new(Note::Rest, RIP),
    Tone::new(Note::B3, 146),
    Tone::new(Note::G3, 166),
    Tone::new(Note::A3, 333),
    Tone::new(Note::Rest, RIP),
    Tone::new(Note::A3, 146),
    Tone::new(Note::F3, 166),
    Tone::new(Note::G3, 666),
    Tone::new(Note::Rest, RIP),
    Tone::new(Note::G3, 146),
    Tone::new(Note::Gs3, 166),
    Tone::new(Note::A3, 166),
    Tone::new(Note::B3, 166),
];

/// Watch alarm — classic clock-radio buzz pattern.  Four "beep beep"
/// pairs with a longer rest between pairs, total ~3.0 s.  Plays once
/// per minute boundary when the watch alarm matches the wall clock.
pub const ALARM: &[Tone] = &[
    Tone::new(Note::A5, 150),
    Tone::new(Note::Rest, 100),
    Tone::new(Note::A5, 150),
    Tone::new(Note::Rest, 250),
    Tone::new(Note::A5, 150),
    Tone::new(Note::Rest, 100),
    Tone::new(Note::A5, 150),
    Tone::new(Note::Rest, 250),
    Tone::new(Note::A5, 150),
    Tone::new(Note::Rest, 100),
    Tone::new(Note::A5, 150),
    Tone::new(Note::Rest, 250),
    Tone::new(Note::A5, 150),
    Tone::new(Note::Rest, 100),
    Tone::new(Note::A5, 150),
];

// "Daisy Bell" (Harry Dacre, 1892) — first phrase of the chorus,
// "Daisy, Daisy, give me your answer do".  3/4 time, BPM ~120,
// quarter = 500 ms.  Famous for being the first computer-sung song
// (IBM 704, 1961) and HAL 9000's parting tune in 2001: A Space
// Odyssey.  Key of C major; ends on the tonic (C4) for closure
// even though the original phrase resolves later in the chorus.
const DBQ: u32 = 500; // quarter
const DBH: u32 = 1000; // half
const DBHD: u32 = 1500; // dotted half
pub const DAISY_BELL: &[Tone] = &[
    // "Dai - sy"  — high G drops to E
    Tone::new(Note::G4, DBH),
    Tone::new(Note::E4, DBQ),
    // "Dai - sy"
    Tone::new(Note::G4, DBH),
    Tone::new(Note::E4, DBQ),
    // "give me your"  — C, D, E walking up
    Tone::new(Note::C4, DBQ),
    Tone::new(Note::D4, DBQ),
    Tone::new(Note::E4, DBQ),
    // "an - swer  do"  — F (long) E D
    Tone::new(Note::F4, DBH),
    Tone::new(Note::E4, DBQ),
    Tone::new(Note::D4, DBQ),
    // Resolve to tonic C — not in the lyrics but lets the standalone
    // phrase end on a stable chord rather than the dominant D.
    Tone::new(Note::C4, DBHD),
];

// Nokia Tune — the unmistakable 13-note phrase from Francisco Tárrega's
// "Gran Vals" (1902) that became the Nokia ringtone in 1994.  Key of
// A major, 4/4 time, BPM ~150.  Pattern: two eighths + two quarters
// repeated three times, ending on a held A4.  ~4.4 s total.
const NQ: u32 = 400; // quarter at BPM 150
const NE: u32 = 200; // eighth
const NH: u32 = 800; // half (final held note)
pub const NOKIA: &[Tone] = &[
    // Bar 1: E D F♯ G♯
    Tone::new(Note::E5, NE),
    Tone::new(Note::D5, NE),
    Tone::new(Note::Fs4, NQ),
    Tone::new(Note::Gs4, NQ),
    // Bar 2: C♯ B D E
    Tone::new(Note::Cs5, NE),
    Tone::new(Note::B4, NE),
    Tone::new(Note::D4, NQ),
    Tone::new(Note::E4, NQ),
    // Bar 3: B A C♯ E
    Tone::new(Note::B4, NE),
    Tone::new(Note::A4, NE),
    Tone::new(Note::Cs4, NQ),
    Tone::new(Note::E4, NQ),
    // Bar 4: A (held tonic)
    Tone::new(Note::A4, NH),
];

// Samsung "Whistle" notification — short 5-note SMS chime.
// Pattern: A → C♯ → A (an octave up) → G♯ (held 2×) → E (held 4×).
// A major-ish with a chromatic G♯ passing tone before resolving down
// to E.  ~900 ms total.
pub const OVER_THE_HORIZON: &[Tone] = &[
    Tone::new(Note::A4, 100),
    Tone::new(Note::Cs5, 100),
    Tone::new(Note::A5, 100),
    Tone::new(Note::Gs5, 200),
    Tone::new(Note::E5, 400),
];

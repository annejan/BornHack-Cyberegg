//! LoRa preamble and duty-cycled receive timing — pure integer maths, built
//! on the host as well so its unit tests run there (`fw::mesh::sx1262` only
//! builds for the firmware).
//!
//! The SX1262's `SetRxDutyCycle` (opcode 0x94, datasheet §13.1.7) alternates
//! a short receive window with a sleep. A packet is only heard if one whole
//! window falls inside its preamble, so the sleep is derived from the
//! shortest preamble we expect peers to send.

/// Preamble length stock MeshCore programs for `sf`
/// (`RadioLibWrapper::preambleLengthForSF`, MeshCore 1.16.0+): 32 symbols at
/// SF8 and below, 16 above. Firmware before 1.16.0 sent 16 at every SF.
pub const fn preamble_length_for_sf(sf: u8) -> u16 {
    if sf <= 8 { 32 } else { 16 }
}

/// Length of each duty-cycle receive window at SF7 and above, in LoRa
/// symbols: how long the chip listens for a preamble before it sleeps again.
/// RadioLib's `startReceiveDutyCycleAuto` keeps the chip awake for at least
/// `minSymbols + 1` = 9 symbols (8 above SF6). Measured on the badge
/// (2026-11-26, SX1262, SF8 / 62.5 kHz): 3-symbol windows never raised
/// PreambleDetected at all; 9-symbol windows received every test packet.
pub const RX_WINDOW_SYMS: u64 = 9;

/// Receive window at SF5 / SF6, where RadioLib uses `minSymbols` = 12.
pub const RX_WINDOW_SYMS_LOW_SF: u64 = 13;

/// The duty-cycle receive window for `sf`, in symbols.
pub const fn rx_window_syms(sf: u8) -> u64 {
    if sf <= 6 { RX_WINDOW_SYMS_LOW_SF } else { RX_WINDOW_SYMS }
}

/// The share of the peer's preamble the window + sleep cycle may use, in
/// percent. The remaining 15 % absorbs the tolerance of the SX1262's 64 kHz
/// RC sleep timer and the ~1 ms context save / restore around each sleep
/// (datasheet §13.1.7, Figure 13-2).
pub const RX_DUTY_CLOCK_MARGIN_PCT: u64 = 85;

/// The shortest sleep worth arming, in symbols. Below this the save /
/// restore overhead eats the saving, so stay in continuous receive.
pub const MIN_SLEEP_SYMS: u64 = 2;

/// Context save + restore around one sleep, ~1 ms (datasheet §13.1.7: "around
/// 1ms to save the context, go into SLEEP mode, re-initialize the radio, lock
/// the PLL and go into RX"). Only used for the listen-before-talk window.
pub const WAKE_OVERHEAD_US: u64 = 1_000;

/// Largest value the 24-bit `rxPeriod` / `sleepPeriod` fields can carry.
const MAX_PERIOD_STEPS: u32 = 0x00FF_FFFF;

/// `SetRxDutyCycle` periods, in the chip's 15.625 µs steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "embassy-core", derive(defmt::Format))]
pub struct RxDutyPeriods {
    /// Receive window (`rxPeriod`).
    pub rx: u32,
    /// Sleep between windows (`sleepPeriod`).
    pub sleep: u32,
}

impl RxDutyPeriods {
    /// The 7-byte `SetRxDutyCycle` command: opcode 0x94, then `rxPeriod` and
    /// `sleepPeriod` as 24-bit big-endian values.
    pub fn command(&self) -> [u8; 7] {
        let rx = self.rx.to_be_bytes();
        let sleep = self.sleep.to_be_bytes();
        [0x94, rx[1], rx[2], rx[3], sleep[1], sleep[2], sleep[3]]
    }

    /// One full window + sleep cycle in µs, including the wake overhead.
    pub fn cycle_us(&self) -> u64 {
        steps_to_us(self.rx) + steps_to_us(self.sleep) + WAKE_OVERHEAD_US
    }

    /// How long to listen continuously before transmitting so that a
    /// preamble which began while we slept is still caught: one full cycle
    /// plus one more receive window.
    pub fn lbt_listen_us(&self) -> u64 {
        self.cycle_us() + steps_to_us(self.rx)
    }
}

/// LoRa symbol time in µs: `2^sf · 1e6 / bw_hz`.
pub fn symbol_us(sf: u8, bw_hz: u32) -> u64 {
    (1_000_000u64 << sf.clamp(5, 12)) / bw_hz.max(1) as u64
}

/// µs → 15.625 µs steps (`us · 64 / 1000`, rounded down).
pub const fn us_to_steps(us: u64) -> u64 {
    us * 64 / 1000
}

/// 15.625 µs steps → µs (rounded down).
pub const fn steps_to_us(steps: u32) -> u64 {
    steps as u64 * 1000 / 64
}

/// `SetRxDutyCycle` periods that still catch a `peer_preamble_syms`-symbol
/// preamble at `sf` / `bw_hz`, or `None` to stay in continuous receive.
///
/// ```text
/// tsym_us  = 2^sf · 1_000_000 / bw_hz
/// rx_us    = rx_window_syms(sf) · tsym_us
/// budget   = peer_preamble_syms · tsym_us · 85 / 100
/// sleep_us = budget − 2 · rx_us        (None if < MIN_SLEEP_SYMS symbols)
/// ```
///
/// The two `rx_us` in the sleep formula cover the worst case: the preamble
/// starts just too late for one window, so the whole of the next window must
/// still fall inside it. With 9-symbol windows a preamble needs more than
/// (2·9 + 2) / 0.85 ≈ 24 symbols before any sleep fits, so only the 32-symbol
/// preamble MeshCore 1.16+ sends at SF ≤ 8 is ever duty-cycled.
///
/// This sizes the *catch* side only. After a preamble is detected the chip
/// restarts its timer at `2·rx + sleep` and, by default, only stops it on the
/// header (§13.1.7: "Tpreamble + Theader ≤ 2 * rxPeriod + sleepPeriod"),
/// which these periods do not satisfy. The driver therefore also enables
/// `StopTimerOnPreamble` when it arms the duty cycle.
pub fn rx_duty_cycle_periods(sf: u8, bw_hz: u32, peer_preamble_syms: u16) -> Option<RxDutyPeriods> {
    if bw_hz == 0 {
        return None;
    }
    let tsym_us = symbol_us(sf, bw_hz);
    let rx_us = rx_window_syms(sf) * tsym_us;
    let budget_us = peer_preamble_syms as u64 * tsym_us * RX_DUTY_CLOCK_MARGIN_PCT / 100;
    let sleep_us = budget_us.checked_sub(2 * rx_us)?;
    if sleep_us < MIN_SLEEP_SYMS * tsym_us {
        return None;
    }
    let rx = u32::try_from(us_to_steps(rx_us)).ok()?;
    let sleep = u32::try_from(us_to_steps(sleep_us)).ok()?;
    if rx == 0 || rx > MAX_PERIOD_STEPS || sleep > MAX_PERIOD_STEPS {
        return None;
    }
    Some(RxDutyPeriods { rx, sleep })
}

/// Duty-cycle periods for receive power save at `sf` / `bw_hz`: sized for
/// the preamble stock MeshCore 1.16+ sends ([`preamble_length_for_sf`]), or
/// `None` to stay in continuous receive. Only SF ≤ 8 (32-symbol preambles)
/// leaves room to sleep; older peers sending 16 symbols may be missed there.
pub fn rx_power_save_periods(sf: u8, bw_hz: u32) -> Option<RxDutyPeriods> {
    rx_duty_cycle_periods(sf, bw_hz, preamble_length_for_sf(sf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(rx: u32, sleep: u32) -> Option<RxDutyPeriods> {
        Some(RxDutyPeriods { rx, sleep })
    }

    #[test]
    fn preamble_for_sf_matches_meshcore_1_16() {
        assert_eq!(preamble_length_for_sf(7), 32);
        assert_eq!(preamble_length_for_sf(8), 32);
        assert_eq!(preamble_length_for_sf(9), 16);
        assert_eq!(preamble_length_for_sf(12), 16);
    }

    #[test]
    fn window_follows_radiolib_min_symbols() {
        assert_eq!(rx_window_syms(5), 13);
        assert_eq!(rx_window_syms(6), 13);
        assert_eq!(rx_window_syms(7), 9);
        assert_eq!(rx_window_syms(12), 9);
    }

    #[test]
    fn nl_preset_sf7() {
        // tsym 2048 µs: rx 18 432 µs; sleep 55 705 − 36 864 µs.
        assert_eq!(rx_duty_cycle_periods(7, 62_500, 32), p(1179, 1205));
        assert_eq!(rx_duty_cycle_periods(7, 62_500, 16), None);
    }

    #[test]
    fn eu_uk_narrow_sf8() {
        // tsym 4096 µs: rx 36 864 µs; sleep 111 411 − 73 728 µs. This is the
        // timing verified on the badge.
        assert_eq!(rx_duty_cycle_periods(8, 62_500, 32), p(2359, 2411));
        assert_eq!(rx_duty_cycle_periods(8, 62_500, 16), None);
    }

    #[test]
    fn sf12_long_symbols_fit_24_bits() {
        // tsym 65 536 µs: rx 589 824 µs; sleep 1 782 579 − 1 179 648 µs.
        assert_eq!(rx_duty_cycle_periods(12, 62_500, 32), p(37_748, 38_587));
        // What stock MeshCore actually sends at SF12 is too short to sleep.
        assert_eq!(rx_duty_cycle_periods(12, 62_500, 16), None);
    }

    #[test]
    fn too_short_preamble_stays_continuous() {
        // 23 symbols at SF7: budget 40 038 µs − 36 864 µs < 2 symbols;
        // 24 symbols is the first that leaves room to sleep.
        assert_eq!(rx_duty_cycle_periods(7, 62_500, 23), None);
        assert!(rx_duty_cycle_periods(7, 62_500, 24).is_some());
        assert_eq!(rx_duty_cycle_periods(7, 62_500, 8), None);
        assert_eq!(rx_duty_cycle_periods(7, 0, 32), None);
    }

    #[test]
    fn power_save_uses_the_meshcore_1_16_preamble() {
        assert_eq!(rx_power_save_periods(7, 62_500), p(1179, 1205));
        assert_eq!(rx_power_save_periods(8, 62_500), p(2359, 2411));
        // Above SF8 stock MeshCore sends 16 symbols: too short to sleep.
        assert_eq!(rx_power_save_periods(9, 62_500), None);
        assert_eq!(rx_power_save_periods(12, 62_500), None);
    }

    #[test]
    fn command_bytes_are_big_endian_24_bit() {
        let c = RxDutyPeriods { rx: 0x01_02_03, sleep: 0x0A_0B_0C }.command();
        assert_eq!(c, [0x94, 0x01, 0x02, 0x03, 0x0A, 0x0B, 0x0C]);
    }

    #[test]
    fn lbt_listen_covers_a_cycle_and_a_window() {
        let d = RxDutyPeriods { rx: 2359, sleep: 2411 };
        // 36 859 + 37 671 + 1 000 overhead, then one more 36 859 µs window.
        assert_eq!(d.cycle_us(), 36_859 + 37_671 + 1_000);
        assert_eq!(d.lbt_listen_us(), d.cycle_us() + 36_859);
    }
}

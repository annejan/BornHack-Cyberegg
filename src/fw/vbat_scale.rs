// Battery SAADC scaling — single definition of the raw-to-millivolt formula.
//
// This file is `include!`-ed by `fw/battery.rs`, `fw/factory_test.rs` and
// `bin/hwtest.rs` (which cannot link the lib), so keep it a regular `//`
// comment and free of `use`s.  Each user needs it from a different place,
// hence the `dead_code` allow.

/// SAADC raw count (12-bit, gain 1/6 on 0.6 V ref = 3.6 V full scale, behind
/// the 1/3 battery divider) to battery millivolts:
/// `raw * 3600 * 3 / 4096`.
#[allow(dead_code)]
const fn vbat_raw_to_mv(raw: u32) -> u16 {
    ((raw * 10_800) / 4_096) as u16
}

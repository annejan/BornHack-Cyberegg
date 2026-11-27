// nRF52840 CLOCK peripheral — raw register addresses (PAC-free), base
// 0x4000_0000.
//
// Consts only.  This file is `include!`-ed by `fw/factory_test.rs` and by
// `bin/hwtest.rs` (which cannot link the lib), so keep it a regular `//`
// comment and free of `use`s.  Each user needs a different subset, hence
// the `dead_code` allows.

#[allow(dead_code)]
const CLOCK_TASKS_HFCLKSTART: *mut u32 = 0x4000_0000 as *mut u32;
#[allow(dead_code)]
const CLOCK_TASKS_HFCLKSTOP: *mut u32 = 0x4000_0004 as *mut u32;
#[allow(dead_code)]
const CLOCK_TASKS_LFCLKSTART: *mut u32 = 0x4000_0008 as *mut u32;
#[allow(dead_code)]
const CLOCK_TASKS_LFCLKSTOP: *mut u32 = 0x4000_000C as *mut u32;
#[allow(dead_code)]
const CLOCK_EVENTS_HFCLKSTARTED: *mut u32 = 0x4000_0100 as *mut u32;
#[allow(dead_code)]
const CLOCK_EVENTS_LFCLKSTARTED: *mut u32 = 0x4000_0104 as *mut u32;
#[allow(dead_code)]
const CLOCK_HFCLKSTAT: *const u32 = 0x4000_0408 as *const u32;
#[allow(dead_code)]
const CLOCK_LFCLKSTAT: *const u32 = 0x4000_0418 as *const u32;
#[allow(dead_code)]
const CLOCK_LFCLKSRC: *mut u32 = 0x4000_0518 as *mut u32;

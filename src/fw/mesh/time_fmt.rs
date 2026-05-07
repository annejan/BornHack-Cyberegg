//! Compact relative-time formatter shared by the Contacts and PM
//! screens.  Both produce the same `now / Nm / Nh / ydy / Nd / Nw`
//! string vocabulary; the only difference between the two callers
//! was the output buffer size, which we now standardise.

/// Format a delta-in-seconds as a short token (≤ 4 chars):
/// `now`, `3m`, `42m`, `5h`, `ydy`, `3d`, `2w`, `9w` (capped at 99w).
pub fn fmt_relative_secs(delta_secs: u64) -> heapless::String<8> {
    let mut s: heapless::String<8> = heapless::String::new();
    if delta_secs < 60 {
        let _ = s.push_str("now");
    } else if delta_secs < 60 * 60 {
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}m", delta_secs / 60));
    } else if delta_secs < 24 * 60 * 60 {
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}h", delta_secs / 3600));
    } else if delta_secs < 2 * 24 * 60 * 60 {
        let _ = s.push_str("ydy");
    } else if delta_secs < 14 * 24 * 60 * 60 {
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}d", delta_secs / 86400));
    } else {
        let weeks = (delta_secs / (7 * 86400)).min(99);
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{}w", weeks));
    }
    s
}

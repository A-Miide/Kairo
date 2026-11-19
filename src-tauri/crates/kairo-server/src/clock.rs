//! Local wall-clock times (stage timers end at a time of day).

use chrono::{Local, TimeZone};

/// Today at `h:m` local time, as epoch milliseconds.
pub fn today_at(h: u32, m: u32) -> Option<u64> {
    let d = Local::now().date_naive().and_hms_opt(h, m, 0)?;
    let t = Local.from_local_datetime(&d).earliest()?;
    u64::try_from(t.timestamp_millis()).ok()
}

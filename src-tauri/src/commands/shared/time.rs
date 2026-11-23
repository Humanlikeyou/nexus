//! Clocks and the local day.
//!
//! Taisun lives in Japan: UTC+9 with no daylight saving. The offset is defined once here; the day
//! key for any instant is derived by [`day_key`] and nowhere else (review §3.7).

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};

/// Japan Standard Time offset in seconds.
pub(crate) const JST_OFFSET_SECS: i32 = 9 * 3600;

/// Japan Standard Time.
pub(crate) fn jst() -> FixedOffset {
    FixedOffset::east_opt(JST_OFFSET_SECS).expect("valid offset")
}

/// Now, in JST.
pub(crate) fn now_jst() -> DateTime<FixedOffset> {
    Utc::now().with_timezone(&jst())
}

/// Milliseconds since the Unix epoch (0 if the clock is before 1970).
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Seconds since the Unix epoch (0 if the clock is before 1970).
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The local (JST) calendar day an instant falls on.
pub(crate) fn day_key<Tz: TimeZone>(t: &DateTime<Tz>) -> NaiveDate {
    t.with_timezone(&jst()).date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_key_rolls_over_at_jst_midnight_not_utc() {
        // 14:59:59Z is 23:59:59 JST; 15:00:00Z is the next JST day.
        let before = DateTime::parse_from_rfc3339("2026-11-13T14:59:59Z").unwrap();
        let after = DateTime::parse_from_rfc3339("2026-11-13T15:00:00Z").unwrap();
        assert_eq!(day_key(&before).to_string(), "2026-11-13");
        assert_eq!(day_key(&after).to_string(), "2026-11-14");
    }

    #[test]
    fn day_key_is_the_same_whatever_zone_the_instant_carries() {
        let in_jst = DateTime::parse_from_rfc3339("2026-11-14T08:00:00+09:00").unwrap();
        let in_utc = in_jst.with_timezone(&Utc);
        assert_eq!(day_key(&in_jst), day_key(&in_utc));
    }

    #[test]
    fn clocks_agree() {
        let s = now_secs();
        let ms = now_ms();
        assert!(ms / 1000 >= s && ms / 1000 <= s + 1);
        assert_eq!(jst().local_minus_utc(), 9 * 3600);
    }
}

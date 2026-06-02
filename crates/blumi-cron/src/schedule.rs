//! Schedule parsing + next-run computation (UTC).
//!
//! Supported forms (no external cron dependency):
//! - `every <dur>` — e.g. `every 30m`, `every 2h`, `every 90s`, `every 1d`
//! - `hourly` / `hourly:MM` — every hour at minute MM (default 0)
//! - `daily HH:MM` — every day at HH:MM (UTC)
//! - an RFC3339 timestamp — a one-shot run at that instant

use time::{Duration, OffsetDateTime};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Schedule {
    /// Repeat every N seconds.
    Every(i64),
    /// Every hour at the given minute.
    Hourly { minute: u8 },
    /// Every day at HH:MM (UTC).
    Daily { hour: u8, minute: u8 },
    /// A single run at a fixed instant.
    Once(OffsetDateTime),
}

#[derive(Debug, thiserror::Error)]
#[error("invalid schedule {input:?}: {reason}")]
pub struct ParseError {
    pub input: String,
    pub reason: String,
}

fn err(input: &str, reason: impl Into<String>) -> ParseError {
    ParseError {
        input: input.to_string(),
        reason: reason.into(),
    }
}

impl Schedule {
    pub fn parse(input: &str) -> Result<Schedule, ParseError> {
        let s = input.trim();
        let lower = s.to_lowercase();

        if let Some(rest) = lower.strip_prefix("every ") {
            let secs = parse_duration_secs(rest.trim()).map_err(|m| err(input, m))?;
            if secs <= 0 {
                return Err(err(input, "duration must be positive"));
            }
            return Ok(Schedule::Every(secs));
        }
        if lower == "hourly" {
            return Ok(Schedule::Hourly { minute: 0 });
        }
        if let Some(rest) = lower
            .strip_prefix("hourly:")
            .or(lower.strip_prefix("hourly "))
        {
            let minute = rest
                .trim()
                .parse::<u8>()
                .ok()
                .filter(|m| *m < 60)
                .ok_or_else(|| err(input, "minute must be 0-59"))?;
            return Ok(Schedule::Hourly { minute });
        }
        if let Some(rest) = lower
            .strip_prefix("daily ")
            .or(lower.strip_prefix("daily@"))
        {
            let (hour, minute) = parse_hhmm(rest.trim()).map_err(|m| err(input, m))?;
            return Ok(Schedule::Daily { hour, minute });
        }
        if let Ok(dt) = OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
            return Ok(Schedule::Once(dt));
        }
        Err(err(
            input,
            "expected 'every <dur>', 'hourly[:MM]', 'daily HH:MM', or an RFC3339 time",
        ))
    }

    /// A human description.
    pub fn describe(&self) -> String {
        match self {
            Schedule::Every(s) => format!("every {}", humanize_secs(*s)),
            Schedule::Hourly { minute } => format!("hourly at :{minute:02}"),
            Schedule::Daily { hour, minute } => format!("daily at {hour:02}:{minute:02} UTC"),
            Schedule::Once(t) => format!("once at {t}"),
        }
    }

    /// The next run strictly after `base`.
    pub fn next_after(&self, base: OffsetDateTime) -> Option<OffsetDateTime> {
        match self {
            Schedule::Every(secs) => Some(base + Duration::seconds(*secs)),
            Schedule::Hourly { minute } => Some(next_at(base, None, *minute)),
            Schedule::Daily { hour, minute } => Some(next_at(base, Some(*hour), *minute)),
            Schedule::Once(t) => (*t > base).then_some(*t),
        }
    }
}

/// Next time matching the given (optional) hour + minute, strictly after `base`.
fn next_at(base: OffsetDateTime, hour: Option<u8>, minute: u8) -> OffsetDateTime {
    let mut c = base
        .replace_minute(minute)
        .unwrap_or(base)
        .replace_second(0)
        .unwrap_or(base)
        .replace_nanosecond(0)
        .unwrap_or(base);
    let step = match hour {
        Some(h) => {
            c = c.replace_hour(h).unwrap_or(c);
            Duration::days(1)
        }
        None => Duration::hours(1),
    };
    while c <= base {
        c += step;
    }
    c
}

fn parse_duration_secs(s: &str) -> Result<i64, String> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len()));
    let n: i64 = num.parse().map_err(|_| format!("bad number in {s:?}"))?;
    let mult = match unit.trim() {
        "s" | "sec" | "secs" | "" => 1,
        "m" | "min" | "mins" => 60,
        "h" | "hr" | "hrs" => 3600,
        "d" | "day" | "days" => 86_400,
        other => return Err(format!("unknown unit {other:?} (use s/m/h/d)")),
    };
    Ok(n * mult)
}

fn parse_hhmm(s: &str) -> Result<(u8, u8), String> {
    let (h, m) = s.split_once(':').ok_or("expected HH:MM")?;
    let hour: u8 = h.trim().parse().map_err(|_| "bad hour")?;
    let minute: u8 = m.trim().parse().map_err(|_| "bad minute")?;
    if hour > 23 || minute > 59 {
        return Err("HH 0-23, MM 0-59".into());
    }
    Ok((hour, minute))
}

fn humanize_secs(s: i64) -> String {
    if s % 86_400 == 0 {
        format!("{}d", s / 86_400)
    } else if s % 3600 == 0 {
        format!("{}h", s / 3600)
    } else if s % 60 == 0 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn parses_forms() {
        assert_eq!(Schedule::parse("every 30m").unwrap(), Schedule::Every(1800));
        assert_eq!(Schedule::parse("every 2h").unwrap(), Schedule::Every(7200));
        assert_eq!(
            Schedule::parse("hourly:15").unwrap(),
            Schedule::Hourly { minute: 15 }
        );
        assert_eq!(
            Schedule::parse("daily 09:30").unwrap(),
            Schedule::Daily {
                hour: 9,
                minute: 30
            }
        );
        assert!(matches!(
            Schedule::parse("2030-01-01T00:00:00Z").unwrap(),
            Schedule::Once(_)
        ));
        assert!(Schedule::parse("nonsense").is_err());
    }

    #[test]
    fn every_advances_by_interval() {
        let base = datetime!(2025-01-01 12:00:00 UTC);
        let next = Schedule::Every(3600).next_after(base).unwrap();
        assert_eq!(next, datetime!(2025-01-01 13:00:00 UTC));
    }

    #[test]
    fn daily_finds_next_occurrence() {
        let base = datetime!(2025-01-01 10:00:00 UTC);
        let next = Schedule::Daily { hour: 9, minute: 0 }
            .next_after(base)
            .unwrap();
        // 09:00 today already passed → tomorrow 09:00.
        assert_eq!(next, datetime!(2025-01-02 09:00:00 UTC));

        let before = datetime!(2025-01-01 08:00:00 UTC);
        let next2 = Schedule::Daily { hour: 9, minute: 0 }
            .next_after(before)
            .unwrap();
        assert_eq!(next2, datetime!(2025-01-01 09:00:00 UTC));
    }

    #[test]
    fn once_is_one_shot() {
        let t = datetime!(2025-06-01 00:00:00 UTC);
        let before = datetime!(2025-05-31 23:00:00 UTC);
        let after = datetime!(2025-06-01 01:00:00 UTC);
        assert_eq!(Schedule::Once(t).next_after(before), Some(t));
        assert_eq!(Schedule::Once(t).next_after(after), None);
    }
}

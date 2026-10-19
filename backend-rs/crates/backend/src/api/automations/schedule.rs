//! Pure calendar calculations: callers supply UTC time, so tests and recovery
//! never depend on sleeping or the server machine's local timezone.

use std::str::FromStr;

use chrono::{DateTime, Duration, LocalResult, TimeZone, Utc};
use chrono_tz::Tz;
use croner::Cron;
use serde::{Deserialize, Serialize};

use crate::api::error::ApiError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    Once {
        at: String,
    },
    Cron {
        expression: String,
    },
    Webhook,
    /// GitHub's native delivery: `X-Hub-Signature-256` over the raw body and
    /// `X-GitHub-Delivery` as identity. `events` empty means all except `ping`.
    Github {
        #[serde(default)]
        events: Vec<String>,
    },
}

impl Trigger {
    pub fn github_events(&self) -> Option<&[String]> {
        match self {
            Self::Github { events } => Some(events),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MissedPolicy {
    #[default]
    CoalesceLatest,
    Skip,
}

pub struct Schedule {
    timezone: Tz,
    trigger: Parsed,
}

enum Parsed {
    Once(DateTime<Utc>),
    Cron(Box<Cron>),
    Webhook,
    Github,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DuePlan {
    pub run_at: Option<i64>,
    pub next_at: Option<i64>,
    /// The journal records the skipped range rather than expanding years of
    /// occurrences after a long outage.
    pub skipped_until: Option<i64>,
}

impl Schedule {
    pub fn parse(trigger: &Trigger, timezone: &str) -> Result<Self, ApiError> {
        let timezone = Tz::from_str(timezone)
            .map_err(|_| ApiError::invalid_input("timezone must be an IANA timezone"))?;
        let trigger = match trigger {
            Trigger::Once { at } => Parsed::Once(
                DateTime::parse_from_rfc3339(at)
                    .map_err(|_| ApiError::invalid_input("once.at must include an RFC3339 offset"))?
                    .with_timezone(&Utc),
            ),
            Trigger::Cron { expression } => {
                if expression.len() > 128 || expression.split_whitespace().count() != 5 {
                    return Err(ApiError::invalid_input("cron requires exactly five fields"));
                }
                Parsed::Cron(Box::new(
                    Cron::from_str(expression)
                        .map_err(|_| ApiError::invalid_input("invalid cron expression"))?,
                ))
            }
            Trigger::Webhook => Parsed::Webhook,
            Trigger::Github { events } => {
                if events.len() > 32 {
                    return Err(ApiError::invalid_input(
                        "GitHub event filter supports at most 32 names",
                    ));
                }
                for event in events {
                    let valid = !event.is_empty()
                        && event.len() <= 64
                        && event.chars().all(|character| {
                            character.is_ascii_alphanumeric()
                                || matches!(character, '_' | '-' | '.')
                        });
                    if !valid {
                        return Err(ApiError::invalid_input(
                            "GitHub event names must be 1–64 ASCII word characters",
                        ));
                    }
                }
                Parsed::Github
            }
        };
        Ok(Self { timezone, trigger })
    }

    pub fn next_after(&self, after: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, ApiError> {
        match &self.trigger {
            Parsed::Webhook | Parsed::Github => Ok(None),
            Parsed::Once(at) => Ok((*at > after).then_some(*at)),
            Parsed::Cron(cron) => {
                // Note: evaluate calendar fields without the cron library's DST
                // policy, then map through IANA explicitly. The product skips
                // gaps and chooses only the first instant of a repeated time.
                let local = after.with_timezone(&self.timezone).naive_local();
                let mut candidate = DateTime::<Utc>::from_naive_utc_and_offset(local, Utc);
                for _ in 0..1500 {
                    candidate = cron
                        .find_next_occurrence(&candidate, false)
                        .map_err(|_| ApiError::invalid_input("cron has no future occurrence"))?;
                    let actual = match self.timezone.from_local_datetime(&candidate.naive_utc()) {
                        LocalResult::None => continue,
                        LocalResult::Single(at) => at,
                        LocalResult::Ambiguous(first, second) => first.min(second),
                    }
                    .with_timezone(&Utc);
                    if actual > after {
                        return Ok(Some(actual));
                    }
                }
                Err(ApiError::invalid_input(
                    "timezone transition exceeds scheduling bound",
                ))
            }
        }
    }

    pub fn preview(&self, after: DateTime<Utc>) -> Result<Vec<String>, ApiError> {
        let mut current = after;
        let mut result = Vec::new();
        for _ in 0..5 {
            let Some(next) = self.next_after(current)? else {
                break;
            };
            result.push(next.with_timezone(&self.timezone).to_rfc3339());
            current = next;
        }
        Ok(result)
    }

    pub fn due(
        &self,
        scheduled: i64,
        now: DateTime<Utc>,
        policy: MissedPolicy,
        recovering: bool,
    ) -> Result<DuePlan, ApiError> {
        if scheduled > now.timestamp() {
            return Ok(DuePlan {
                run_at: None,
                next_at: Some(scheduled),
                skipped_until: None,
            });
        }
        let next_at = self.next_after(now)?.map(|at| at.timestamp());
        let missed = recovering || now.timestamp() - scheduled > 60;
        if missed && policy == MissedPolicy::Skip {
            return Ok(DuePlan {
                run_at: None,
                next_at,
                skipped_until: Some(now.timestamp()),
            });
        }
        let cutoff = now - Duration::hours(24);
        let first = scheduled.max(cutoff.timestamp());
        let from = DateTime::from_timestamp(first, 0)
            .ok_or_else(|| ApiError::invalid_input("invalid scheduled timestamp"))?
            - Duration::nanoseconds(1);
        let mut cursor = from;
        let mut latest = None;
        for _ in 0..=1440 {
            let Some(candidate) = self.next_after(cursor)? else {
                break;
            };
            if candidate > now {
                break;
            }
            latest = Some(candidate.timestamp());
            cursor = candidate;
        }
        Ok(DuePlan {
            run_at: latest,
            next_at,
            skipped_until: if latest == Some(scheduled) {
                None
            } else {
                Some(latest.map(|at| at - 1).unwrap_or(now.timestamp()))
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }
    fn cron(expression: &str, timezone: &str) -> Schedule {
        Schedule::parse(
            &Trigger::Cron {
                expression: expression.into(),
            },
            timezone,
        )
        .unwrap()
    }

    #[test]
    fn preview_uses_iana_timezone_and_returns_five_runs() {
        let result = cron("0 9 * * MON-FRI", "Asia/Shanghai")
            .preview(utc("2026-10-16T02:00:00Z"))
            .unwrap();
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], "2026-10-19T09:00:00+08:00");
        assert_eq!(result[4], "2026-10-23T09:00:00+08:00");
    }
    #[test]
    fn rejects_extra_fields_invalid_timezone_and_offsetless_once() {
        assert!(Schedule::parse(
            &Trigger::Cron {
                expression: "0 * * * * *".into()
            },
            "UTC"
        )
        .is_err());
        assert!(Schedule::parse(&Trigger::Webhook, "Beijing").is_err());
        assert!(Schedule::parse(
            &Trigger::Once {
                at: "2026-10-20T08:00:00".into()
            },
            "UTC"
        )
        .is_err());
    }
    #[test]
    fn daylight_gap_is_skipped_instead_of_shifted() {
        let result = cron("30 2 * * *", "America/New_York")
            .preview(utc("2026-03-08T00:00:00Z"))
            .unwrap();
        assert_eq!(result[0], "2026-03-09T02:30:00-04:00");
    }
    #[test]
    fn repeated_local_time_runs_only_at_its_first_instant() {
        let schedule = cron("30 1 * * *", "America/New_York");
        assert_eq!(
            schedule.next_after(utc("2026-11-01T05:00:00Z")).unwrap(),
            Some(utc("2026-11-01T05:30:00Z"))
        );
        assert_eq!(
            schedule.next_after(utc("2026-11-01T05:30:00Z")).unwrap(),
            Some(utc("2026-11-02T06:30:00Z"))
        );
        assert_eq!(
            schedule.next_after(utc("2026-11-01T06:10:00Z")).unwrap(),
            Some(utc("2026-11-02T06:30:00Z"))
        );
    }
    #[test]
    fn recovery_in_second_fold_keeps_latest_actual_occurrence() {
        let schedule = cron("* * * * *", "America/New_York");
        let now = utc("2026-11-01T06:10:00Z");
        let plan = schedule
            .due(
                utc("2026-11-01T04:00:00Z").timestamp(),
                now,
                MissedPolicy::CoalesceLatest,
                true,
            )
            .unwrap();
        assert_eq!(plan.run_at, Some(utc("2026-11-01T05:59:00Z").timestamp()));
        assert_eq!(plan.next_at, Some(utc("2026-11-01T07:00:00Z").timestamp()));
    }
    #[test]
    fn long_outage_coalesces_latest_day_without_unbounded_history() {
        let now = utc("2026-10-18T12:34:10Z");
        let plan = cron("* * * * *", "UTC")
            .due(
                utc("2020-01-01T00:00:00Z").timestamp(),
                now,
                MissedPolicy::CoalesceLatest,
                true,
            )
            .unwrap();
        assert_eq!(plan.run_at, Some(utc("2026-10-18T12:34:00Z").timestamp()));
        assert_eq!(plan.next_at, Some(utc("2026-10-18T12:35:00Z").timestamp()));
        assert!(plan.skipped_until.is_some());
    }
    #[test]
    fn skip_policy_does_not_skip_regular_timer_ticks() {
        let schedule = cron("* * * * *", "UTC");
        let now = utc("2026-10-18T12:00:03Z");
        let due = utc("2026-10-18T12:00:00Z").timestamp();
        assert_eq!(
            schedule
                .due(due, now, MissedPolicy::Skip, false)
                .unwrap()
                .run_at,
            Some(due)
        );
        assert_eq!(
            schedule
                .due(due, now, MissedPolicy::Skip, true)
                .unwrap()
                .run_at,
            None
        );
    }
    #[test]
    fn github_filter_accepts_event_names_and_never_has_a_calendar_run() {
        let schedule = Schedule::parse(
            &Trigger::Github {
                events: vec!["push".into(), "pull_request".into()],
            },
            "UTC",
        )
        .unwrap();
        assert_eq!(
            schedule.next_after(utc("2026-10-18T00:00:00Z")).unwrap(),
            None
        );
        assert!(Schedule::parse(
            &Trigger::Github {
                events: vec!["not an event".into()]
            },
            "UTC"
        )
        .is_err());
        assert!(Schedule::parse(
            &Trigger::Github {
                events: (0..33).map(|index| format!("event_{index}")).collect()
            },
            "UTC"
        )
        .is_err());
    }

    #[test]
    fn once_outside_catchup_window_is_skipped_and_never_repeats() {
        let schedule = Schedule::parse(
            &Trigger::Once {
                at: "2026-10-15T09:00:00+08:00".into(),
            },
            "Asia/Shanghai",
        )
        .unwrap();
        let plan = schedule
            .due(
                utc("2026-10-15T01:00:00Z").timestamp(),
                utc("2026-10-18T01:00:00Z"),
                MissedPolicy::CoalesceLatest,
                true,
            )
            .unwrap();
        assert_eq!(plan.run_at, None);
        assert_eq!(plan.next_at, None);
        assert!(plan.skipped_until.is_some());
    }
}

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, TimeZone, Timelike};

use super::{date, relative, time, weekday};

#[derive(Debug, Default)]
pub struct DateTimeBuilder {
    pub(super) base: Option<DateTime<FixedOffset>>,
    pub(super) timestamp: Option<i32>,
    pub(super) date: Option<date::Date>,
    pub(super) time: Option<time::Time>,
    pub(super) weekday: Option<weekday::Weekday>,
    pub(super) timezone: Option<time::Offset>,
    pub(super) relative: Vec<relative::Relative>,
}

impl DateTimeBuilder {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn set_base(mut self, base: DateTime<FixedOffset>) -> Self {
        self.base = Some(base);
        self
    }

    pub(super) fn set_timestamp(mut self, ts: i32) -> Self {
        self.timestamp = Some(ts);
        self
    }

    pub(super) fn set_year(mut self, year: u32) -> Self {
        if let Some(date) = self.date.as_mut() {
            date.year = Some(year);
        } else {
            self.date = Some(date::Date {
                day: 1,
                month: 1,
                year: Some(year),
            });
        }
        self
    }

    pub(super) fn set_date(mut self, date: date::Date) -> Self {
        self.date = Some(date);
        self
    }

    pub(super) fn set_time(mut self, time: time::Time) -> Self {
        self.time = Some(time);
        self
    }

    pub(super) fn set_weekday(mut self, weekday: weekday::Weekday) -> Self {
        self.weekday = Some(weekday);
        self
    }

    pub(super) fn set_timezone(mut self, timezone: time::Offset) -> Self {
        self.timezone = Some(timezone);
        self
    }

    pub(super) fn set_relative(mut self, relative: relative::Relative) -> Self {
        self.relative.push(relative);
        self
    }

    pub(super) fn build(self) -> Option<DateTime<FixedOffset>> {
        let base = self.base.unwrap_or_else(|| chrono::Local::now().into());
        let mut dt = new_date(
            base.year(),
            base.month(),
            base.day(),
            0,
            0,
            0,
            0,
            *base.offset(),
        )?;

        if let Some(ts) = self.timestamp {
            dt = chrono::Utc
                .timestamp_opt(ts.into(), 0)
                .unwrap()
                .with_timezone(&dt.timezone());
        }

        if let Some(date::Date { year, month, day }) = self.date {
            dt = new_date(
                year.map(|x| x as i32).unwrap_or(dt.year()),
                month,
                day,
                dt.hour(),
                dt.minute(),
                dt.second(),
                dt.nanosecond(),
                *dt.offset(),
            )?;
        }

        if let Some(time::Time {
            hour,
            minute,
            second,
            ref offset,
        }) = self.time
        {
            let offset = offset
                .clone()
                .and_then(|o| chrono::FixedOffset::try_from(o).ok())
                .unwrap_or(*dt.offset());

            dt = new_date(
                dt.year(),
                dt.month(),
                dt.day(),
                hour,
                minute,
                second as u32,
                (second.fract() * 10f64.powi(9)).round() as u32,
                offset,
            )?;
        }

        if let Some(weekday::Weekday { offset, day }) = self.weekday {
            if self.time.is_none() {
                dt = dt
                    .with_hour(0)?
                    .with_minute(0)?
                    .with_second(0)?
                    .with_nanosecond(0)?;
            }

            let mut offset = offset;
            let day = day.into();

            // If the current day is not the target day, we need to adjust
            // the x value to ensure we find the correct day.
            //
            // Consider this:
            // Assuming today is Monday, next Friday is actually THIS Friday;
            // but next Monday is indeed NEXT Monday.
            if dt.weekday() != day && offset > 0 {
                offset -= 1;
            }

            // Calculate the delta to the target day.
            //
            // Assuming today is Thursday, here are some examples:
            //
            // Example 1: last Thursday (x = -1, day = Thursday)
            //            delta = (3 - 3) % 7 + (-1) * 7 = -7
            //
            // Example 2: last Monday (x = -1, day = Monday)
            //            delta = (0 - 3) % 7 + (-1) * 7 = -3
            //
            // Example 3: next Monday (x = 1, day = Monday)
            //            delta = (0 - 3) % 7 + (0) * 7 = 4
            // (Note that we have adjusted the x value above)
            //
            // Example 4: next Thursday (x = 1, day = Thursday)
            //            delta = (3 - 3) % 7 + (1) * 7 = 7
            let delta = (day.num_days_from_monday() as i32
                - dt.weekday().num_days_from_monday() as i32)
                .rem_euclid(7)
                + offset.checked_mul(7)?;

            dt = if delta < 0 {
                dt.checked_sub_days(chrono::Days::new((-delta) as u64))?
            } else {
                dt.checked_add_days(chrono::Days::new(delta as u64))?
            }
        }

        for rel in self.relative {
            if self.timestamp.is_none()
                && self.date.is_none()
                && self.time.is_none()
                && self.weekday.is_none()
            {
                dt = base;
            }

            match rel {
                relative::Relative::Years(x) => {
                    dt = dt.with_year(dt.year() + x)?;
                }
                relative::Relative::Months(x) => {
                    // *NOTE* This is done in this way to conform to
                    // GNU behavior.
                    let days = last_day_of_month(dt.year(), dt.month());
                    if x >= 0 {
                        dt += dt
                            .date_naive()
                            .checked_add_days(chrono::Days::new((days * x as u32) as u64))?
                            .signed_duration_since(dt.date_naive());
                    } else {
                        dt += dt
                            .date_naive()
                            .checked_sub_days(chrono::Days::new((days * -x as u32) as u64))?
                            .signed_duration_since(dt.date_naive());
                    }
                }
                relative::Relative::Days(x) => dt += chrono::Duration::days(x.into()),
                relative::Relative::Hours(x) => dt += chrono::Duration::hours(x.into()),
                relative::Relative::Minutes(x) => {
                    dt += chrono::Duration::try_minutes(x.into())?;
                }
                // Seconds are special because they can be given as a float
                relative::Relative::Seconds(x) => {
                    dt += chrono::Duration::try_seconds(x as i64)?;
                }
            }
        }

        if let Some(offset) = self.timezone {
            dt = with_timezone_restore(offset, dt)?;
        }

        Some(dt)
    }
}

#[allow(clippy::too_many_arguments)]
fn new_date(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nano: u32,
    offset: FixedOffset,
) -> Option<DateTime<FixedOffset>> {
    let newdate = NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|naive| naive.and_hms_nano_opt(hour, minute, second, nano))?;

    Some(DateTime::<FixedOffset>::from_local(newdate, offset))
}

/// Restores year, month, day, etc after applying the timezone
/// returns None if timezone overflows the date
fn with_timezone_restore(
    offset: time::Offset,
    at: DateTime<FixedOffset>,
) -> Option<DateTime<FixedOffset>> {
    let offset: FixedOffset = chrono::FixedOffset::try_from(offset).ok()?;
    let copy = at;
    let x = at
        .with_timezone(&offset)
        .with_day(copy.day())?
        .with_month(copy.month())?
        .with_year(copy.year())?
        .with_hour(copy.hour())?
        .with_minute(copy.minute())?
        .with_second(copy.second())?
        .with_nanosecond(copy.nanosecond())?;
    Some(x)
}

fn last_day_of_month(year: i32, month: u32) -> u32 {
    NaiveDate::from_ymd_opt(year, month + 1, 1)
        .unwrap_or(NaiveDate::from_ymd_opt(year + 1, 1, 1).unwrap())
        .pred_opt()
        .unwrap()
        .day()
}

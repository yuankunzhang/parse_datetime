// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore multispace0

//! From the GNU docs:
//!
//! > A date is a string, possibly empty, containing many items separated by
//! > whitespace. The whitespace may be omitted when no ambiguity arises. The
//! > empty string means the beginning of today (i.e., midnight). Order of the
//! > items is immaterial. A date string may contain many flavors of items:
//! >  - calendar date items
//! >  - time of day items
//! >  - time zone items
//! >  - combined date and time of day items
//! >  - day of the week items
//! >  - relative items
//! >  - pure numbers.
//!
//! We put all of those in separate modules:
//!  - [`date`]
//!  - [`time`]
//!  - [`time_zone`]
//!  - [`combined`]
//!  - [`weekday`]
//!  - [`relative`]
//!  - [`number]

#![allow(deprecated)]
mod combined;
mod date;
mod ordinal;
mod primitive;
mod relative;
mod time;
mod weekday;

mod epoch {
    use winnow::{combinator::preceded, ModalResult, Parser};

    use super::primitive::{dec_int, s};

    pub fn parse(input: &mut &str) -> ModalResult<i32> {
        s(preceded("@", dec_int)).parse_next(input)
    }
}

mod timezone {
    use winnow::ModalResult;

    use super::time;

    pub(crate) fn parse(input: &mut &str) -> ModalResult<time::Offset> {
        time::timezone(input)
    }
}

use chrono::NaiveDate;
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike};

use primitive::space;
use winnow::{
    combinator::{alt, trace},
    error::{AddContext, ContextError, ErrMode, StrContext, StrContextValue},
    stream::Stream,
    ModalResult, Parser,
};

use crate::ParseDateTimeError;

#[derive(Debug, Default)]
pub struct DateTimeBuilder {
    base: Option<DateTime<FixedOffset>>,
    timestamp: Option<i32>,
    date: Option<date::Date>,
    time: Option<time::Time>,
    weekday: Option<weekday::Weekday>,
    timezone: Option<time::Offset>,
    relative: Vec<relative::Relative>,
}

impl DateTimeBuilder {
    fn new() -> Self {
        Self::default()
    }

    fn set_base(mut self, base: DateTime<FixedOffset>) -> Self {
        self.base = Some(base);
        self
    }

    fn set_timestamp(mut self, ts: i32) -> Self {
        self.timestamp = Some(ts);
        self
    }

    fn set_year(mut self, year: u32) -> Self {
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

    fn set_date(mut self, date: date::Date) -> Self {
        self.date = Some(date);
        self
    }

    fn set_time(mut self, time: time::Time) -> Self {
        self.time = Some(time);
        self
    }

    fn set_weekday(mut self, weekday: weekday::Weekday) -> Self {
        self.weekday = Some(weekday);
        self
    }

    fn set_timezone(mut self, timezone: time::Offset) -> Self {
        self.timezone = Some(timezone);
        self
    }

    fn set_relative(mut self, relative: relative::Relative) -> Self {
        self.relative.push(relative);
        self
    }

    fn build(self) -> Option<DateTime<FixedOffset>> {
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

#[derive(PartialEq, Debug)]
pub enum Item {
    Timestamp(i32),
    Year(u32),
    DateTime(combined::DateTime),
    Date(date::Date),
    Time(time::Time),
    Weekday(weekday::Weekday),
    Relative(relative::Relative),
    TimeZone(time::Offset),
}

// Parse an item
pub fn parse_one(input: &mut &str) -> ModalResult<Item> {
    trace(
        "parse_one",
        alt((
            combined::parse.map(Item::DateTime),
            date::parse.map(Item::Date),
            time::parse.map(Item::Time),
            relative::parse.map(Item::Relative),
            weekday::parse.map(Item::Weekday),
            epoch::parse.map(Item::Timestamp),
            timezone::parse.map(Item::TimeZone),
            date::year.map(Item::Year),
        )),
    )
    .parse_next(input)
}

fn expect_error(input: &mut &str, reason: &'static str) -> ErrMode<ContextError> {
    ErrMode::Cut(ContextError::new()).add_context(
        input,
        &input.checkpoint(),
        StrContext::Expected(StrContextValue::Description(reason)),
    )
}

pub fn parse(input: &mut &str) -> ModalResult<DateTimeBuilder> {
    let mut builder = DateTimeBuilder::new();

    loop {
        match parse_one.parse_next(input) {
            Ok(item) => match item {
                Item::Timestamp(ts) => {
                    if builder.timestamp.is_some() {
                        return Err(expect_error(
                            input,
                            "timestamp cannot appear more than once",
                        ));
                    }
                    builder = builder.set_timestamp(ts);
                }
                Item::Year(year) => {
                    if builder.date.as_ref().and_then(|d| d.year).is_some() {
                        return Err(expect_error(input, "year cannot appear more than once"));
                    }
                    builder = builder.set_year(year);
                }
                Item::DateTime(dt) => {
                    if builder.date.is_some() || builder.time.is_some() {
                        return Err(expect_error(
                            input,
                            "date or time cannot appear more than once",
                        ));
                    }
                    builder = builder.set_date(dt.date).set_time(dt.time);
                }
                Item::Date(d) => {
                    if builder.date.is_some() {
                        return Err(expect_error(input, "date cannot appear more than once"));
                    }
                    builder = builder.set_date(d);
                }
                Item::Time(t) => {
                    if builder.time.is_some() {
                        return Err(expect_error(input, "time cannot appear more than once"));
                    }
                    if builder.timezone.is_some() && t.offset.is_some() {
                        return Err(expect_error(input, "timezone cannot appear more than once"));
                    }
                    builder = builder.set_time(t);
                }
                Item::Weekday(weekday) => {
                    if builder.weekday.is_some() {
                        return Err(expect_error(input, "weekday cannot appear more than once"));
                    }
                    builder = builder.set_weekday(weekday);
                }
                Item::TimeZone(tz) => {
                    if builder.timezone.is_some()
                        || (builder
                            .time
                            .as_ref()
                            .and_then(|t| t.offset.as_ref())
                            .is_some())
                    {
                        return Err(expect_error(input, "timezone cannot appear more than once"));
                    }
                    builder = builder.set_timezone(tz);
                }
                Item::Relative(rel) => {
                    builder = builder.set_relative(rel);
                }
            },
            Err(ErrMode::Backtrack(_)) => break,
            Err(e) => return Err(e),
        }
    }

    space.parse_next(input)?;
    if !input.is_empty() {
        return Err(expect_error(input, "unexpected input"));
    }

    Ok(builder)
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

pub(crate) fn at_date(
    builder: DateTimeBuilder,
    base: DateTime<FixedOffset>,
) -> Result<DateTime<FixedOffset>, ParseDateTimeError> {
    builder
        .set_base(base)
        .build()
        .ok_or(ParseDateTimeError::InvalidInput)
}

pub(crate) fn at_local(
    builder: DateTimeBuilder,
) -> Result<DateTime<FixedOffset>, ParseDateTimeError> {
    builder.build().ok_or(ParseDateTimeError::InvalidInput)
}

#[cfg(test)]
mod tests {
    use super::{at_date, parse, DateTimeBuilder};
    use chrono::{
        DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Timelike, Utc,
    };

    fn at_utc(builder: DateTimeBuilder) -> DateTime<FixedOffset> {
        at_date(builder, Utc::now().fixed_offset()).unwrap()
    }

    fn test_eq_fmt(fmt: &str, input: &str) -> String {
        let input = input.to_ascii_lowercase();
        parse(&mut input.as_str())
            .map(at_utc)
            .map_err(|e| eprintln!("TEST FAILED AT:\n{e}"))
            .expect("parsing failed during tests")
            .format(fmt)
            .to_string()
    }

    #[test]
    fn date_and_time() {
        // assert_eq!(
        //     parse(&mut "   10:10   2022-12-12    "),
        //     Ok(vec![
        //         Item::Time(Time {
        //             hour: 10,
        //             minute: 10,
        //             second: 0.0,
        //             offset: None,
        //         }),
        //         Item::Date(Date {
        //             day: 12,
        //             month: 12,
        //             year: Some(2022)
        //         })
        //     ])
        // );

        //               format,  expected output, input
        assert_eq!("2024-01-02", test_eq_fmt("%Y-%m-%d", "2024-01-02"));

        // https://github.com/uutils/coreutils/issues/6662
        assert_eq!("2005-01-02", test_eq_fmt("%Y-%m-%d", "2005-01-01 +1 day"));

        // https://github.com/uutils/coreutils/issues/6644
        assert_eq!("Jul 16", test_eq_fmt("%b %d", "Jul 16"));
        assert_eq!("0718061449", test_eq_fmt("%m%d%H%M%S", "Jul 18 06:14:49"));
        assert_eq!(
            "07182024061449",
            test_eq_fmt("%m%d%Y%H%M%S", "Jul 18, 2024 06:14:49")
        );
        assert_eq!(
            "07182024061449",
            test_eq_fmt("%m%d%Y%H%M%S", "Jul 18 06:14:49 2024")
        );

        // https://github.com/uutils/coreutils/issues/5177
        assert_eq!(
            "2023-07-27T13:53:54+00:00",
            test_eq_fmt("%Y-%m-%dT%H:%M:%S%:z", "@1690466034")
        );

        // https://github.com/uutils/coreutils/issues/6398
        // TODO: make this work
        // assert_eq!("1111 1111 00", test_eq_fmt("%m%d %H%M %S", "11111111"));

        assert_eq!(
            "2024-07-17 06:14:49 +00:00",
            test_eq_fmt("%Y-%m-%d %H:%M:%S %:z", "Jul 17 06:14:49 2024 GMT"),
        );

        assert_eq!(
            "2024-07-17 06:14:49.567 +00:00",
            test_eq_fmt("%Y-%m-%d %H:%M:%S%.f %:z", "Jul 17 06:14:49.567 2024 GMT"),
        );

        assert_eq!(
            "2024-07-17 06:14:49.567 +00:00",
            test_eq_fmt("%Y-%m-%d %H:%M:%S%.f %:z", "Jul 17 06:14:49,567 2024 GMT"),
        );

        assert_eq!(
            "2024-07-17 06:14:49 -03:00",
            test_eq_fmt("%Y-%m-%d %H:%M:%S %:z", "Jul 17 06:14:49 2024 BRT"),
        );
    }

    #[test]
    fn invalid() {
        let result = parse(&mut "2025-05-19 2024-05-20 06:14:49");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("date or time cannot appear more than once"));

        let result = parse(&mut "2025-05-19 2024-05-20");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("date cannot appear more than once"));

        let result = parse(&mut "06:14:49 06:14:49");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("time cannot appear more than once"));

        let result = parse(&mut "2025-05-19 2024");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("year cannot appear more than once"));

        let result = parse(&mut "2025-05-19 +00:00 +01:00");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("timezone cannot appear more than once"));

        let result = parse(&mut "m1y");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("timezone cannot appear more than once"));

        let result = parse(&mut "2025-05-19 abcdef");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unexpected input"));
    }

    #[test]
    fn relative_weekday() {
        // Jan 1 2025 is a Wed
        let now = Utc
            .from_utc_datetime(&NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
                NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            ))
            .fixed_offset();

        assert_eq!(
            at_date(parse(&mut "last wed").unwrap(), now).unwrap(),
            now - chrono::Duration::days(7)
        );
        assert_eq!(at_date(parse(&mut "this wed").unwrap(), now).unwrap(), now);
        assert_eq!(
            at_date(parse(&mut "next wed").unwrap(), now).unwrap(),
            now + chrono::Duration::days(7)
        );
        assert_eq!(
            at_date(parse(&mut "last thu").unwrap(), now).unwrap(),
            now - chrono::Duration::days(6)
        );
        assert_eq!(
            at_date(parse(&mut "this thu").unwrap(), now).unwrap(),
            now + chrono::Duration::days(1)
        );
        assert_eq!(
            at_date(parse(&mut "next thu").unwrap(), now).unwrap(),
            now + chrono::Duration::days(1)
        );
        assert_eq!(
            at_date(parse(&mut "1 wed").unwrap(), now).unwrap(),
            now + chrono::Duration::days(7)
        );
        assert_eq!(
            at_date(parse(&mut "1 thu").unwrap(), now).unwrap(),
            now + chrono::Duration::days(1)
        );
        assert_eq!(
            at_date(parse(&mut "2 wed").unwrap(), now).unwrap(),
            now + chrono::Duration::days(14)
        );
        assert_eq!(
            at_date(parse(&mut "2 thu").unwrap(), now).unwrap(),
            now + chrono::Duration::days(8)
        );
    }

    #[test]
    fn relative_date_time() {
        let now = Utc::now().fixed_offset();

        let result = at_date(parse(&mut "2 days ago").unwrap(), now).unwrap();
        assert_eq!(result, now - chrono::Duration::days(2));
        assert_eq!(result.hour(), now.hour());
        assert_eq!(result.minute(), now.minute());
        assert_eq!(result.second(), now.second());

        let result = at_date(parse(&mut "2025-01-01 2 days ago").unwrap(), now).unwrap();
        assert_eq!(result.hour(), 0);
        assert_eq!(result.minute(), 0);
        assert_eq!(result.second(), 0);

        let result = at_date(parse(&mut "3 weeks").unwrap(), now).unwrap();
        assert_eq!(result, now + chrono::Duration::days(21));
        assert_eq!(result.hour(), now.hour());
        assert_eq!(result.minute(), now.minute());
        assert_eq!(result.second(), now.second());

        let result = at_date(parse(&mut "2025-01-01 3 weeks").unwrap(), now).unwrap();
        assert_eq!(result.hour(), 0);
        assert_eq!(result.minute(), 0);
        assert_eq!(result.second(), 0);
    }
}

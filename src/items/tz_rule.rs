// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

//! From the GNU docs:
//!
//! > Normally, dates are interpreted using the rules of the current time zone,
//! > which in turn are specified by the ‘TZ’ environment variable, or by a
//! > system default if ‘TZ’ is not set. To specify a different set of default
//! > time zone rules that apply just to one date, start the date with a string
//! > of the form ‘TZ="RULE"’. The two quote characters (‘"’) must be present in
//! > the date, and any quotes or backslashes within RULE must be escaped by a
//! > backslash.
//! >
//! > A ‘TZ’ value is a rule that typically names a location in the ‘tz’ database
//! > (https://www.iana.org/time-zones). A recent catalog of location names
//! > appears in the TWiki Date and Time Gateway
//! > (https://twiki.org/cgi-bin/xtra/tzdatepick.html). A few non-GNU hosts
//! > require a colon before a location name in a ‘TZ’ setting, e.g.,
//! > ‘TZ=":America/New_York"’.

use jiff::tz::{Offset, TimeZone};
use winnow::{
    combinator::{alt, delimited, opt, preceded},
    stream::AsChar,
    token::take_while,
    ModalResult, Parser,
};

use super::primitive::{dec_uint, escaped_string, plus_or_minus};

pub(super) fn parse(input: &mut &str) -> ModalResult<TimeZone> {
    preceded("TZ=", delimited('"', alt((proleptic, geographical)), '"')).parse_next(input)
}

/// Parse forms like `America/New_York` or `Etc/UTC`.
fn geographical(input: &mut &str) -> ModalResult<TimeZone> {
    preceded(opt(':'), escaped_string)
        .map(|s| TimeZone::get(&s).unwrap_or(TimeZone::UTC))
        .parse_next(input)
}

/// Parse a proleptic timezone specification.
///
/// TODO: This implementation is incomplete. It currently only parses the
/// `STDOFFSET` part of the specification.
///
/// From the GNU docs:
///
/// > The proleptic format is:
/// >
/// >   STDOFFSET[DST[OFFSET][,START[/TIME],END[/TIME]]]
/// >
/// > The STD string specifies the time zone abbreviation, which must be at
/// > least three bytes long. ...
/// >
/// > The OFFSET specifies the time value you must add to the local time to
/// > get a UTC value.  It has syntax like:
/// >
/// >   [+|-]HH[:MM[:SS]]
fn proleptic(input: &mut &str) -> ModalResult<TimeZone> {
    (proleptic_std, opt(proleptic_offset))
        .verify_map(|(tz, offset)| {
            let base = tz.to_fixed_offset().ok()?.seconds();
            let offset = offset.unwrap_or(0);
            Some(Offset::from_seconds(base + offset).ok()?.to_time_zone())
        })
        .parse_next(input)
}

fn proleptic_std(input: &mut &str) -> ModalResult<TimeZone> {
    // GNU quirk: all names are treated as UTC.
    take_while(3.., AsChar::is_alpha)
        .map(|_| TimeZone::UTC)
        .parse_next(input)
}

fn proleptic_offset(input: &mut &str) -> ModalResult<i32> {
    let uint = dec_uint::<u32, _>;

    (
        opt(plus_or_minus),
        alt((
            (uint, preceded(':', uint), preceded(':', uint)).map(|(h, m, s)| (h, m, s)),
            (uint, preceded(':', uint)).map(|(h, m)| (h, m, 0)),
            uint.map(|h| (h, 0, 0)),
        )),
    )
        .map(|(sign, (h, m, s))| {
            let sign = if sign == Some('-') { -1 } else { 1 };

            // GNU quirks:
            // - If hour is greater than 24, clamp it to 24.
            // - If minute is greater than 59, clamp it to 59.
            // - If second is greater than 59, clamp it to 59.
            let h = h.min(24) as i32;
            let m = m.min(59) as i32;
            let s = s.min(59) as i32;

            sign * (h * 3600 + m * 60 + s)
        })
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    // #[test]
    // fn test_tmp() {
    //     let mut s = r#"TZ="UTC12""#;
    //     let tz = parse(&mut s).unwrap();
    //     let now = Zoned::now();
    //     // let now = Timestamp::now().to_zoned(tz.clone());
    //     // .with_time_zone(TimeZone::UTC);
    //     println!("{tz:?}, {now}");
    // }

    #[test]
    fn parse_geographical() {
        for (input, expected) in [
            ("America/New_York", "America/New_York"),  // named timezone
            (":America/New_York", "America/New_York"), // named timezone with leading colon
            ("Etc/UTC", "Etc/UTC"),                    // etc timezone
            (":Etc/UTC", "Etc/UTC"),                   // etc timezone with leading colon
            ("UTC", "UTC"),                            // utc timezone
            (":UTC", "UTC"),                           // utc timezone with leading colon
            ("Unknown/Timezone", "UTC"),               // unknown timezone
            (":Unknown/Timezone", "UTC"),              // unknown timezone with leading colon
        ] {
            let mut s = input;
            assert_eq!(
                geographical(&mut s).unwrap().iana_name(),
                Some(expected),
                "{input}"
            );
        }
    }

    #[test]
    fn parse_proleptic() {
        let to_seconds = |input: &str| {
            let mut s = input;
            proleptic(&mut s)
                .unwrap()
                .to_fixed_offset()
                .unwrap()
                .seconds()
        };

        // no offset
        for (input, expected) in [
            ("UTC", 0), // utc timezone, no offset
            ("ABC", 0), // unknown timezone (treated as UTC), no offset
        ] {
            assert_eq!(to_seconds(input), expected, "{input}");
        }

        // hour
        for (input, expected) in [
            ("UTC", 0),           // utc timezone, no offset
            ("UTC0", 0),          // utc timezone, zero offset
            ("UTC+0", 0),         // utc timezone, zero offset with explicit plus
            ("UTC-0", 0),         // utc timezone, zero offset with explicit minus
            ("UTC000", 0),        // utc timezone, zero offset with three digits
            ("UTC+5", 5 * 3600),  // utc timezone, positive hour offset
            ("UTC-5", -5 * 3600), // utc timezone, negative hour offset
            ("ABC0", 0),          // unknown timezone (treated as UTC), zero offset
        ] {
            assert_eq!(to_seconds(input), expected, "{input}");
        }

        // hour:minute
        for (input, expected) in [
            ("UTC0:0", 0),                       // utc timezone, zero hour and minute offset
            ("UTC+0:0", 0), // utc timezone, zero hour and minute offset with explicit plus
            ("UTC-0:0", 0), // utc timezone, zero hour and minute offset with explicit minus
            ("UTC00:00", 0), // utc timezone, zero hour and minute offset with two digits
            ("UTC+5:30", 5 * 3600 + 30 * 60), // utc timezone, positive hour and minute offset
            ("UTC-5:30", -(5 * 3600 + 30 * 60)), // utc timezone, negative hour and minute offset
            ("ABC0:0", 0),  // unknown timezone (treated as UTC), zero hour and minute offset
        ] {
            assert_eq!(to_seconds(input), expected, "{input}");
        }

        // hour:minute:second
        for (input, expected) in [
            ("UTC0:0:0", 0),    // utc timezone, zero hour, minute, and second offset
            ("UTC+0:0:0", 0), // utc timezone, zero hour, minute, and second offset with explicit plus
            ("UTC-0:0:0", 0), // utc timezone, zero hour, minute, and second offset with explicit minus
            ("UTC00:00:00", 0), // utc timezone, zero hour, minute, and second offset with two digits
            ("UTC+5:30:15", 5 * 3600 + 30 * 60 + 15), // utc timezone, positive hour, minute, and second offset
            ("UTC-5:30:15", -(5 * 3600 + 30 * 60 + 15)), // utc timezone, negative hour, minute, and second offset
            ("ABC0:0:0", 0), // unknown timezone (treated as UTC), zero hour, minute, and second offset
        ] {
            assert_eq!(to_seconds(input), expected, "{input}");
        }
    }

    #[test]
    fn parse_proleptic_std() {
        for (input, expected) in [
            ("UTC", "UTC"), // utc timezone
            ("ABC", "UTC"), // unknown timezone (treated as UTC)
        ] {
            let mut s = input;
            assert_eq!(
                proleptic_std(&mut s).unwrap().iana_name(),
                Some(expected),
                "{input}"
            );
        }

        for input in [
            "AB",  // too short
            "A1C", // not just letters
        ] {
            let mut s = input;
            assert!(proleptic_std(&mut s).is_err(), "{input}");
        }
    }

    #[test]
    fn parse_proleptic_offset() {
        // hour
        for (input, expected) in [
            ("0", 0),            // zero hour
            ("00", 0),           // zero hour, two digits
            ("000", 0),          // zero hour, three digits
            ("+0", 0),           // zero hour, explicit plus
            ("-0", 0),           // zero hour, explicit minus
            ("5", 5 * 3600),     // positive hour
            ("-5", -5 * 3600),   // negative hour
            ("005", 5 * 3600),   // positive hour with leading zeros
            ("-05", -5 * 3600),  // negative hour with leading zeros
            ("25", 24 * 3600),   // hour > 24, clamps to 24 (GNU quirk)
            ("-25", -24 * 3600), // hour > 24, clamps to 24 (GNU quirk)
        ] {
            let mut s = input;
            assert_eq!(proleptic_offset(&mut s).unwrap(), expected, "{input}");
        }

        // hour:minute
        for (input, expected) in [
            ("0:0", 0),                         // zero hour and minute
            ("00:00", 0),                       // zero hour and minute, two digits
            ("000:000", 0),                     // zero hour and minute, three digits
            ("+0:0", 0),                        // zero hour and minute, explicit plus
            ("-0:0", 0),                        // zero hour and minute, explicit minus
            ("5:30", 5 * 3600 + 30 * 60),       // positive hour and minute
            ("-5:30", -(5 * 3600 + 30 * 60)),   // negative hour and minute
            ("005:030", 5 * 3600 + 30 * 60),    // positive hour and minute with leading zeros
            ("-05:30", -(5 * 3600 + 30 * 60)),  // negative hour and minute with leading zeros
            ("25:30", 24 * 3600 + 30 * 60),     // hour > 24, clamps to 24 (GNU quirk)
            ("-25:30", -(24 * 3600 + 30 * 60)), // hour > 24, clamps to 24 (GNU quirk)
            ("5:60", 5 * 3600 + 59 * 60),       // minute > 59, clamps to 59 (GNU quirk)
            ("-5:60", -(5 * 3600 + 59 * 60)),   // minute > 59, clamps to 59 (GNU quirk)
        ] {
            let mut s = input;
            assert_eq!(proleptic_offset(&mut s).unwrap(), expected, "{input}");
        }

        // hour:minute:second
        for (input, expected) in [
            ("0:0:0", 0),                               // zero hour, minute, and second
            ("00:00:00", 0),                            // zero hour, minute, and second, two digits
            ("000:000:000", 0), // zero hour, minute, and second, three digits
            ("+0:0:0", 0),      // zero hour, minute, and second, explicit plus
            ("-0:0:0", 0),      // zero hour, minute, and second, explicit minus
            ("5:30:15", 5 * 3600 + 30 * 60 + 15), // positive hour, minute, and second
            ("-5:30:15", -(5 * 3600 + 30 * 60 + 15)), // negative hour, minute, and second
            ("005:030:015", 5 * 3600 + 30 * 60 + 15), // positive hour, minute, and second with leading zeros
            ("-05:30:15", -(5 * 3600 + 30 * 60 + 15)), // negative hour, minute, and second with leading zeros
            ("25:30:15", 24 * 3600 + 30 * 60 + 15),    // hour > 24, clamps to 24 (GNU quirk)
            ("-25:30:15", -(24 * 3600 + 30 * 60 + 15)), // hour > 24, clamps to 24 (GNU quirk)
            ("5:60:15", 5 * 3600 + 59 * 60 + 15),      // minute > 59, clamps to 59 (GNU quirk)
            ("-5:60:15", -(5 * 3600 + 59 * 60 + 15)),  // minute > 59, clamps to 59 (GNU quirk)
            ("5:30:60", 5 * 3600 + 30 * 60 + 59),      // second > 59, clamps to 59 (GNU quirk)
            ("-5:30:60", -(5 * 3600 + 30 * 60 + 59)),  // second > 59, clamps to 59 (GNU quirk)
        ] {
            let mut s = input;
            assert_eq!(proleptic_offset(&mut s).unwrap(), expected, "{input}");
        }
    }
}

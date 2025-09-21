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

use jiff::tz::TimeZone;
use winnow::{
    combinator::{alt, delimited, opt, preceded},
    stream::AsChar,
    token::take_while,
    ModalResult, Parser,
};

use super::primitive::{dec_int, dec_uint, plus_or_minus};

pub(super) fn parse(input: &mut &str) -> ModalResult<TimeZone> {
    preceded("TZ=", delimited('"', alt((proleptic, geographical)), '"')).parse_next(input)
}

/// Parse forms like `America/New_York` or `Etc/UTC`.
fn geographical(input: &mut &str) -> ModalResult<TimeZone> {
    todo!()
}

/// Parse a proleptic timezone specification.
///
/// Note: This implementation is incomplete. It currently only parses the
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
    todo!()
}

fn proleptic_std(input: &mut &str) -> ModalResult<TimeZone> {
    take_while(3.., AsChar::is_alpha)
        .map(|name| TimeZone::get(name).unwrap_or(TimeZone::UTC))
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
            // - If hour is greater than 23, clamp it to 0.
            // - If minute is greater than 59, clamp it to 59.
            // - If second is greater than 59, clamp it to 59.
            let h = if h > 23 { 0 } else { h } as i32;
            let m = m.min(59) as i32;
            let s = s.min(59) as i32;
            sign * (h * 3600 + m * 60 + s)
        })
        .parse_next(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_proleptic_offset() {
        // hour
        for (input, expected) in [
            ("0", 0),           // zero hour
            ("00", 0),          // zero hour
            ("000", 0),         // zero hour
            ("+0", 0),          // zero hour, explicit plus
            ("-0", 0),          // zero hour, explicit minus
            ("5", 5 * 3600),    // positive hour
            ("-5", -5 * 3600),  // negative hour
            ("005", 5 * 3600),  // positive hour with leading zeros
            ("-05", -5 * 3600), // negative hour with leading zeros
            ("24", 0),          // hour > 23, clamps to 0 (GNU quirk)
            ("-24", 0),         // hour < -23, clamps to 0 (GNU quirk)
        ] {
            let mut s = input;
            assert_eq!(proleptic_offset(&mut s).unwrap(), expected, "{input}");
        }

        // hour:minute
        for (input, expected) in [
            ("0:0", 0),                        // zero hour and minute
            ("00:00", 0),                      // zero hour and minute, two digits
            ("000:000", 0),                    // zero hour and minute, three digits
            ("+0:0", 0),                       // zero hour and minute, explicit plus
            ("-0:0", 0),                       // zero hour and minute, explicit minus
            ("5:30", 5 * 3600 + 30 * 60),      // positive hour and minute
            ("-5:30", -(5 * 3600 + 30 * 60)),  // negative hour and minute
            ("005:030", 5 * 3600 + 30 * 60),   // positive hour and minute with leading zeros
            ("-05:30", -(5 * 3600 + 30 * 60)), // negative hour and minute with leading zeros
            ("24:30", 30 * 60),                // hour > 23, clamps to 0 (GNU quirk)
            ("-24:30", -30 * 60),              // hour < -23, clamps to 0 (GNU quirk)
            ("5:60", 5 * 3600 + 59 * 60),      // minute > 59, clamps to 59 (GNU quirk)
            ("-5:60", -(5 * 3600 + 59 * 60)),  // minute > 59, clamps to 59 (GNU quirk)
        ] {
            let mut s = input;
            assert_eq!(proleptic_offset(&mut s).unwrap(), expected, "{input}");
        }

        // hour:minute:second
        for (input, expected) in [
            ("0:0:0", 0),                              // zero hour, minute, and second
            ("00:00:00", 0),                           // zero hour, minute, and second, two digits
            ("000:000:000", 0), // zero hour, minute, and second, three digits
            ("+0:0:0", 0),      // zero hour, minute, and second, explicit plus
            ("-0:0:0", 0),      // zero hour, minute, and second, explicit minus
            ("5:30:15", 5 * 3600 + 30 * 60 + 15), // positive hour, minute, and second
            ("-5:30:15", -(5 * 3600 + 30 * 60 + 15)), // negative hour, minute, and second
            ("005:030:015", 5 * 3600 + 30 * 60 + 15), // positive hour, minute, and second with leading zeros
            ("-05:30:15", -(5 * 3600 + 30 * 60 + 15)), // negative hour, minute, and second with leading zeros
            ("24:30:15", 30 * 60 + 15),                // hour > 23, clamps to 0 (GNU quirk)
            ("-24:30:15", -(30 * 60 + 15)),            // hour < -23, clamps to 0 (GNU quirk)
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

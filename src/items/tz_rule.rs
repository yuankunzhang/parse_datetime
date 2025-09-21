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
use winnow::ModalResult;

pub(super) fn parse(input: &mut &str) -> ModalResult<TimeZone> {
    todo!()
}

fn utc_offset(input: &mut &str) -> ModalResult<TimeZone> {
    todo!()
}

fn named_tz(input: &mut &str) -> ModalResult<TimeZone> {
    todo!()
}

//! Timezone Translator — convert timestamps between timezones.
//!
//! Rust port of the Ruby reference implementation (`lib/tztr.rb`). Kept
//! functionally identical: same timestamp detection, same format preservation,
//! same timezone aliases and resolution. See `CLAUDE.md` for the parity
//! contract.
//!
//! Timezone math uses `jiff`, which reads the system tz database — the same
//! source Ruby's `Time` uses via `ENV['TZ']` — so DST behavior matches.

use jiff::civil::DateTime;
use jiff::tz::{Offset, TimeZone};
use jiff::Zoned;
use regex::Regex;
use std::sync::OnceLock;

/// Explicit output format (mirrors Ruby's `--format iso|short|time`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Iso,
    Short,
    Time,
}

/// One detected timestamp and its analysis, as emitted by [`matches`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub original: String,
    pub detected_format: String,
    pub detected_tz: Option<String>,
    /// `None` when detecting only, or when the timestamp failed to parse.
    pub translated: Option<String>,
}

/// Timestamp patterns, ordered and first-match-wins per line (see CLAUDE.md).
/// More specific patterns (with timezone) come before less specific ones.
fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            [
                // ISO 8601 with Z or offset
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})",
                // ISO 8601 without timezone
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?",
                // Date space time with tz
                r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)? ?(?:UTC|GMT|[A-Z]{2,4}|[+-]\d{4})",
                // Date space time
                r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?",
                // Time with tz
                r"\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?(?:UTC|GMT|[A-Z]{2,4}|[+-]\d{4})\b",
                // Time with offset
                r"\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?[+-]\d{2}:?\d{2}\b",
                // Bare time
                r"\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?\b",
            ]
            .iter()
            .map(|p| Regex::new(p).expect("valid pattern"))
            .collect()
        })
        .as_slice()
}

/// Resolve a user-supplied zone (alias, numeric offset, or IANA name) to an
/// IANA-style name string. Mirrors `Tztr.resolve_tz`.
pub fn resolve_tz(input: &str) -> String {
    // Numeric offset: -7 -> Etc/GMT+7 (POSIX sign is inverted)
    if numeric_offset_re().is_match(input) {
        let n: i32 = input.parse().unwrap_or(0);
        if n == 0 {
            return "UTC".to_string();
        }
        let sign = if n > 0 { '-' } else { '+' };
        return format!("Etc/GMT{}{}", sign, n.abs());
    }

    let key = input.to_lowercase().replace(' ', "_");
    timezone_aliases()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| input.to_string())
}

fn numeric_offset_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[+-]?\d{1,2}$").unwrap())
}

/// Translate every detected timestamp in `line`, preserving surrounding text.
/// `to`/`from` accept aliases; `from` is the assumed input zone for naive
/// timestamps; `date` supplies a reference date for time-only inputs.
pub fn translate(
    line: &str,
    to: &str,
    from: Option<&str>,
    format: Option<Format>,
    local: bool,
    date: Option<&str>,
) -> String {
    let to_tz = resolve_zone(to);
    let from_tz = from.map(resolve_zone);

    for pattern in patterns() {
        if pattern.is_match(line) {
            return pattern
                .replace_all(line, |caps: &regex::Captures| {
                    let m = &caps[0];
                    convert_match(m, from_tz.as_ref(), &to_tz, format, local, date)
                        .unwrap_or_else(|| m.to_string())
                })
                .into_owned();
        }
    }

    line.to_string()
}

/// Per-match structured analysis of a line. With `detect`, translation is
/// skipped and [`Match::translated`] is `None`. Mirrors `Tztr.matches`.
#[allow(clippy::too_many_arguments)]
pub fn matches(
    line: &str,
    to: &str,
    from: Option<&str>,
    format: Option<Format>,
    local: bool,
    detect: bool,
    date: Option<&str>,
) -> Vec<Match> {
    let to_tz = resolve_zone(to);
    let from_tz = from.map(resolve_zone);
    let mut results = Vec::new();

    for pattern in patterns() {
        if pattern.is_match(line) {
            for m in pattern.find_iter(line) {
                let original = m.as_str();
                let translated = if detect {
                    None
                } else {
                    convert_match(original, from_tz.as_ref(), &to_tz, format, local, date)
                };
                results.push(Match {
                    original: original.to_string(),
                    detected_format: detect_format(original).to_string(),
                    detected_tz: detect_zone(original),
                    translated,
                });
            }
            break;
        }
    }

    results
}

fn convert_match(
    m: &str,
    from_tz: Option<&TimeZone>,
    to_tz: &TimeZone,
    format: Option<Format>,
    local: bool,
    date: Option<&str>,
) -> Option<String> {
    let zoned = parse(m, from_tz, to_tz, date)?;
    Some(format_time(&zoned, format, m, local))
}

/// Label the detected format. Mirrors `Tztr.detect_format`.
pub fn detect_format(s: &str) -> &'static str {
    if iso_t_re().is_match(s) {
        "iso"
    } else if date_space_re().is_match(s) {
        "datetime"
    } else {
        "time"
    }
}

/// Extract the literal timezone token from a match, if present. Mirrors
/// `Tztr.detect_zone`.
pub fn detect_zone(s: &str) -> Option<String> {
    detect_zone_re().captures(s).map(|c| c[1].to_string())
}

fn iso_t_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2}T").unwrap())
}

fn date_space_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{4}-\d{2}-\d{2} ").unwrap())
}

fn detect_zone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s?(Z|[+-]\d{2}:?\d{2}|UTC|GMT|[A-Z]{2,4})$").unwrap())
}

// --- timestamp parsing -----------------------------------------------------

fn components_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"^(?:(\d{4})-(\d{2})-(\d{2})[T ])?(\d{1,2}):(\d{2})(?::(\d{2})(?:\.(\d+))?)?\s?(Z|[+-]\d{2}:?\d{2}|[+-]\d{4}|[A-Za-z]{2,4})?$",
        )
        .unwrap()
    })
}

fn time_only_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{1,2}:").unwrap())
}

/// Parse a matched timestamp into an instant expressed in `to_tz`.
///
/// Mirrors `Tztr.parse`'s three branches: an embedded recognized zone wins; a
/// naive timestamp uses `from_tz` if given; otherwise it is interpreted as
/// already being in `to_tz`. An *unrecognized* zone abbreviation is ignored and
/// parsed in `to_tz` — matching Ruby's `Time.parse`.
fn parse(
    s: &str,
    from_tz: Option<&TimeZone>,
    to_tz: &TimeZone,
    date: Option<&str>,
) -> Option<Zoned> {
    let owned;
    let s = match date {
        Some(d) if time_only_re().is_match(s) => {
            owned = format!("{d} {s}");
            owned.as_str()
        }
        _ => s,
    };

    let caps = components_re().captures(s)?;

    let hour: i8 = caps[4].parse().ok()?;
    let minute: i8 = caps[5].parse().ok()?;
    let second: i8 = caps.get(6).map_or(0, |m| m.as_str().parse().unwrap_or(0));
    let nanos = caps.get(7).map_or(0, |m| frac_to_nanos(m.as_str()));
    let zone_token = caps.get(8).map(|m| m.as_str()).unwrap_or("");

    let recognized = zone_offset_seconds(zone_token);
    let has_token = !zone_token.is_empty();

    // Determine how to anchor the wall-clock time, and which zone supplies
    // "today" for date-less inputs.
    enum Mode<'a> {
        Fixed(i32),
        InZone(&'a TimeZone),
    }
    let (mode, today_zone): (Mode, &TimeZone) = if has_token {
        match recognized {
            Some(off) => (Mode::Fixed(off), to_tz),
            None => (Mode::InZone(to_tz), to_tz), // unknown abbrev -> ignored
        }
    } else if let Some(f) = from_tz {
        (Mode::InZone(f), f)
    } else {
        (Mode::InZone(to_tz), to_tz)
    };

    let (year, month, day) = match (caps.get(1), caps.get(2), caps.get(3)) {
        (Some(y), Some(mo), Some(d)) => (
            y.as_str().parse().ok()?,
            mo.as_str().parse().ok()?,
            d.as_str().parse().ok()?,
        ),
        _ => today_in(today_zone),
    };

    let civil = DateTime::new(year, month, day, hour, minute, second, nanos).ok()?;

    let instant = match mode {
        Mode::Fixed(off) => {
            let tz = TimeZone::fixed(Offset::from_seconds(off).ok()?);
            civil.to_zoned(tz).ok()?
        }
        Mode::InZone(tz) => civil.to_zoned(tz.clone()).ok()?,
    };

    Some(instant.with_time_zone(to_tz.clone()))
}

fn today_in(tz: &TimeZone) -> (i16, i8, i8) {
    let d = Zoned::now().with_time_zone(tz.clone()).date();
    (d.year(), d.month(), d.day())
}

fn frac_to_nanos(frac: &str) -> i32 {
    let mut digits: String = frac.chars().take(9).collect();
    while digits.len() < 9 {
        digits.push('0');
    }
    digits.parse().unwrap_or(0)
}

/// Recognized zone offset in seconds, or `None` for empty/unknown tokens.
/// Mirrors the abbreviation table Ruby's `Time.parse` uses.
fn zone_offset_seconds(token: &str) -> Option<i32> {
    if token.is_empty() {
        return None;
    }
    if token == "Z" {
        return Some(0);
    }
    if numeric_zone_re().is_match(token) {
        return parse_numeric_offset(token);
    }
    match token.to_uppercase().as_str() {
        "UTC" | "GMT" | "UT" => Some(0),
        "EST" => Some(-5 * 3600),
        "EDT" => Some(-4 * 3600),
        "CST" => Some(-6 * 3600),
        "CDT" => Some(-5 * 3600),
        "MST" => Some(-7 * 3600),
        "MDT" => Some(-6 * 3600),
        "PST" => Some(-8 * 3600),
        "PDT" => Some(-7 * 3600),
        _ => None,
    }
}

fn numeric_zone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[+-]\d{2}:?\d{2}$").unwrap())
}

fn parse_numeric_offset(token: &str) -> Option<i32> {
    let sign = if token.starts_with('-') { -1 } else { 1 };
    let digits: String = token[1..].chars().filter(|c| *c != ':').collect();
    if digits.len() != 4 {
        return None;
    }
    let hh: i32 = digits[0..2].parse().ok()?;
    let mm: i32 = digits[2..4].parse().ok()?;
    Some(sign * (hh * 3600 + mm * 60))
}

/// Resolve to a `TimeZone`, falling back to UTC for unknown names — matching
/// Ruby, where an invalid `ENV['TZ']` is treated as UTC.
fn resolve_zone(input: &str) -> TimeZone {
    let name = resolve_tz(input);
    TimeZone::get(&name).unwrap_or(TimeZone::UTC)
}

// --- output formatting -----------------------------------------------------

fn frac_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{2}:\d{2}:\d{2}\.\d+").unwrap())
}

fn has_seconds_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{1,2}:\d{2}:\d{2}").unwrap())
}

fn hour_minute_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{1,2}:\d{2}").unwrap())
}

/// Rebuild the output to mirror the original match (or honor an explicit
/// format). Mirrors `Tztr.format_time`.
fn format_time(zoned: &Zoned, fmt: Option<Format>, original: &str, local: bool) -> String {
    let offset_secs = zoned.offset().seconds();
    let tz = if offset_secs == 0 {
        "Z".to_string()
    } else {
        offset_colon(offset_secs)
    };
    let abbrev = zoned.strftime("%Z").to_string();

    match fmt {
        Some(Format::Time) => return strf(zoned, "%H:%M:%S"),
        Some(Format::Iso) => return format!("{}{}", strf(zoned, "%Y-%m-%d %H:%M:%S"), tz),
        Some(Format::Short) => {
            let base = strf(zoned, "%Y-%m-%d %H:%M");
            if local {
                return base;
            }
            return format!("{base} {abbrev}");
        }
        None => {}
    }

    // Preserve input format.
    if iso_t_re().is_match(original) {
        let base = if frac_re().is_match(original) {
            format!("{}.{}", strf(zoned, "%Y-%m-%dT%H:%M:%S"), millis(zoned))
        } else {
            strf(zoned, "%Y-%m-%dT%H:%M:%S")
        };
        format!("{base}{tz}")
    } else if date_space_re().is_match(original) {
        let base = if frac_re().is_match(original) {
            format!("{}.{}", strf(zoned, "%Y-%m-%d %H:%M:%S"), millis(zoned))
        } else {
            strf(zoned, "%Y-%m-%d %H:%M:%S")
        };
        format!("{base} {abbrev}")
    } else if has_seconds_re().is_match(original) {
        format!("{} {abbrev}", strf(zoned, "%H:%M:%S"))
    } else if hour_minute_re().is_match(original) {
        format!("{} {abbrev}", strf(zoned, "%H:%M"))
    } else {
        format!("{} {abbrev}", strf(zoned, "%Y-%m-%d %H:%M:%S"))
    }
}

fn strf(zoned: &Zoned, fmt: &str) -> String {
    zoned.strftime(fmt).to_string()
}

fn millis(zoned: &Zoned) -> String {
    format!("{:03}", zoned.subsec_nanosecond() / 1_000_000)
}

fn offset_colon(secs: i32) -> String {
    let sign = if secs < 0 { '-' } else { '+' };
    let secs = secs.abs();
    format!("{}{:02}:{:02}", sign, secs / 3600, (secs % 3600) / 60)
}

include!("aliases.rs");

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(line: &str, to: &str) -> String {
        translate(line, to, None, None, false, None)
    }

    #[test]
    fn passes_through_iso_z_to_utc() {
        assert_eq!(tr("2026-04-03T12:00:00Z", "UTC"), "2026-04-03T12:00:00Z");
    }

    #[test]
    fn converts_iso_z_to_timezone() {
        assert_eq!(
            tr("2026-04-03T12:00:00Z", "America/Los_Angeles"),
            "2026-04-03T05:00:00-07:00"
        );
    }

    #[test]
    fn converts_iso_offset_to_utc() {
        assert_eq!(
            tr("2026-04-03T05:00:00-07:00", "UTC"),
            "2026-04-03T12:00:00Z"
        );
    }

    #[test]
    fn preserves_fractional_seconds() {
        assert_eq!(
            tr("2026-04-03T12:00:00.123Z", "America/Los_Angeles"),
            "2026-04-03T05:00:00.123-07:00"
        );
    }

    #[test]
    fn converts_space_format_with_tz() {
        assert_eq!(
            tr("2026-04-03 12:00:00 UTC", "America/Los_Angeles"),
            "2026-04-03 05:00:00 PDT"
        );
    }

    #[test]
    fn converts_time_with_tz() {
        assert_eq!(tr("15:30 UTC", "America/Los_Angeles"), "08:30 PDT");
    }

    #[test]
    fn preserves_surrounding_text() {
        assert_eq!(
            tr(
                "log 2026-04-03T12:00:00Z something happened",
                "America/New_York"
            ),
            "log 2026-04-03T08:00:00-04:00 something happened"
        );
    }

    #[test]
    fn replaces_multiple_timestamps_on_same_line() {
        assert_eq!(
            tr("from 15:30 UTC to 16:45 UTC", "America/Los_Angeles"),
            "from 08:30 PDT to 09:45 PDT"
        );
    }

    #[test]
    fn formats_as_short_with_abbreviation() {
        assert_eq!(
            translate(
                "2026-04-03T12:00:00Z",
                "America/Los_Angeles",
                None,
                Some(Format::Short),
                false,
                None
            ),
            "2026-04-03 05:00 PDT"
        );
    }

    #[test]
    fn formats_as_short_without_zone_when_local() {
        assert_eq!(
            translate(
                "2026-04-03T12:00:00Z",
                "America/Los_Angeles",
                None,
                Some(Format::Short),
                true,
                None
            ),
            "2026-04-03 05:00"
        );
    }

    #[test]
    fn formats_as_iso() {
        assert_eq!(
            translate(
                "2026-04-03T12:00:00Z",
                "America/Los_Angeles",
                None,
                Some(Format::Iso),
                false,
                None
            ),
            "2026-04-03 05:00:00-07:00"
        );
    }

    #[test]
    fn formats_as_time() {
        assert_eq!(
            translate(
                "2026-04-03T12:00:00Z",
                "America/Los_Angeles",
                None,
                Some(Format::Time),
                false,
                None
            ),
            "05:00:00"
        );
    }

    #[test]
    fn applies_from_timezone_to_naive_timestamps() {
        assert_eq!(
            translate(
                "2026-04-03T12:00:00",
                "UTC",
                Some("America/Los_Angeles"),
                None,
                false,
                None
            ),
            "2026-04-03T19:00:00Z"
        );
    }

    #[test]
    fn passes_through_lines_without_timestamps() {
        assert_eq!(tr("no timestamps here", "UTC"), "no timestamps here");
    }

    #[test]
    fn resolve_tz_abbreviations_and_cities() {
        assert_eq!(resolve_tz("pst"), "America/Los_Angeles");
        assert_eq!(resolve_tz("PST"), "America/Los_Angeles");
        assert_eq!(resolve_tz("sf"), "America/Los_Angeles");
        assert_eq!(resolve_tz("nyc"), "America/New_York");
        assert_eq!(resolve_tz("tokyo"), "Asia/Tokyo");
        assert_eq!(resolve_tz("utc"), "UTC");
    }

    #[test]
    fn resolve_tz_numeric_offsets() {
        assert_eq!(resolve_tz("-7"), "Etc/GMT+7");
        assert_eq!(resolve_tz("+9"), "Etc/GMT-9");
        assert_eq!(resolve_tz("0"), "UTC");
        assert_eq!(resolve_tz("-12"), "Etc/GMT+12");
    }

    #[test]
    fn resolve_tz_passes_through_iana() {
        assert_eq!(resolve_tz("America/Chicago"), "America/Chicago");
    }

    #[test]
    fn matches_returns_structured_info() {
        let m = matches(
            "2026-04-03T12:00:00Z",
            "America/Los_Angeles",
            None,
            None,
            false,
            false,
            None,
        );
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].original, "2026-04-03T12:00:00Z");
        assert_eq!(m[0].detected_format, "iso");
        assert_eq!(m[0].detected_tz.as_deref(), Some("Z"));
        assert_eq!(
            m[0].translated.as_deref(),
            Some("2026-04-03T05:00:00-07:00")
        );
    }

    #[test]
    fn matches_detect_omits_translated() {
        let m = matches("15:30 PST", "UTC", None, None, false, true, None);
        assert_eq!(m[0].detected_format, "time");
        assert_eq!(m[0].detected_tz.as_deref(), Some("PST"));
        assert_eq!(m[0].translated, None);
    }

    #[test]
    fn reference_date_resolves_dst() {
        // 15:30 in LA on Jan 15 is PST (-08:00) -> 23:30 UTC
        assert_eq!(
            translate(
                "15:30",
                "UTC",
                Some("America/Los_Angeles"),
                None,
                false,
                Some("2026-01-15")
            ),
            "23:30 UTC"
        );
        // ...and PDT (-07:00) in July -> 22:30 UTC
        assert_eq!(
            translate(
                "15:30",
                "UTC",
                Some("America/Los_Angeles"),
                None,
                false,
                Some("2026-07-15")
            ),
            "22:30 UTC"
        );
    }
}

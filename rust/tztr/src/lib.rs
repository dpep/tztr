//! Timezone Translator — convert timestamps between timezones.
//!
//! Rust port of the Ruby reference implementation (`lib/tztr.rb`). Kept
//! functionally identical: same timestamp detection, same format preservation,
//! same timezone aliases and resolution. See `CLAUDE.md` for the parity
//! contract.
//!
//! Timezone math uses `jiff`, which reads the system tz database — the same
//! source Ruby's `Time` uses via `ENV['TZ']` — so DST behavior matches.

use jiff::civil::{Date, DateTime};
use jiff::tz::{Offset, TimeZone};
use jiff::{Span, Zoned};
use regex::bytes::Regex as BytesRegex;
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

/// Timezone abbreviations `tztr` recognizes inside text.
///
/// The union of the abbreviations Ruby's `Time.parse` resolves natively
/// (`UT UTC GMT` plus E/C/M/P × ST/DT) and the abbreviation-shaped keys of
/// [`timezone_aliases`]. Deliberately an allowlist: the `[A-Z]{2,4}` wildcard
/// this replaces ate log levels (`INFO`, `WARN`, `ERROR`) and meridiems (`PM`)
/// straight out of the line.
///
/// City nicknames (`sf`, `nyc`, …) are *not* here — they are `-t`/`-f` values,
/// not things to look for inside text.
pub const ZONE_ABBREVIATIONS: &[&str] = &[
    "AEDT", "AEST", "AKDT", "AKST", "BST", "CDT", "CEST", "CET", "CST", "CT", "EDT", "EST", "ET",
    "GMT", "HKT", "HST", "IST", "JST", "KST", "MDT", "MST", "MT", "NZDT", "NZST", "PDT", "PST",
    "PT", "UT", "UTC", "Z",
];

/// Abbreviations whose lowercase spelling is also a word likely to follow a
/// time — French "est"/"cet"/"et", German "ist" — so "à 15:30 est annulée" is
/// left alone. Mirrors `Tztr::WORD_ABBREVIATIONS`.
const WORD_ABBREVIATIONS: &[&str] = &["EST", "CET", "ET", "IST", "UT", "Z"];

/// [`ZONE_ABBREVIATIONS`] as a regex alternation: uppercase, or wholly
/// lowercase unless that is also a word, never mixed case. Longest token first
/// — the regex crate's alternation is leftmost-*first*, so `UT` ahead of `UTC`
/// would clip the trailing `C` off and orphan it in the output.
fn zone_alternation() -> &'static str {
    static ALT: OnceLock<String> = OnceLock::new();
    ALT.get_or_init(|| {
        let lowercase = ZONE_ABBREVIATIONS
            .iter()
            .filter(|t| !WORD_ABBREVIATIONS.contains(t))
            .map(|t| t.to_lowercase());
        let mut tokens: Vec<String> = ZONE_ABBREVIATIONS
            .iter()
            .map(|t| t.to_string())
            .chain(lowercase)
            .collect();
        tokens.sort_by_key(|t| std::cmp::Reverse(t.len()));
        tokens.join("|")
    })
}

/// The zone an abbreviation names, for the ones that are not a fixed offset
/// Ruby's `Time.parse` already knows (`CET`, `JST`, `AEST`, …).
fn abbrev_zone(token: &str) -> Option<TimeZone> {
    let key = token.to_lowercase();
    let name = timezone_aliases()
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, v)| *v)?;
    TimeZone::get(name).ok()
}

/// Every pattern is pure ASCII, so every substring one matches is too.
fn ascii(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("the patterns match ASCII only")
}

/// Timestamp patterns, ordered and first-match-wins per line (see CLAUDE.md).
/// More specific patterns (with timezone) come before less specific ones.
///
/// Byte-oriented so a line with a stray non-UTF-8 byte in it can still have its
/// timestamps converted without those bytes being rewritten. Unicode mode stays
/// on: `\b` has to agree with Ruby's, which is Unicode-aware.
fn patterns() -> &'static [BytesRegex] {
    static PATTERNS: OnceLock<Vec<BytesRegex>> = OnceLock::new();
    PATTERNS
        .get_or_init(|| {
            let zone = zone_alternation();
            // A trailing zone token: an allowlisted abbreviation or +HHMM.
            let tz = format!(r"(?:(?:{zone})\b|[+-]\d{{4}}\b)");
            // Seconds and fractional seconds, both optional.
            let secs = r"(?::\d{2}(?:\.\d+)?)?";
            // A meridiem, optionally followed by a zone ("3:45 PM PST"). A
            // dotted one takes its closing dot; an undotted one leaves a
            // following full stop to the sentence.
            let mer = format!(r" ?[AaPp](?:\.[Mm]\.|\.?[Mm]\b)(?: ?{tz})?");
            [
                // ISO 8601 with Z or offset
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})".to_string(),
                // ISO 8601 without timezone
                r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?".to_string(),
                // Date space 12-hour time — above the with-tz pattern, and the
                // date must be part of the match or the time resolves its DST
                // against today instead of the date beside it.
                format!(r"\d{{4}}-\d{{2}}-\d{{2}} \d{{1,2}}:\d{{2}}{secs}{mer}"),
                // Date space time with tz
                format!(r"\d{{4}}-\d{{2}}-\d{{2}} \d{{2}}:\d{{2}}:\d{{2}}(?:\.\d+)? ?{tz}"),
                // Date space time
                r"\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?".to_string(),
                // 12-hour time
                format!(r"\b\d{{1,2}}:\d{{2}}{secs}{mer}"),
                // Time with tz
                format!(r"\b\d{{1,2}}:\d{{2}}{secs} ?{tz}"),
                // Time with offset
                format!(r"\b\d{{1,2}}:\d{{2}}{secs}[+-]\d{{2}}:?\d{{2}}\b"),
                // Bare time
                format!(r"\b\d{{1,2}}:\d{{2}}{secs}\b"),
            ]
            .iter()
            .map(|p| BytesRegex::new(p).expect("valid pattern"))
            .collect()
        })
        .as_slice()
}

/// Why a user-supplied timezone could not be resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TzError {
    /// Not an alias, not a numeric offset, and not a name the system tz
    /// database knows.
    Unknown(String),
    /// A whole-hour numeric offset outside the range real zones occupy.
    OffsetOutOfRange(String),
}

impl std::fmt::Display for TzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown(s) => write!(f, "unknown timezone: {s}"),
            Self::OffsetOutOfRange(s) => {
                write!(f, "offset out of range: {s} (expected -12..14)")
            }
        }
    }
}

impl std::error::Error for TzError {}

/// Resolve a user-supplied zone (alias, numeric offset, or IANA name) to an
/// IANA-style name the system tz database can load. Mirrors `Tztr.resolve_tz`.
///
/// This is the validating boundary: everything downstream may assume the name
/// it returns names a real zone. A zone that cannot be resolved is an error,
/// never a silent fall back to UTC — the old fallback answered every
/// conversion in the wrong zone, confidently.
pub fn resolve_tz(input: &str) -> Result<String, TzError> {
    // POSIX spells TZ with a leading colon (`TZ=:America/New_York`).
    let input = input.strip_prefix(':').unwrap_or(input);

    if numeric_offset_re().is_match(input) {
        let n: i32 = input.parse().map_err(|_| unknown(input))?;
        if n == 0 {
            return Ok("UTC".to_string());
        }
        if !(-12..=14).contains(&n) {
            return Err(TzError::OffsetOutOfRange(input.to_string()));
        }
        // Numeric offset: -7 -> Etc/GMT+7 (POSIX sign is inverted)
        let sign = if n > 0 { '-' } else { '+' };
        return Ok(format!("Etc/GMT{}{}", sign, n.abs()));
    }

    let key = input.to_lowercase().replace(' ', "_");
    let name = timezone_aliases()
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(input, |(_, v)| *v);

    TimeZone::get(name).map_err(|_| unknown(input))?;
    Ok(name.to_string())
}

fn unknown(input: &str) -> TzError {
    TzError::Unknown(input.to_string())
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
    date: Option<&str>,
) -> String {
    let out = translate_bytes(line.as_bytes(), to, from, format, date);
    String::from_utf8(out).expect("ASCII replacements inside valid UTF-8 stay valid UTF-8")
}

/// [`translate`] for input that may not be valid UTF-8 — a log line with a
/// stray byte in it. Timestamps still convert; every other byte, valid or not,
/// comes out exactly as it went in.
pub fn translate_bytes(
    line: &[u8],
    to: &str,
    from: Option<&str>,
    format: Option<Format>,
    date: Option<&str>,
) -> Vec<u8> {
    let to_tz = resolve_zone(to);
    let from_tz = from.map(resolve_zone);

    for pattern in patterns() {
        if pattern.is_match(line) {
            return pattern
                .replace_all(line, |caps: &regex::bytes::Captures| {
                    let m = ascii(&caps[0]);
                    convert_match(m, from_tz.as_ref(), &to_tz, format, date)
                        .unwrap_or_else(|| m.to_string())
                        .into_bytes()
                })
                .into_owned();
        }
    }

    line.to_vec()
}

/// Per-match structured analysis of a line. With `detect`, translation is
/// skipped and [`Match::translated`] is `None`. Mirrors `Tztr.matches`.
pub fn matches(
    line: &str,
    to: &str,
    from: Option<&str>,
    format: Option<Format>,
    detect: bool,
    date: Option<&str>,
) -> Vec<Match> {
    matches_bytes(line.as_bytes(), to, from, format, detect, date)
}

/// [`matches`] for input that may not be valid UTF-8. Every match is ASCII, so
/// the `Match` fields are text either way.
pub fn matches_bytes(
    line: &[u8],
    to: &str,
    from: Option<&str>,
    format: Option<Format>,
    detect: bool,
    date: Option<&str>,
) -> Vec<Match> {
    let to_tz = resolve_zone(to);
    let from_tz = from.map(resolve_zone);
    let mut results = Vec::new();

    for pattern in patterns() {
        if pattern.is_match(line) {
            for m in pattern.find_iter(line) {
                let original = ascii(m.as_bytes());
                let translated = if detect {
                    None
                } else {
                    convert_match(original, from_tz.as_ref(), &to_tz, format, date)
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
    date: Option<&str>,
) -> Option<String> {
    let zoned = parse(m, from_tz, to_tz, date)?;
    Some(format_time(&zoned, format, m))
}

/// Which assumptions a line's timestamps force on us, for `-v` to disclose.
///
/// The two are separate because they are assumed for separate reasons, and a
/// diagnostic that claims the wrong one is worse than a missing one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Assumptions {
    /// A timestamp carries no date, so DST in the *target* zone is resolved
    /// against today — `15:30 UTC` is 11:30 in New York in July and 10:30 in
    /// January, so naming its own zone does not get a timestamp off this hook.
    pub date: bool,
    /// A timestamp carries no zone either, so its source zone came from `$TZ`.
    /// Never set without `date`.
    pub zone: bool,
}

impl Assumptions {
    /// Whether anything at all is being assumed.
    pub fn any(self) -> bool {
        self.date || self.zone
    }

    /// The assumptions in `self` that `already` does not cover — what is left
    /// to say after an earlier line has spoken.
    #[must_use]
    pub fn minus(self, already: Self) -> Self {
        Self {
            date: self.date && !already.date,
            zone: self.zone && !already.zone,
        }
    }

    /// Everything either one assumes.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            date: self.date || other.date,
            zone: self.zone || other.zone,
        }
    }
}

/// What converting `line` would have to assume. See [`Assumptions`].
pub fn assumptions(line: &[u8]) -> Assumptions {
    let Some(pattern) = patterns().iter().find(|p| p.is_match(line)) else {
        return Assumptions::default();
    };

    let dateless: Vec<&str> = pattern
        .find_iter(line)
        .map(|m| ascii(m.as_bytes()))
        .filter(|s| detect_format(s) == "time")
        .collect();

    Assumptions {
        date: !dateless.is_empty(),
        zone: dateless.iter().any(|s| detect_zone(s).is_none()),
    }
}

/// Today's date in `tz` as `YYYY-MM-DD` — the date a date-less timestamp is
/// resolved against when no reference date is given.
pub fn today_in_zone(tz: &str) -> String {
    let (y, m, d) = today_in(&resolve_zone(tz));
    format!("{y:04}-{m:02}-{d:02}")
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
    RE.get_or_init(|| {
        let zone = zone_alternation();
        Regex::new(&format!(r"\s?({zone}|[+-]\d{{2}}:?\d{{2}})$")).unwrap()
    })
}

// --- timestamp parsing -----------------------------------------------------

fn components_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let zone = zone_alternation();
        Regex::new(&format!(
            r"^(?:(?<y>\d{{4}})-(?<mo>\d{{2}})-(?<d>\d{{2}})[T ])?(?<h>\d{{1,2}}):(?<mi>\d{{2}})(?::(?<s>\d{{2}})(?:\.(?<frac>\d+))?)?(?: ?(?<mer>[AaPp])\.?[Mm]\.?)?\s?(?<zone>{zone}|[+-]\d{{2}}:?\d{{2}}|[+-]\d{{4}})?$",
        ))
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
    let group = |n| caps.name(n).map(|m| m.as_str());

    let hour: i8 = group("h")?.parse().ok()?;
    let minute: i8 = group("mi")?.parse().ok()?;
    let second: i8 = group("s").map_or(0, |v| v.parse().unwrap_or(0));
    let nanos = group("frac").map_or(0, frac_to_nanos);
    let zone_token = group("zone").unwrap_or("");
    let hour = apply_meridiem(hour, group("mer"));

    // The zone the wall-clock time is anchored in — and, for date-less inputs,
    // the zone whose "today" fills the missing date (mirrors Ruby's
    // Time.parse). NOT the output zone when a zone is embedded, which would
    // land a day off near a midnight boundary.
    let anchor: TimeZone = if !zone_token.is_empty() {
        match zone_offset_seconds(zone_token) {
            // UTC/GMT/UT, E/C/M/P × ST/DT and numeric offsets are fixed offsets.
            Some(off) => TimeZone::fixed(Offset::from_seconds(off).ok()?),
            // CET, JST, AEST, … carry real DST rules; resolve them as zones.
            None => abbrev_zone(zone_token)?,
        }
    } else if let Some(f) = from_tz {
        f.clone()
    } else {
        to_tz.clone()
    };

    let (year, month, day) = match (group("y"), group("mo"), group("d")) {
        (Some(y), Some(mo), Some(d)) => (y.parse().ok()?, mo.parse().ok()?, d.parse().ok()?),
        _ => today_in(&anchor),
    };

    let civil = civil_datetime(
        Date::new(year, month, day).ok()?,
        hour,
        minute,
        second,
        nanos,
    )?;
    let instant = civil.to_zoned(anchor).ok()?;

    Some(instant.with_time_zone(to_tz.clone()))
}

/// Place a wall-clock reading on `date`, carrying the two overflows a clock
/// legitimately produces: `24:00`, the midnight that ends a day, and `:60`, a
/// leap second. Anything further out of range (`25:00`, `12:60`) is not a time
/// at all — the caller leaves that text exactly as it found it.
fn civil_datetime(date: Date, hour: i8, minute: i8, second: i8, nanos: i32) -> Option<DateTime> {
    let in_range = (0..=59).contains(&minute)
        && (0..=60).contains(&second)
        && match hour {
            0..=23 => true,
            24 => minute == 0 && second == 0,
            _ => false,
        };
    if !in_range {
        return None;
    }

    let span = Span::new()
        .hours(i64::from(hour))
        .minutes(i64::from(minute))
        .seconds(i64::from(second));
    date.at(0, 0, 0, nanos).checked_add(span).ok()
}

/// Fold a 12-hour clock reading onto the 24-hour clock. Out-of-range readings
/// (`15:30 PM`) are left alone — the calendar check downstream decides.
fn apply_meridiem(hour: i8, meridiem: Option<&str>) -> i8 {
    match meridiem.map(str::to_ascii_lowercase).as_deref() {
        Some("p") if hour < 12 => hour + 12,
        Some("a") if hour == 12 => 0,
        _ => hour,
    }
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

/// Resolve to a `TimeZone`. Callers are expected to have validated the name
/// through [`resolve_tz`] first (the CLI does, and exits on failure); the UTC
/// fallback here is the last resort for a library caller that did not.
fn resolve_zone(input: &str) -> TimeZone {
    resolve_tz(input)
        .ok()
        .and_then(|name| TimeZone::get(&name).ok())
        .unwrap_or(TimeZone::UTC)
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
fn format_time(zoned: &Zoned, fmt: Option<Format>, original: &str) -> String {
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
        // Always labelled: a cross-timezone tool whose output does not say
        // which zone it is in is not useful, and this output gets pasted.
        Some(Format::Short) => return format!("{} {abbrev}", strf(zoned, "%Y-%m-%d %H:%M")),
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
        translate(line, to, None, None, None)
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
                None
            ),
            "2026-04-03 05:00 PDT"
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
                None
            ),
            "2026-04-03T19:00:00Z"
        );
    }

    #[test]
    fn passes_through_lines_without_timestamps() {
        assert_eq!(tr("no timestamps here", "UTC"), "no timestamps here");
    }

    fn tz(input: &str) -> String {
        resolve_tz(input).expect("resolvable")
    }

    #[test]
    fn resolve_tz_abbreviations_and_cities() {
        for (input, expected) in [
            ("pst", "America/Los_Angeles"),
            ("PST", "America/Los_Angeles"),
            ("sf", "America/Los_Angeles"),
            ("nyc", "America/New_York"),
            ("tokyo", "Asia/Tokyo"),
            ("utc", "UTC"),
        ] {
            assert_eq!(tz(input), expected, "{input}");
        }
    }

    #[test]
    fn alias_table_is_sorted_and_unique() {
        // `-l` prints this table in order and must match Ruby's live sort;
        // the table is hand-written, so the invariant needs a guard.
        let keys: Vec<&str> = timezone_aliases().iter().map(|(k, _)| *k).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn gmt_is_utc_not_london() {
        // Europe/London follows British Summer Time; GMT does not.
        assert_eq!(tz("gmt"), "UTC");
        assert_eq!(tz("bst"), "Europe/London");
        assert_eq!(tz("london"), "Europe/London");
        assert_eq!(
            tr("2026-07-15T12:00:00Z", &tz("gmt")),
            "2026-07-15T12:00:00Z"
        );
    }

    #[test]
    fn resolve_tz_numeric_offsets() {
        assert_eq!(tz("-7"), "Etc/GMT+7");
        assert_eq!(tz("+9"), "Etc/GMT-9");
        assert_eq!(tz("0"), "UTC");
        assert_eq!(tz("-12"), "Etc/GMT+12");
        assert_eq!(tz("14"), "Etc/GMT-14");
    }

    #[test]
    fn resolve_tz_passes_through_iana() {
        assert_eq!(tz("America/Chicago"), "America/Chicago");
        assert_eq!(tz(":America/Chicago"), "America/Chicago");
    }

    #[test]
    fn resolve_tz_rejects_what_it_cannot_resolve() {
        use TzError::*;
        for (input, expected) in [
            ("Bogus/Zone", Unknown("Bogus/Zone".into())),
            ("", Unknown("".into())),
            ("America/New York", Unknown("America/New York".into())),
            ("+5:30", Unknown("+5:30".into())),
            ("15", OffsetOutOfRange("15".into())),
            ("-13", OffsetOutOfRange("-13".into())),
        ] {
            assert_eq!(resolve_tz(input), Err(expected), "{input}");
        }
        assert_eq!(
            resolve_tz("15").unwrap_err().to_string(),
            "offset out of range: 15 (expected -12..14)"
        );
        assert_eq!(
            resolve_tz("Bogus/Zone").unwrap_err().to_string(),
            "unknown timezone: Bogus/Zone"
        );
    }

    #[test]
    fn matches_returns_structured_info() {
        let m = matches(
            "2026-04-03T12:00:00Z",
            "America/Los_Angeles",
            None,
            None,
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
        let m = matches("15:30 PST", "UTC", None, None, true, None);
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
                Some("2026-07-15")
            ),
            "22:30 UTC"
        );
    }

    // --- B1: zone detection is an allowlist, not [A-Z]{2,4} ----------------

    fn tr_on(line: &str, to: &str, date: &str) -> String {
        translate(line, to, None, None, Some(date))
    }

    #[test]
    fn natively_known_abbreviations_keep_their_fixed_offset() {
        assert_eq!(tr_on("15:30 PST", "UTC", "2026-04-03"), "23:30 UTC");
        assert_eq!(tr_on("15:30 EST", "UTC", "2026-04-03"), "20:30 UTC");
    }

    #[test]
    fn alias_only_abbreviations_resolve_through_the_alias_table() {
        // Previously matched by [A-Z]{2,4}, then silently ignored by the
        // parser — a Tokyo time was treated as local and came out 9h wrong.
        assert_eq!(tr_on("15:30 JST", "UTC", "2026-04-03"), "06:30 UTC");
        assert_eq!(tr_on("15:30 CET", "UTC", "2026-01-15"), "14:30 UTC");
        assert_eq!(tr_on("15:30 CET", "UTC", "2026-07-15"), "13:30 UTC");
        assert_eq!(tr_on("15:30 AEST", "UTC", "2026-06-15"), "05:30 UTC");
    }

    #[test]
    fn log_levels_are_not_timezones() {
        assert_eq!(
            tr_on("15:30 INFO server started", "UTC", "2026-04-03"),
            "15:30 UTC INFO server started"
        );
        assert_eq!(
            tr_on("15:30 WARN disk low", "UTC", "2026-04-03"),
            "15:30 UTC WARN disk low"
        );
        assert_eq!(
            tr("2026-04-03 12:00:00 ERROR db failed", "UTC"),
            "2026-04-03 12:00:00 UTC ERROR db failed"
        );
    }

    // --- B6: DST transitions ----------------------------------------------

    #[test]
    fn ambiguous_wall_clock_takes_the_earlier_occurrence() {
        // A repeated hour resolves to the first (daylight) occurrence: jiff's
        // `compatible` disambiguation, which is also macOS date(1), Temporal,
        // RFC 5545 and ICU. Pinned so a jiff default change cannot move it by
        // an hour, twice a year, in every DST zone, without a test failing.
        let from = Some("America/New_York");
        assert_eq!(
            translate("2026-11-01 01:30:00", "UTC", from, None, None),
            "2026-11-01 05:30:00 UTC" // EDT (-4), not EST (-5)
        );
        assert_eq!(
            translate("01:30", "UTC", from, None, Some("2026-11-01")),
            "05:30 UTC"
        );
        assert_eq!(
            translate(
                "2026-10-25 02:30:00",
                "UTC",
                Some("Europe/Berlin"),
                None,
                None
            ),
            "2026-10-25 00:30:00 UTC" // CEST (+2), not CET (+1)
        );
    }

    #[test]
    fn nonexistent_wall_clock_springs_forward() {
        assert_eq!(
            translate(
                "2026-03-08 02:30:00",
                "UTC",
                Some("America/New_York"),
                None,
                None
            ),
            "2026-03-08 07:30:00 UTC"
        );
    }

    // --- B7: overflowing times normalize, impossible dates do not ----------

    #[test]
    fn legal_but_overflowing_times_normalize() {
        assert_eq!(tr("24:00 UTC", "UTC"), "00:00 UTC");
        assert_eq!(tr("23:59:60 UTC", "UTC"), "00:00:00 UTC");
    }

    #[test]
    fn genuinely_out_of_range_times_are_left_alone() {
        for input in ["25:00 UTC", "12:60 UTC", "24:00:01 UTC"] {
            assert_eq!(tr(input, "UTC"), input, "{input}");
        }
    }

    #[test]
    fn impossible_dates_pass_through_untouched() {
        // Rewriting 2026-02-30 to 2026-03-02 moves a logged event to another
        // day with no signal. Leave it exactly as found.
        for input in [
            "2026-02-30T12:00:00Z",
            "2026-02-29T12:00:00Z",
            "2026-13-03T12:00:00Z",
        ] {
            assert_eq!(tr(input, "America/Los_Angeles"), input, "{input}");
        }
        // ...and a real leap day still converts.
        assert_eq!(
            tr("2028-02-29T12:00:00Z", "UTC"),
            "2028-02-29T12:00:00Z",
            "2028 is a leap year"
        );
    }

    // --- B2: 12-hour clock ------------------------------------------------

    #[test]
    fn applies_the_meridiem() {
        for (input, expected) in [
            ("11:30:00 PM", "23:30:00 UTC"),
            ("12:30:00 AM", "00:30:00 UTC"),
            ("12:30:00 PM", "12:30:00 UTC"),
            ("1:00:00 PM", "13:00:00 UTC"),
            ("11:59:59 PM", "23:59:59 UTC"),
            ("12:00 AM", "00:00 UTC"),
            ("12:00 PM", "12:00 UTC"),
        ] {
            assert_eq!(tr(input, "UTC"), expected, "{input}");
        }
        assert_eq!(
            tr("2026-04-03 03:45:00 PM", "UTC"),
            "2026-04-03 15:45:00 UTC"
        );
    }

    #[test]
    fn a_twelve_hour_time_may_carry_a_zone() {
        assert_eq!(tr("3:45 PM PST", "UTC"), "23:45 UTC");
        assert_eq!(tr("3:45 pm pst", "UTC"), "23:45 UTC");
        assert_eq!(
            tr("2026-04-03 03:45:00 PM PST", "UTC"),
            "2026-04-03 23:45:00 UTC"
        );
    }

    #[test]
    fn a_dated_12_hour_time_may_drop_its_seconds() {
        // The date has to be part of the match: matching only `3:45 PM` leaves
        // the timestamp resolving its DST against *today* instead of the date
        // sitting right next to it.
        assert_eq!(tr("2026-04-03 3:45 PM", "UTC"), "2026-04-03 15:45:00 UTC");
        assert_eq!(tr("2026-04-03 03:45 PM", "UTC"), "2026-04-03 15:45:00 UTC");
    }

    #[test]
    fn meridiem_dots_are_stripped_not_parsed() {
        // Ruby's Time.parse reads a bare `P` as military zone P (-03:00), so
        // the dots have to come off before the hour is folded.
        assert_eq!(tr("11:30 p.m.", "UTC"), "23:30 UTC");
        assert_eq!(tr("11:30 P.M", "UTC"), "23:30 UTC");
        assert_eq!(tr("at 11:30 A.M. sharp", "UTC"), "at 11:30 UTC sharp");
        assert_eq!(tr("3:45 P.M. PST", "UTC"), "23:45 UTC");
    }

    #[test]
    fn an_undotted_meridiem_leaves_the_full_stop_to_the_sentence() {
        assert_eq!(
            tr("It starts at 11:30 PM.", "UTC"),
            "It starts at 23:30 UTC."
        );
    }

    #[test]
    fn a_lowercase_abbreviation_resolves_as_the_uppercase_one() {
        assert_eq!(tr("15:30 jst", "UTC"), "06:30 UTC");
        // pdt is a fixed -07:00 like PDT, even in January.
        assert_eq!(
            translate("12:00 pdt", "UTC", None, None, Some("2026-01-15")),
            "19:00 UTC"
        );
        let m = matches("12:00 pst", "UTC", None, None, true, None);
        assert_eq!(m[0].detected_tz.as_deref(), Some("pst"));
    }

    #[test]
    fn a_lowercase_abbreviation_that_is_also_a_word_is_left_alone() {
        // French "is" / "this", German "is": reading them as zones is a
        // silent wrong answer.
        assert_eq!(tr("à 15:30 est annulée", "UTC"), "à 15:30 UTC est annulée");
        assert_eq!(
            tr("à 15:30 cet après-midi", "UTC"),
            "à 15:30 UTC cet après-midi"
        );
        assert_eq!(tr("um 15:30 ist es", "UTC"), "um 15:30 UTC ist es");
        assert_eq!(tr("15:30 Pst", "UTC"), "15:30 UTC Pst");
    }

    #[test]
    fn meridiem_is_not_read_as_a_zone() {
        let m = matches("11:30:00 PM", "UTC", None, None, true, None);
        assert_eq!(m[0].original, "11:30:00 PM");
        assert_eq!(m[0].detected_tz, None);
    }

    #[test]
    fn zone_abbreviations_are_the_documented_union() {
        assert_eq!(ZONE_ABBREVIATIONS.len(), 30);
        for a in ZONE_ABBREVIATIONS {
            assert!(
                zone_offset_seconds(a).is_some() || abbrev_zone(a).is_some(),
                "{a} is detected but resolves to nothing"
            );
        }
        for junk in ["INFO", "WARN", "ERROR", "PM", "AM", "TODO"] {
            assert!(!ZONE_ABBREVIATIONS.contains(&junk), "{junk} is not a zone");
        }
    }

    #[test]
    fn detected_zone_is_the_one_that_converted() {
        let m = matches("15:30 JST", "UTC", None, None, false, None);
        assert_eq!(m[0].detected_tz.as_deref(), Some("JST"));
        let m = matches("15:30 INFO", "UTC", None, None, false, None);
        assert_eq!(m[0].original, "15:30");
        assert_eq!(m[0].detected_tz, None);
    }

    #[test]
    fn dateless_input_anchors_date_to_embedded_zone() {
        // "15:30 UTC" carries no date; the missing date is "today" in the
        // embedded zone (UTC), independent of the output zone. Regression: a
        // recognized embedded zone used to borrow the output zone's "today",
        // which lands a day off near a UTC midnight boundary. Rendered to a
        // fixed -05:00 offset (no DST, so the assertion holds year-round);
        // 15:30Z -> 10:30 on the same UTC date.
        let today = Zoned::now().with_time_zone(TimeZone::UTC).date();
        let expected = format!(
            "{:04}-{:02}-{:02} 10:30:00-05:00",
            today.year(),
            today.month(),
            today.day()
        );
        assert_eq!(
            translate("15:30 UTC", "Etc/GMT+5", None, Some(Format::Iso), None),
            expected
        );
    }
}

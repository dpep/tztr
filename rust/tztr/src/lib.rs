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
    /// The range or list this timestamp belongs to, if any.
    pub group: Option<Group>,
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

/// Every timestamp format as one alternation, most specific first. The regex
/// crate tries alternatives in order at each position, as Ruby does, so a
/// longer format wins over a shorter one inside it, and one pass converts every
/// timestamp on the line whatever its format. Mirrors `Tztr::TIMESTAMP`.
///
/// Byte-oriented so a line with a stray non-UTF-8 byte in it can still have its
/// timestamps converted without those bytes being rewritten. Unicode mode stays
/// on: `\b` has to agree with Ruby's, which is Unicode-aware.
fn timestamp() -> &'static BytesRegex {
    static RE: OnceLock<BytesRegex> = OnceLock::new();
    RE.get_or_init(|| {
        let zone = zone_alternation();
        // A trailing zone token: an allowlisted abbreviation or +HHMM within
        // the -12..+14 real zones occupy.
        let num = NUM_OFFSET;
        let tz = format!(r"(?:(?:{zone})\b|{num})");
        // Seconds and fractional seconds, both optional.
        let secs = r"(?::[0-9]{2}(?:\.[0-9]+)?)?";
        // A meridiem, optionally followed by a zone ("3:45 PM PST"). A
        // dotted one takes its closing dot; an undotted one leaves a
        // following full stop to the sentence.
        let mer = format!(r" ?[AaPp](?:\.[Mm]\.|\.?[Mm]\b)(?: ?{tz})?");
        // A dated clock's seconds: any fraction after a dot, or Python
        // logging's comma and three.
        let dsecs = r"(?::[0-9]{2}(?:\.[0-9]+|,[0-9]{3}\b)?)?";
        // A date, with dashes or slashes throughout (Go's log package, nginx).
        let date = r"[0-9]{4}(?:-[0-9]{2}-[0-9]{2}|/[0-9]{2}/[0-9]{2})";
        let alternation = [
            // nginx/Apache access log
            format!(r"\b[0-9]{{2}}/{MON}/[0-9]{{4}}:[0-9]{{2}}:[0-9]{{2}}:[0-9]{{2}} [+-][0-9]{{4}}\b"),
            // glibc's locale date(1)
            format!(
                r"\b{DAY} [0-9]{{1,2}} {MON} [0-9]{{4}} [0-9]{{1,2}}:[0-9]{{2}}:[0-9]{{2}}(?: [AP]M)? {}\b",
                date_zone()
            ),
            // date(1), ctime and ls -lT
            format!(
                r"\b(?:{DAY} )?{MON}  ?[0-9]{{1,2}} [0-9]{{1,2}}:[0-9]{{2}}:[0-9]{{2}} (?:{} )?[0-9]{{4}}\b",
                date_zone()
            ),
            // RFC 2822
            format!(
                r"\b(?:{DAY}, )?[0-9]{{1,2}} {MON} [0-9]{{4}} [0-9]{{2}}:[0-9]{{2}}(?::[0-9]{{2}})? (?:[+-][0-9]{{4}}\b|(?:{zone})\b)"
            ),
            // ISO 8601 with Z or offset
            format!(r"[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}T[0-9]{{1,2}}:[0-9]{{2}}{dsecs}(?:Z|[+-][0-9]{{2}}:?[0-9]{{2}})"),
            // ISO 8601 without timezone
            format!(r"[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}T[0-9]{{1,2}}:[0-9]{{2}}{dsecs}"),
            // Date space 12-hour time — above the with-tz pattern, and the
            // date must be part of the match or the time resolves its DST
            // against today instead of the date beside it.
            format!(r"{date} [0-9]{{1,2}}:[0-9]{{2}}{dsecs}{mer}"),
            // Date space time with tz
            format!(r"{date} [0-9]{{1,2}}:[0-9]{{2}}{dsecs} ?{tz}"),
            // Date space time
            format!(r"{date} [0-9]{{1,2}}:[0-9]{{2}}{dsecs}"),
            // 12-hour time
            format!(r"\b[0-9]{{1,2}}:[0-9]{{2}}{secs}{mer}"),
            // Time with tz. A numeric offset glued to the clock needs
            // seconds, or 15:30-1645 would read as one.
            format!(
                r"\b[0-9]{{1,2}}:[0-9]{{2}}(?::[0-9]{{2}}(?:\.[0-9]+)? ?{tz}| ?(?:{zone})\b| (?:{num}))"
            ),
            // Time with offset. Seconds required and the offset in range, so
            // the hyphen of a range like 15:30-16:45 is not read as one.
            r"\b[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?[+-](?:(?:0[0-9]|1[0-3]):?[0-5][0-9]|14:?00)\b".to_string(),
            // Hour with a meridiem: 9am, 9 PM PST
            format!(r"\b[0-9]{{1,2}}{mer}"),
            // Bare time
            format!(r"\b[0-9]{{1,2}}:[0-9]{{2}}{secs}\b"),
        ]
        .map(|p| format!("(?:{p})"))
        .join("|");
        BytesRegex::new(&alternation).expect("valid pattern")
    })
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
    RE.get_or_init(|| Regex::new(r"^[+-]?[0-9]{1,2}$").unwrap())
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

    let mut out = Vec::with_capacity(line.len());
    let mut pos = 0;
    for stamp in timestamps(line, false) {
        out.extend_from_slice(&line[pos..stamp.start]);
        let converted = convert_stamp(&stamp, from_tz.as_ref(), &to_tz, format, date);
        out.extend_from_slice(converted.as_deref().unwrap_or(stamp.text).as_bytes());
        pos = stamp.end;
    }
    out.extend_from_slice(&line[pos..]);
    out
}

/// A timestamp found in a line: where it sits, its text, what it is read as
/// (the text plus any zone or meridiem it shares with the end of its range or
/// list), and that range or list for `-j`.
struct Stamp<'a> {
    start: usize,
    end: usize,
    text: &'a str,
    effective: String,
    group: Option<Group>,
    /// Days after the reference date, for a later member of a range or list
    /// that is earlier on the clock.
    days: i64,
}

/// The range or list a timestamp belongs to, as `-j` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// `"range"` or `"list"`.
    pub kind: String,
    /// Every member's original text, in order.
    pub members: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Range,
    List,
}

/// The timestamps in a line, in order. A bare time beside one that names a
/// date, zone or meridiem is most likely a duration ("took 0:05"), and is left
/// out: that zone belongs to the timestamp naming it. Mirrors `Tztr.timestamps`.
fn timestamps(line: &[u8], with_groups: bool) -> Vec<Stamp<'_>> {
    let mut stamps = with_range_hours(line, scan_stamps(line));
    let joins: Vec<Option<Join>> = stamps
        .windows(2)
        .map(|w| join_between(line, &w[0], &w[1]))
        .collect();

    // Each member takes the zone and meridiem written after the one it joins:
    // "3:30 to 4:45 PM PST" starts at 3:30 PM PST. Walked backwards, so a chain
    // passes them all the way down.
    for i in (0..joins.len()).rev() {
        if joins[i].is_some() {
            let (date, clock) = split_date(&stamps[i].effective);
            stamps[i].effective = format!("{date}{}", range_start(clock, &stamps[i + 1].effective));
        }
    }

    // Forwards, each member in the same zone as the one before it takes that
    // one's date, and the next day if it is earlier on the clock: 11:30 PM to
    // 12:30 AM ends tomorrow.
    for i in 0..joins.len() {
        let (head, tail) = (&stamps[i], &stamps[i + 1]);
        if joins[i].is_none() || detect_zone(&head.effective) != detect_zone(&tail.effective) {
            continue;
        }
        let (date, clock) = split_date(&head.effective);
        let rolls = i64::from(minute_of_day(&tail.effective) < minute_of_day(clock));
        if date.is_empty() {
            stamps[i + 1].days = head.days + rolls;
        } else {
            let day = date[..10]
                .parse::<Date>()
                .ok()
                .and_then(|d| d.checked_add(Span::new().days(rolls)).ok());
            if let Some(day) = day {
                stamps[i + 1].effective = format!("{day} {}", tail.effective);
            }
        }
    }

    let mut group_ids = vec![0];
    for join in &joins {
        let last = *group_ids.last().unwrap();
        group_ids.push(if join.is_some() { last } else { last + 1 });
    }
    let mut kept: Vec<usize> = (0..stamps.len())
        .filter(|&i| !bare_time_re().is_match(&stamps[i].effective))
        .collect();
    if kept.is_empty() {
        kept = (0..stamps.len()).collect();
    }

    // Each group built once, in one pass: a line of thousands of stamps must
    // not cost a scan of the line per stamp.
    // Only -j reports groups; each member carries its own copy, so building
    // them for plain translation would clone every list once per member.
    if !with_groups {
        let mut stamps: Vec<Option<Stamp>> = stamps.into_iter().map(Some).collect();
        return kept.iter().map(|&i| stamps[i].take().unwrap()).collect();
    }

    let mut by_group: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for &i in &kept {
        by_group.entry(group_ids[i]).or_default().push(i);
    }
    let built: std::collections::BTreeMap<usize, Group> = by_group
        .into_iter()
        .filter(|(_, members)| members.len() >= 2)
        .map(|(id, members)| {
            let range = members[..members.len() - 1]
                .iter()
                .all(|&m| joins[m] == Some(Join::Range));
            let group = Group {
                kind: if range { "range" } else { "list" }.to_string(),
                members: members
                    .iter()
                    .map(|&m| stamps[m].text.to_string())
                    .collect(),
            };
            (id, group)
        })
        .collect();
    let groups: Vec<Option<Group>> = kept
        .iter()
        .map(|&i| built.get(&group_ids[i]).cloned())
        .collect();

    let mut stamps: Vec<Option<Stamp>> = stamps.into_iter().map(Some).collect();
    kept.iter()
        .zip(groups)
        .map(|(&i, group)| Stamp {
            group,
            ..stamps[i].take().unwrap()
        })
        .collect()
}

fn scan_stamps(line: &[u8]) -> Vec<Stamp<'_>> {
    timestamp()
        .find_iter(line)
        // A clock followed by a unit of time is a duration: "Finished in
        // 1:05 minutes".
        .filter(|m| {
            !(bare_time_re().is_match(ascii(m.as_bytes()))
                && duration_unit_re().is_match(&line[m.end()..]))
        })
        .map(|m| {
            let text = ascii(m.as_bytes());
            Stamp {
                start: m.start(),
                end: m.end(),
                text,
                effective: reading(text),
                group: None,
                days: 0,
            }
        })
        .collect()
}

const DAY: &str = "(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)";
const MONTH_ABBRS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MON: &str = "(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)";

/// Only units spelled out enough not to be a word of their own.
fn duration_unit_re() -> &'static BytesRegex {
    static RE: OnceLock<BytesRegex> = OnceLock::new();
    RE.get_or_init(|| {
        BytesRegex::new(r"(?i)^[ \t]+(?:secs?|seconds?|mins?|minutes?|hrs?|hours?)\b").unwrap()
    })
}

/// The zone a date(1)-shaped line may carry: any capitalized abbreviation --
/// one we can't resolve leaves the line alone rather than half-converted --
/// or a numeric one as tzdb writes it for zones without a name (`+03`).
fn date_zone() -> String {
    format!(
        "(?:{}|[A-Z]{{2,5}}|[+-][0-9]{{2}}(?:[0-9]{{2}})?)",
        zone_alternation()
    )
}

/// date(1)/ctime: `Fri Sep 25 22:14:42 PDT 2026`, weekday and zone optional
/// (`ls -lT` has neither).
fn unix_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?<weekday>{DAY} )?(?<mon>{MON})  ?(?<day>[0-9]{{1,2}}) (?<time>\S+) (?:(?<zone>\S+) )?(?<year>[0-9]{{4}})$"
        ))
        .unwrap()
    })
}

/// glibc's locale date(1): `Fri 25 Sep 2026 10:14:42 PM PDT`.
fn locale_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^{DAY} (?<day>[0-9]{{1,2}}) (?<mon>{MON}) (?<year>[0-9]{{4}}) (?<time>\S+)(?<mer> [AP]M)? (?<zone>\S+)$"
        ))
        .unwrap()
    })
}

/// nginx/Apache access log: `15/Jan/2015:12:31:01 -0700`.
fn clf_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?<day>[0-9]{{2}})/(?<mon>{MON})/(?<year>[0-9]{{4}}):(?<time>\S+) (?<zone>\S+)$"
        ))
        .unwrap()
    })
}

/// RFC 2822 / HTTP: `Fri, 25 Sep 2026 22:14:42 -0700`.
fn rfc_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^(?<weekday>{DAY}, )?(?<day>[0-9]{{1,2}}) (?<mon>{MON}) (?<year>[0-9]{{4}}) (?<time>\S+) (?<zone>\S+)$"
        ))
        .unwrap()
    })
}

/// What a timestamp is parsed as: a named-month date as YYYY-MM-DD, so the
/// weekday and day roll over with the clock; an hour alone as its o'clock
/// (9am is 9:00am); anything else as written. Mirrors `Tztr.reading`.
fn reading(text: &str) -> String {
    let named = [
        unix_date_re(),
        rfc_date_re(),
        locale_date_re(),
        clf_date_re(),
    ];
    if let Some(c) = named.iter().find_map(|re| re.captures(text)) {
        let month = MONTH_ABBRS.iter().position(|m| *m == &c["mon"]).unwrap() + 1;
        let day: u8 = c["day"].parse().unwrap();
        let time = &c["time"];
        let time = if time.matches(':').count() == 1 {
            format!("{time}:00")
        } else {
            time.to_string()
        };
        let mer = c.name("mer").map_or("", |m| m.as_str());
        let mut out = format!("{}-{month:02}-{day:02} {time}{mer}", &c["year"]);
        if let Some(zone) = c.name("zone") {
            out.push(' ');
            out.push_str(zone.as_str());
            // +03, as tzdb writes a zone without a name, is +0300.
            if zone.len() == 3 && zone.as_str().starts_with(['+', '-']) {
                out.push_str("00");
            }
        }
        out
    } else if let Some(c) = hour_only_re().captures(text) {
        format!("{}:00{}", &c[1], &text[c[1].len()..])
    } else {
        let text = slashed_date_re().replace(text, "$1-$2-");
        comma_frac_re().replace(&text, "$1.$2").into_owned()
    }
}

fn slashed_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([0-9]{4})/([0-9]{2})/").unwrap())
}

fn comma_frac_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(:[0-9]{2}),([0-9])").unwrap())
}

/// A bare hour is only a time as the start of a range whose end has a
/// meridiem: the 9 of "9-9:15am". Anywhere else it is just a number.
fn with_range_hours<'a>(line: &'a [u8], stamps: Vec<Stamp<'a>>) -> Vec<Stamp<'a>> {
    let mut out = Vec::with_capacity(stamps.len());
    let mut prev_end = 0;
    for stamp in stamps {
        let gap = &line[prev_end..stamp.start];
        let has_meridiem = time_parts_re()
            .captures(&stamp.effective)
            .is_some_and(|c| c.name("meridiem").is_some());
        if let Some(hour) = range_hour_re()
            .captures(gap)
            .filter(|_| has_meridiem)
            .map(|c| c.name("hour").unwrap())
            .filter(|h| !after_month_re().is_match(&gap[..h.start()]))
            .filter(|h| (1..=12).contains(&ascii(h.as_bytes()).parse::<u8>().unwrap_or(0)))
        {
            let text = ascii(hour.as_bytes());
            out.push(Stamp {
                start: prev_end + hour.start(),
                end: prev_end + hour.end(),
                text,
                effective: format!("{text}:00"),
                group: None,
                days: 0,
            });
        }
        prev_end = stamp.end;
        out.push(stamp);
    }
    out
}

/// How two neighbouring time-only timestamps are joined, if at all.
/// A reading split into its date with separator, if any, and its clock.
fn split_date(time: &str) -> (&str, &str) {
    match dated_prefix_re().find(time) {
        Some(m) => (m.as_str(), &time[m.end()..]),
        None => ("", time),
    }
}

fn dated_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}[T ]").unwrap())
}

fn join_between(line: &[u8], head: &Stamp, tail: &Stamp) -> Option<Join> {
    if !time_parts_re().is_match(split_date(&head.effective).1)
        || !time_parts_re().is_match(&tail.effective)
    {
        return None;
    }
    let gap = &line[head.end..tail.start];
    if range_join_re().is_match(gap) {
        Some(Join::Range)
    } else if list_join_re().is_match(gap) {
        Some(Join::List)
    } else {
        None
    }
}

fn minute_of_day(time: &str) -> u32 {
    let Some(c) = time_parts_re().captures(time) else {
        return 0;
    };
    let mut hour: u32 = c["hour"].parse().unwrap_or(0);
    if let Some(m) = c.name("meridiem") {
        hour = hour % 12
            + if m.as_str().starts_with(['P', 'p']) {
                12
            } else {
                0
            };
    }
    let minute: u32 = c["clock"]
        .split(':')
        .nth(1)
        .unwrap_or("0")
        .parse()
        .unwrap_or(0);
    hour * 60 + minute
}

fn range_start(head: &str, tail: &str) -> String {
    let (Some(h), Some(t)) = (
        time_parts_re().captures(head),
        time_parts_re().captures(tail),
    ) else {
        return head.to_string();
    };
    if h.name("zone").is_some() {
        return head.to_string();
    }

    let hour: u8 = h["hour"].parse().unwrap_or(0);
    let meridiem = h
        .name("meridiem")
        .map(|m| m.as_str())
        .or_else(|| shared_meridiem(hour, &t));
    [
        Some(&h["clock"]),
        meridiem,
        t.name("zone").map(|z| z.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

/// The end's meridiem, unless that would run the range backwards: 11:30 to
/// 1:00 PM starts in the morning. A 24-hour start takes none.
fn shared_meridiem(hour: u8, tail: &regex::Captures) -> Option<&'static str> {
    let meridiem = tail.name("meridiem")?;
    if !(1..=12).contains(&hour) {
        return None;
    }
    let tail_hour: u8 = tail["hour"].parse().unwrap_or(0);
    let pm = meridiem.as_str().starts_with(['P', 'p']) != (hour % 12 > tail_hour % 12);
    Some(if pm { "PM" } else { "AM" })
}

fn bare_time_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]{1,2}:[0-9]{2}(?::[0-9]{2}(?:\.[0-9]+)?)?$").unwrap())
}

fn hour_only_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([0-9]{1,2}) ?[AaPp]").unwrap())
}

/// A time-only timestamp in pieces, for sharing a range's zone and meridiem.
fn time_parts_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let zone = zone_alternation();
        Regex::new(&format!(
            r"^(?<clock>(?<hour>[0-9]{{1,2}}):[0-9]{{2}}(?::[0-9]{{2}}(?:\.[0-9]+)?)?)(?: ?(?<meridiem>[AaPp](?:\.[Mm]\.|\.?[Mm]\b)))?(?: ?(?<zone>(?:{zone})\b|{NUM_OFFSET}))?$"
        ))
        .unwrap()
    })
}

/// A numeric offset within the -12..+14 real zones occupy.
const NUM_OFFSET: &str = r"[+-](?:0[0-9]|1[0-3])[0-5][0-9]\b|[+-]1400\b";

const RANGE_WORDS: &str = "-|–|—|to|until|till|through|thru";

/// What joins the ends of a range: `15:30-16:45`, `3:30 to 4:45 PM`.
fn range_join_re() -> &'static BytesRegex {
    static RE: OnceLock<BytesRegex> = OnceLock::new();
    RE.get_or_init(|| BytesRegex::new(&format!(r"(?i)^[ \t]*(?:{RANGE_WORDS})[ \t]*$")).unwrap())
}

/// What joins the items of a list: `3:00, 4:00 or 5:00 PM`.
fn list_join_re() -> &'static BytesRegex {
    static RE: OnceLock<BytesRegex> = OnceLock::new();
    RE.get_or_init(|| BytesRegex::new(r"(?i)^[ \t]*(?:,|(?:,[ \t]*)?(?:or|and))[ \t]*$").unwrap())
}

/// A bare hour starting a range, the 9 of "9-10am" or "9 to 10am": at the
/// start of the text or after a space or "(", and hyphenated tight or joined
/// by a word. A spaced hyphen ("Room 7 - 3pm", "Apr 3 - 5pm") is not enough.
fn range_hour_re() -> &'static BytesRegex {
    static RE: OnceLock<BytesRegex> = OnceLock::new();
    RE.get_or_init(|| {
        BytesRegex::new(
            r"(?i)(?:^|[ \t(])(?<hour>[0-9]{1,2})(?:[-–—]|[ \t]+(?:to|until|till|through|thru)[ \t]+)$",
        )
        .unwrap()
    })
}

/// A day of the month, not an hour: the 1 of "Oct 1 thru 5pm".
fn after_month_re() -> &'static BytesRegex {
    static RE: OnceLock<BytesRegex> = OnceLock::new();
    RE.get_or_init(|| {
        BytesRegex::new(&format!(
            r"(?i)\b(?:{})[a-z]*\.?[ \t]*$",
            MONTH_ABBRS.join("|")
        ))
        .unwrap()
    })
}

/// Zone abbreviations written right after a timestamp but not read as one
/// because of their case (`Pst`), for `-v` to point out. Mirrors
/// `Tztr.ignored_zones`.
pub fn ignored_zones(line: &[u8]) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    for m in timestamp().find_iter(line) {
        let Some(c) = trailing_word_re().captures(&line[m.end()..]) else {
            continue;
        };
        let token = ascii(&c[1]);
        let mixed = token != token.to_uppercase() && token != token.to_lowercase();
        if mixed
            && ZONE_ABBREVIATIONS.contains(&token.to_uppercase().as_str())
            && !tokens.iter().any(|t| t == token)
        {
            tokens.push(token.to_string());
        }
    }
    tokens
}

fn trailing_word_re() -> &'static BytesRegex {
    static RE: OnceLock<BytesRegex> = OnceLock::new();
    RE.get_or_init(|| BytesRegex::new(r"^ ?([A-Za-z]{2,4})\b").unwrap())
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
    timestamps(line, true)
        .into_iter()
        .map(|stamp| {
            let translated = if detect {
                None
            } else {
                convert_stamp(&stamp, from_tz.as_ref(), &to_tz, format, date)
            };
            Match {
                original: stamp.text.to_string(),
                detected_format: detect_format(&reading(stamp.text)).to_string(),
                detected_tz: detect_zone(&stamp.effective),
                translated,
                group: stamp.group,
            }
        })
        .collect()
}

/// Parsed as its effective reading, formatted to mirror what was written.
fn convert_stamp(
    stamp: &Stamp,
    from_tz: Option<&TimeZone>,
    to_tz: &TimeZone,
    format: Option<Format>,
    date: Option<&str>,
) -> Option<String> {
    let zoned = parse(&stamp.effective, from_tz, to_tz, date, stamp.days)?;
    Some(format_time(&zoned, format, stamp.text))
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
    let readings: Vec<String> = timestamps(line, false)
        .into_iter()
        .map(|s| s.effective)
        .collect();
    Assumptions {
        date: readings.iter().any(|s| time_only_re().is_match(s)),
        zone: readings.iter().any(|s| detect_zone(s).is_none()),
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
    RE.get_or_init(|| Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T").unwrap())
}

fn date_space_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]{4}-[0-9]{2}-[0-9]{2} ").unwrap())
}

fn detect_zone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let zone = zone_alternation();
        Regex::new(&format!(r" ?({zone}|[+-][0-9]{{2}}:?[0-9]{{2}})$")).unwrap()
    })
}

// --- timestamp parsing -----------------------------------------------------

fn components_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        let zone = zone_alternation();
        Regex::new(&format!(
            r"^(?:(?<y>[0-9]{{4}})-(?<mo>[0-9]{{2}})-(?<d>[0-9]{{2}})[T ])?(?<h>[0-9]{{1,2}}):(?<mi>[0-9]{{2}})(?::(?<s>[0-9]{{2}})(?:\.(?<frac>[0-9]+))?)?(?: ?(?<mer>[AaPp])\.?[Mm]\.?)?\s?(?<zone>{zone}|[+-][0-9]{{2}}:?[0-9]{{2}}|[+-][0-9]{{4}})?$",
        ))
        .unwrap()
    })
}

fn time_only_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]{1,2}:").unwrap())
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
    days: i64,
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
    let anchor = anchor(zone_token, from_tz, to_tz)?;

    let (year, month, day) = match (group("y"), group("mo"), group("d")) {
        (Some(y), Some(mo), Some(d)) => (y.parse().ok()?, mo.parse().ok()?, d.parse().ok()?),
        _ => today_in(&anchor),
    };
    // A later member of a range or list that rolled past midnight.
    let day = Date::new(year, month, day)
        .ok()?
        .checked_add(Span::new().days(days))
        .ok()?;

    let civil = civil_datetime(day, hour, minute, second, nanos)?;
    let instant = civil.to_zoned(anchor).ok()?;

    Some(instant.with_time_zone(to_tz.clone()))
}

/// Place a wall-clock reading on `date`, carrying the two overflows a clock
/// legitimately produces: `24:00`, the midnight that ends a day, and `:60`, a
/// leap second. Anything further out of range (`25:00`, `12:60`) is not a time
/// at all — the caller leaves that text exactly as it found it.
/// The zone a timestamp's wall clock is read in: the zone it names, else the
/// source zone, else the output zone. For a dateless one, also the zone whose
/// "today" fills the missing date -- where it was written.
fn anchor(zone_token: &str, from_tz: Option<&TimeZone>, to_tz: &TimeZone) -> Option<TimeZone> {
    if zone_token.is_empty() {
        return Some(from_tz.unwrap_or(to_tz).clone());
    }
    match zone_offset_seconds(zone_token) {
        Some(off) => Some(TimeZone::fixed(Offset::from_seconds(off).ok()?)),
        // ET, CT, MT, PT follow DST; resolve them as zones.
        None => abbrev_zone(zone_token),
    }
}

/// The date `-v` says it assumed for this line's first dateless timestamp:
/// today where it was written. Mirrors `Tztr.assumed_date`.
pub fn assumed_date(line: &[u8], from: Option<&str>, to: &str) -> Option<String> {
    let stamp = timestamps(line, false)
        .into_iter()
        .find(|s| time_only_re().is_match(&s.effective))?;
    let to_tz = resolve_zone(to);
    let from_tz = from.map(resolve_zone);
    let token = detect_zone(&stamp.effective).unwrap_or_default();
    let (y, m, d) = today_in(&anchor(&token, from_tz.as_ref(), &to_tz)?);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

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
        // Standard and daylight abbreviations are fixed whatever the date --
        // CEST is +02:00 even in January. Only ET, CT, MT, PT follow DST.
        "HST" => Some(-10 * 3600),
        "AKST" => Some(-9 * 3600),
        "AKDT" => Some(-8 * 3600),
        "CET" | "BST" => Some(3600),
        "CEST" => Some(2 * 3600),
        "IST" => Some(5 * 3600 + 1800),
        "JST" | "KST" => Some(9 * 3600),
        "HKT" => Some(8 * 3600),
        "AEST" => Some(10 * 3600),
        "AEDT" => Some(11 * 3600),
        "NZST" => Some(12 * 3600),
        "NZDT" => Some(13 * 3600),
        _ => None,
    }
}

fn numeric_zone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[+-][0-9]{2}:?[0-9]{2}$").unwrap())
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

/// An hour alone, as a range's start (`9`) or with a meridiem (`9am`).
fn hour_alone_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]{1,2}(?:$| ?[AaPp])").unwrap())
}

fn hour_minute_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]{1,2}:[0-9]{2}").unwrap())
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
        format!(
            "{}{}{tz}",
            strf(zoned, "%Y-%m-%dT"),
            written_clock(zoned, original)
        )
    } else if let Some(c) = any_date_space_re().captures(original) {
        let sep = &c[1];
        format!(
            "{} {} {abbrev}",
            strf(zoned, &format!("%Y{sep}%m{sep}%d")),
            written_clock(zoned, original)
        )
    } else if clf_date_re().is_match(original) {
        strf(zoned, "%d/%b/%Y:%H:%M:%S %z")
    } else if let Some(c) = unix_date_re().captures(original) {
        let weekday = if c.name("weekday").is_some() {
            "%a "
        } else {
            ""
        };
        format!(
            "{} {} {}",
            strf(zoned, &format!("{weekday}%b %e %H:%M:%S")),
            written_zone(zoned, c.name("zone").map(|z| z.as_str())),
            strf(zoned, "%Y")
        )
    } else if let Some(c) = locale_date_re().captures(original) {
        let day = if c["day"].len() == 2 { "%d" } else { "%-d" };
        let clock = if c.name("mer").is_some() {
            "%I:%M:%S %p"
        } else {
            "%H:%M:%S"
        };
        format!(
            "{} {}",
            strf(zoned, &format!("%a {day} %b %Y {clock}")),
            written_zone(zoned, Some(&c["zone"]))
        )
    } else if let Some(c) = rfc_date_re().captures(original) {
        let weekday = if c.name("weekday").is_some() {
            "%a, "
        } else {
            ""
        };
        let day = if c["day"].len() == 2 { "%d" } else { "%-d" };
        let secs = if c["time"].matches(':').count() == 2 {
            ":%S"
        } else {
            ""
        };
        let zone = if c["zone"].starts_with(['+', '-']) {
            strf(zoned, "%z")
        } else {
            abbrev
        };
        format!(
            "{} {zone}",
            strf(zoned, &format!("{weekday}{day} %b %Y %H:%M{secs}"))
        )
    } else if hour_minute_re().is_match(original) {
        format!("{} {abbrev}", written_clock(zoned, original))
    } else if hour_alone_re().is_match(original) {
        format!("{} {abbrev}", strf(zoned, "%H:%M"))
    } else {
        format!("{} {abbrev}", strf(zoned, "%Y-%m-%d %H:%M:%S"))
    }
}

/// A clock to the precision it was written with, fraction and its separator
/// included: `22:14:42,123` stays three digits after a comma. Mirrors
/// `Tztr.written_clock`.
fn written_clock(zoned: &Zoned, original: &str) -> String {
    let Some(c) = clock_re().captures(original) else {
        return strf(zoned, "%H:%M");
    };
    if c.name("secs").is_none() {
        return strf(zoned, "%H:%M");
    }
    let mut out = strf(zoned, "%H:%M:%S");
    if let Some(frac) = c.name("frac") {
        let digits = frac.len() - 1;
        let nanos = format!("{:09}", zoned.subsec_nanosecond());
        out.push_str(&frac.as_str()[..1]);
        out.push_str(&nanos[..digits.min(9)]);
    }
    out
}

fn clock_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[0-9]{1,2}:[0-9]{2}(?<secs>:[0-9]{2}(?<frac>[.,][0-9]+)?)?").unwrap()
    })
}

/// A zone written back the way the input wrote it: numeric if it was.
fn written_zone(zoned: &Zoned, zone: Option<&str>) -> String {
    if zone.is_some_and(|z| z.starts_with(['+', '-'])) {
        strf(zoned, "%z")
    } else {
        zoned.strftime("%Z").to_string()
    }
}

fn any_date_space_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[0-9]{4}([-/])[0-9]{2}[-/][0-9]{2} ").unwrap())
}

fn strf(zoned: &Zoned, fmt: &str) -> String {
    zoned.strftime(fmt).to_string()
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
    fn converts_every_timestamp_on_a_line_whatever_their_formats() {
        let line = r#"{"ts":"2026-04-03T12:00:00Z","created":"2026-04-03 13:00:00 UTC","msg":"at 15:30 UTC"}"#;
        assert_eq!(
            translate(line, "America/Los_Angeles", None, None, Some("2026-04-03")),
            r#"{"ts":"2026-04-03T05:00:00-07:00","created":"2026-04-03 06:00:00 PDT","msg":"at 08:30 PDT"}"#
        );
    }

    #[test]
    fn reports_every_timestamp_on_a_line_in_order() {
        let m = matches(
            "15:30 UTC then 2026-04-03T12:00:00Z",
            "UTC",
            None,
            None,
            true,
            None,
        );
        let formats: Vec<_> = m.iter().map(|m| m.detected_format.as_str()).collect();
        assert_eq!(formats, ["time", "iso"]);

        let m = matches("2026-04-03 12:00:00 UTC", "UTC", None, None, true, None);
        assert_eq!(
            m.len(),
            1,
            "a shorter format must not match inside a longer one"
        );
    }

    #[test]
    fn leaves_a_bare_time_alone_beside_a_timestamp_with_a_date_or_zone() {
        // Most likely a duration: the zone belongs to the timestamp naming it.
        assert_eq!(
            tr("2026-04-03T12:00:00Z took 0:05", "America/Los_Angeles"),
            "2026-04-03T05:00:00-07:00 took 0:05"
        );
        assert_eq!(
            translate(
                "15:30 UTC, retry in 0:30",
                "America/Los_Angeles",
                None,
                None,
                Some("2026-04-03")
            ),
            "08:30 PDT, retry in 0:30"
        );
        let m = matches(
            "2026-04-03T12:00:00Z took 0:05",
            "UTC",
            None,
            None,
            true,
            None,
        );
        assert_eq!(m.len(), 1);
        let a = assumptions(b"2026-04-03T12:00:00Z took 0:05");
        assert!(!a.date && !a.zone);
    }

    #[test]
    fn still_converts_bare_times_when_nothing_on_the_line_is_more_specific() {
        assert_eq!(
            translate(
                "from 15:30 to 16:45",
                "UTC",
                Some("America/Los_Angeles"),
                None,
                Some("2026-04-03")
            ),
            "from 22:30 UTC to 23:45 UTC"
        );
    }

    #[test]
    fn a_meridiem_is_specific_enough_to_leave_a_bare_time_alone() {
        assert_eq!(
            tr("meeting 3:30 PM, took 0:05", "UTC"),
            "meeting 15:30 UTC, took 0:05"
        );
    }

    fn range(line: &str) -> String {
        translate(
            line,
            "UTC",
            Some("America/Los_Angeles"),
            None,
            Some("2026-04-03"),
        )
    }

    #[test]
    fn the_start_of_a_range_takes_the_zone_after_its_end() {
        assert_eq!(
            range("from 15:30 to 16:45 PST"),
            "from 23:30 UTC to 00:45 UTC"
        );
        assert_eq!(range("15:30-16:45 PST"), "23:30 UTC-00:45 UTC");
        assert_eq!(range("15:30 – 16:45 JST"), "06:30 UTC – 07:45 UTC");
    }

    #[test]
    fn the_start_of_a_range_takes_the_meridiem_unless_that_runs_it_backwards() {
        assert_eq!(range("from 3:30 to 4:45 PM"), "from 22:30 UTC to 23:45 UTC");
        assert_eq!(
            range("from 3:30 to 4:45 PM PST"),
            "from 23:30 UTC to 00:45 UTC"
        );
        assert_eq!(range("11:30 to 1:00 PM PST"), "19:30 UTC to 21:00 UTC");
        assert_eq!(
            range("10:00 until 2:00 AM PST"),
            "06:00 UTC until 10:00 UTC"
        );
        assert_eq!(
            range("from 15:30 to 4:45 PM PST"),
            "from 23:30 UTC to 00:45 UTC"
        );
    }

    #[test]
    fn the_start_of_a_range_reports_the_shared_zone() {
        let m = matches("from 3:30 to 4:45 PM PST", "UTC", None, None, true, None);
        let got: Vec<_> = m
            .iter()
            .map(|m| (m.original.as_str(), m.detected_tz.as_deref()))
            .collect();
        assert_eq!(got, [("3:30", Some("PST")), ("4:45 PM PST", Some("PST"))]);
    }

    #[test]
    fn only_a_range_or_list_shares_not_other_words() {
        assert_eq!(range("15:30, then 16:45 PST"), "15:30, then 00:45 UTC");
    }

    #[test]
    fn a_list_shares_the_trailing_zone_and_meridiem() {
        assert_eq!(
            range("Options at 3:00, 4:00 or 5:00 PM PST"),
            "Options at 23:00 UTC, 00:00 UTC or 01:00 UTC"
        );
        assert_eq!(range("at 3:00 and 4:00 PM"), "at 22:00 UTC and 23:00 UTC");
        assert_eq!(
            range("11:00, 12:00, or 1:00 PM PST"),
            "19:00 UTC, 20:00 UTC, or 21:00 UTC"
        );
    }

    #[test]
    fn every_member_of_a_range_or_list_is_listed() {
        let group = |kind: &str, members: &[&str]| {
            Some(Group {
                kind: kind.to_string(),
                members: members.iter().map(|m| m.to_string()).collect(),
            })
        };
        let groups = |line| -> Vec<Option<Group>> {
            matches(line, "UTC", None, None, true, None)
                .into_iter()
                .map(|m| m.group)
                .collect()
        };

        let r = group("range", &["3:30", "4:45 PM PST"]);
        assert_eq!(groups("from 3:30 to 4:45 PM PST"), [r.clone(), r]);
        let l = group("list", &["3:00", "4:00", "5:00 PM"]);
        assert_eq!(groups("3:00, 4:00 or 5:00 PM"), [l.clone(), l.clone(), l]);
        assert_eq!(groups("15:30 UTC"), [None]);
        assert_eq!(groups("2026-04-03T12:00:00Z,15:30"), [None]);
    }

    fn named(line: &str, to: &str) -> String {
        translate(line, to, Some("America/Los_Angeles"), None, None)
    }

    #[test]
    fn date_1_output_converts_as_one_timestamp() {
        let ny = "America/New_York";
        assert_eq!(
            named("Fri Sep 25 22:14:42 PDT 2026", ny),
            "Sat Sep 26 01:14:42 EDT 2026"
        );
        assert_eq!(
            named("Fri Sep 25 22:14:42 2026", ny),
            "Sat Sep 26 01:14:42 EDT 2026"
        );
        assert_eq!(
            named("Sat Sep  5 22:14:42 UTC 2026", "America/Los_Angeles"),
            "Sat Sep  5 15:14:42 PDT 2026"
        );
        assert_eq!(
            named("Mon Feb 30 12:00:00 UTC 2026", ny),
            "Mon Feb 30 12:00:00 UTC 2026"
        );
    }

    #[test]
    fn an_rfc_2822_date_keeps_its_shape() {
        let ny = "America/New_York";
        assert_eq!(
            named("Fri, 25 Sep 2026 22:14:42 -0700", ny),
            "Sat, 26 Sep 2026 01:14:42 -0400"
        );
        assert_eq!(named("25 Sep 2026 22:14 PDT", ny), "26 Sep 2026 01:14 EDT");
        assert_eq!(
            named("Date: Sat, 5 Sep 2026 12:00:00 GMT", "UTC"),
            "Date: Sat, 5 Sep 2026 12:00:00 UTC"
        );
    }

    #[test]
    fn a_named_month_date_is_one_dated_match() {
        let m = matches(
            "Fri Sep 25 22:14:42 PDT 2026",
            "UTC",
            None,
            None,
            true,
            None,
        );
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].original, "Fri Sep 25 22:14:42 PDT 2026");
        assert_eq!(m[0].detected_format, "datetime");
        assert_eq!(m[0].detected_tz.as_deref(), Some("PDT"));
    }

    fn log(line: &str, to: &str, from: Option<&str>) -> String {
        translate(line, to, from, None, None)
    }

    #[test]
    fn an_access_log_timestamp_converts_date_and_all() {
        assert_eq!(
            log(
                r#"127.0.0.1 - - [15/Jan/2015:12:31:01 -0700] "GET / HTTP/1.1" 200 1"#,
                "America/New_York",
                None
            ),
            r#"127.0.0.1 - - [15/Jan/2015:14:31:01 -0500] "GET / HTTP/1.1" 200 1"#
        );
        assert_eq!(
            log("[07/Apr/2015:23:31:01 -0700]", "UTC", None),
            "[08/Apr/2015:06:31:01 +0000]"
        );
    }

    #[test]
    fn a_slashed_date_converts() {
        assert_eq!(
            log(
                "2026/09/25 23:14:42 main.go:42: started",
                "UTC",
                Some("Etc/GMT+7")
            ),
            "2026/09/26 06:14:42 UTC main.go:42: started"
        );
        assert_eq!(
            log("2026/09/25 23:14:42 PDT", "UTC", None),
            "2026/09/26 06:14:42 UTC"
        );
    }

    #[test]
    fn ctime_without_a_weekday_or_with_a_numeric_zone_converts() {
        assert_eq!(
            log(
                "staff 100 Sep 25 23:40:39 2026 file.txt",
                "UTC",
                Some("Etc/GMT+7")
            ),
            "staff 100 Sep 26 06:40:39 UTC 2026 file.txt"
        );
        assert_eq!(
            log("Fri Sep 25 23:40:39 -0700 2026", "UTC", None),
            "Sat Sep 26 06:40:39 +0000 2026"
        );
        assert_eq!(
            log("Fri Sep 25 22:14:42 +03 2026", "UTC", None),
            "Fri Sep 25 19:14:42 +0000 2026"
        );
        assert_eq!(
            log("Fri Sep 25 22:14:42 WIB 2026", "UTC", None),
            "Fri Sep 25 22:14:42 WIB 2026"
        );
    }

    #[test]
    fn glibc_locale_date_output_converts() {
        let ny = "America/New_York";
        assert_eq!(
            log("Fri 25 Sep 2026 10:14:42 PM PDT", ny, None),
            "Sat 26 Sep 2026 01:14:42 AM EDT"
        );
        assert_eq!(
            log("Fri 25 Sep 2026 22:14:42 PDT", ny, None),
            "Sat 26 Sep 2026 01:14:42 EDT"
        );
    }

    #[test]
    fn fractions_keep_their_digits_and_separator() {
        assert_eq!(
            log(
                "2026-09-25 22:14:42,123 INFO x",
                "America/New_York",
                Some("UTC")
            ),
            "2026-09-25 18:14:42,123 EDT INFO x"
        );
        assert_eq!(
            log("[2026-04-03 12:00:00,123] ok", "UTC", Some("UTC")),
            "[2026-04-03 12:00:00,123 UTC] ok"
        );
        assert_eq!(
            log("2026-09-25T22:14:42.123456789Z", "America/New_York", None),
            "2026-09-25T18:14:42.123456789-04:00"
        );
        assert_eq!(
            log("2026-09-25 22:14:42.5 UTC", "UTC", None),
            "2026-09-25 22:14:42.5 UTC"
        );
        assert_eq!(
            tr_on("12:34:56.25 UTC", "UTC", "2026-04-03"),
            "12:34:56.25 UTC"
        );
    }

    #[test]
    fn a_time_followed_by_a_unit_is_a_duration() {
        assert_eq!(
            tr(
                "Finished in 1:05 minutes (files took 2.3 seconds to load)",
                "UTC"
            ),
            "Finished in 1:05 minutes (files took 2.3 seconds to load)"
        );
        assert_eq!(tr("took 2:30 hrs", "UTC"), "took 2:30 hrs");
        assert_eq!(
            tr("at 3:05 PM, took 0:30 sec", "UTC"),
            "at 15:05 UTC, took 0:30 sec"
        );
        assert_eq!(
            tr_on("15:30 meeting", "UTC", "2026-04-03"),
            "15:30 UTC meeting"
        );
        assert_eq!(tr_on("10:30 h", "UTC", "2026-04-03"), "10:30 UTC h");
    }

    #[test]
    fn an_hour_with_a_meridiem_is_a_time() {
        assert_eq!(range("Meeting at 9am PST"), "Meeting at 17:00 UTC");
        assert_eq!(range("at 9 PM"), "at 04:00 UTC");
        assert_eq!(range("at 9 p.m. sharp"), "at 04:00 UTC sharp");
    }

    #[test]
    fn a_bare_hour_starts_a_range_whose_end_has_a_meridiem() {
        assert_eq!(range("Standup 9-9:15am PST"), "Standup 17:00 UTC-17:15 UTC");
        assert_eq!(range("9 to 10am PST"), "17:00 UTC to 18:00 UTC");
        assert_eq!(range("11-1pm PST"), "19:00 UTC-21:00 UTC");
        let m = matches("9-9:15am PST", "UTC", None, None, true, None);
        let originals: Vec<_> = m.iter().map(|m| m.original.as_str()).collect();
        assert_eq!(originals, ["9", "9:15am PST"]);
    }

    #[test]
    fn a_date_or_label_is_not_the_start_of_a_range() {
        assert_eq!(
            range("Deadline: Friday, Apr 3 - 5pm PST"),
            "Deadline: Friday, Apr 3 - 01:00 UTC"
        );
        assert_eq!(range("Due 4/3 - 5pm PST"), "Due 4/3 - 01:00 UTC");
        assert_eq!(range("2026-04-03 - 10am"), "2026-04-03 - 17:00 UTC");
        assert_eq!(range("Oct 1 thru 5pm"), "Oct 1 thru 00:00 UTC");
        assert_eq!(range("Ticket #12 - 9:30am"), "Ticket #12 - 16:30 UTC");
        assert_eq!(range("Room 7 - 3pm"), "Room 7 - 22:00 UTC");
    }

    #[test]
    fn digits_are_ascii_digits() {
        // Fullwidth and Arabic-Indic digits are not timestamps, and used to
        // panic on the way through the parser.
        let line = "Fri Sep \u{ff12}\u{ff15} 22:14:42 2026";
        assert_eq!(
            tr(line, "UTC"),
            "Fri Sep \u{ff12}\u{ff15} 22:14:42 UTC 2026"
        );
        let line = "\u{662}\u{665} Sep 2026 22:14 GMT";
        assert_eq!(tr(line, "UTC"), "\u{662}\u{665} Sep 2026 22:14 UTC");
        assert!(matches("\u{ff11}\u{ff12}:30 UTC", "UTC", None, None, true, None).is_empty());
    }

    #[test]
    fn a_named_month_date_is_not_an_hour() {
        // "15 Apr" and "01 Aug" are not "15 am".
        assert_eq!(
            named("Date: 15 Apr 2026 22:14:42 -0700", "Asia/Tokyo"),
            "Date: 16 Apr 2026 14:14:42 +0900"
        );
        assert_eq!(
            named("01 Aug 2026 10:00 GMT", "Asia/Tokyo"),
            "01 Aug 2026 19:00 JST"
        );
    }

    #[test]
    fn other_bare_numbers_are_left_alone() {
        assert_eq!(range("won 3-10"), "won 3-10");
        assert_eq!(range("page 9, 10am standup"), "page 9, 17:00 UTC standup");
        assert_eq!(range("v1.9-10am"), "v1.9-17:00 UTC");
        assert_eq!(range("item 42-10am"), "item 42-17:00 UTC");
    }

    #[test]
    fn names_a_zone_ignored_for_its_case() {
        assert_eq!(ignored_zones(b"15:30 Pst and 16:00 Pst"), ["Pst"]);
        assert!(ignored_zones(b"a 15:30 est annulee").is_empty());
        assert!(ignored_zones(b"15:30 PST").is_empty());
    }

    #[test]
    fn an_offset_needs_seconds_so_a_hyphenated_range_stays_a_range() {
        let tr = |l| translate(l, "UTC", None, None, Some("2026-04-03"));
        assert_eq!(tr("12:34:56-05:00"), "17:34:56 UTC");
        assert_eq!(tr("15:30-16:45"), "15:30 UTC-16:45 UTC");
        assert_eq!(tr("12:34:56-16:45"), "12:34:56 UTC-16:45 UTC");
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
    fn a_standard_or_daylight_abbreviation_is_the_offset_it_names() {
        // Previously matched by [A-Z]{2,4}, then silently ignored by the
        // parser — a Tokyo time was treated as local and came out 9h wrong.
        assert_eq!(tr_on("15:30 JST", "UTC", "2026-04-03"), "06:30 UTC");
        // CEST is +02:00 even in January, CET +01:00 even in July.
        assert_eq!(tr_on("15:30 CET", "UTC", "2026-07-15"), "14:30 UTC");
        assert_eq!(tr_on("15:30 CEST", "UTC", "2026-01-15"), "13:30 UTC");
        assert_eq!(tr_on("15:30 BST", "UTC", "2026-01-15"), "14:30 UTC");
        assert_eq!(tr_on("15:30 AEST", "UTC", "2026-01-15"), "05:30 UTC");
        assert_eq!(tr_on("15:30 NZDT", "UTC", "2026-07-15"), "02:30 UTC");
        assert_eq!(tr_on("15:30 IST", "UTC", "2026-01-15"), "10:00 UTC");
        assert_eq!(tr("2026-07-15 15:30 CET", "UTC"), "2026-07-15 14:30 UTC");
    }

    #[test]
    fn a_generic_abbreviation_follows_dst() {
        assert_eq!(tr_on("15:30 PT", "UTC", "2026-01-15"), "23:30 UTC");
        assert_eq!(tr_on("15:30 PT", "UTC", "2026-07-15"), "22:30 UTC");
    }

    #[test]
    fn a_dated_start_shares_its_date_and_takes_the_ends_zone() {
        assert_eq!(
            tr("2026-04-03 9:00 AM - 10:00 AM PST", "UTC"),
            "2026-04-03 17:00 UTC - 18:00 UTC"
        );
        assert_eq!(
            translate(
                "2026-04-03 15:30 - 16:30",
                "America/Los_Angeles",
                Some("UTC"),
                None,
                None
            ),
            "2026-04-03 08:30 PDT - 09:30 PDT"
        );
        assert_eq!(
            translate(
                "2026-04-03 23:00 - 01:00",
                "UTC",
                Some("UTC"),
                Some(Format::Iso),
                None
            ),
            "2026-04-03 23:00:00Z - 2026-04-04 01:00:00Z"
        );
    }

    #[test]
    fn a_dated_single_digit_hour_keeps_its_date() {
        assert_eq!(
            tr("2026-01-15 9:00 UTC", "Etc/GMT+12"),
            "2026-01-14 21:00 -12"
        );
        assert_eq!(
            tr("2026-01-15T9:00:00Z", "Etc/GMT+12"),
            "2026-01-14T21:00:00-12:00"
        );
    }

    #[test]
    fn a_glued_offset_needs_seconds_and_range() {
        let tr = |l| translate(l, "UTC", None, None, Some("2026-04-03"));
        assert_eq!(tr("15:30-1645"), "15:30 UTC-1645");
        assert_eq!(tr("12:00+0530"), "12:00 UTC+0530");
        assert_eq!(tr("12:00 +0530"), "06:30 UTC");
        assert_eq!(tr("12:00:00-14:59"), "12:00:00 UTC-14:59 UTC");
        assert_eq!(tr("12:00:00+14:00"), "22:00:00 UTC");
        assert_eq!(
            translate("2026-04-03 12:00:00 -1645", "UTC", None, None, None),
            "2026-04-03 12:00:00 UTC -1645"
        );
    }

    #[test]
    fn impossible_clocks_are_left_alone() {
        assert_eq!(
            tr("2026-09-25 99:14:42 PDT", "UTC"),
            "2026-09-25 99:14:42 PDT"
        );
        assert_eq!(tr_on("99am", "UTC", "2026-04-03"), "99am");
        assert_eq!(
            tr("Fri Sep 25 25:14:42 PDT 2026", "UTC"),
            "Fri Sep 25 25:14:42 PDT 2026"
        );
    }

    #[test]
    fn a_range_past_midnight_is_an_hour_long_whatever_today_is_where() {
        for to in ["Etc/GMT-14", "Etc/GMT+12", "Asia/Tokyo"] {
            let out = translate(
                "11:30 PM to 12:30 AM PST",
                to,
                None,
                Some(Format::Iso),
                None,
            );
            let (start, end) = out.split_once(" to ").unwrap();
            let parse = |t: &str| {
                jiff::fmt::strtime::parse("%Y-%m-%d %H:%M:%S%:z", t)
                    .unwrap()
                    .to_timestamp()
                    .unwrap()
            };
            let hour = parse(end).duration_since(parse(start));
            assert_eq!(hour.as_secs(), 3600, "{to}: {out}");
        }
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
        assert_eq!(tr("2026-04-03 3:45 PM", "UTC"), "2026-04-03 15:45 UTC");
        assert_eq!(tr("2026-04-03 03:45 PM", "UTC"), "2026-04-03 15:45 UTC");
    }

    #[test]
    fn a_dated_timestamp_without_seconds_keeps_its_date() {
        assert_eq!(
            translate(
                "2026-01-15 23:30",
                "America/Los_Angeles",
                Some("UTC"),
                None,
                None
            ),
            "2026-01-15 15:30 PST"
        );
        assert_eq!(
            tr("2026-01-15 23:30 UTC", "America/Los_Angeles"),
            "2026-01-15 15:30 PST"
        );
    }

    #[test]
    fn minute_precision_iso_converts() {
        assert_eq!(
            tr("2026-12-31T23:30+05:30", "Pacific/Auckland"),
            "2027-01-01T07:00+13:00"
        );
        assert_eq!(
            tr("2026-12-31T23:30Z", "Pacific/Auckland"),
            "2027-01-01T12:30+13:00"
        );
        assert_eq!(
            translate("2026-12-31T23:30", "UTC", Some("UTC"), None, None),
            "2026-12-31T23:30Z"
        );
    }

    #[test]
    fn a_range_crossing_midnight_ends_the_next_day() {
        let iso = |l| translate(l, "UTC", None, Some(Format::Iso), Some("2026-04-03"));
        assert_eq!(
            iso("11:30 PM to 12:30 AM PST"),
            "2026-04-04 07:30:00Z to 2026-04-04 08:30:00Z"
        );
        assert_eq!(
            iso("11:30 PM, 12:15 AM or 1:00 AM PST"),
            "2026-04-04 07:30:00Z, 2026-04-04 08:15:00Z or 2026-04-04 09:00:00Z"
        );
        assert_eq!(
            iso("22:00-02:00 UTC"),
            "2026-04-03 22:00:00Z-2026-04-04 02:00:00Z"
        );
        assert_eq!(
            iso("9:00 to 17:00 UTC"),
            "2026-04-03 09:00:00Z to 2026-04-03 17:00:00Z"
        );
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

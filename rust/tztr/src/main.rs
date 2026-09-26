//! `tztr` CLI — Rust port of `bin/tztr`. Kept functionally identical to the
//! Ruby reference (same flags, output, and behavior); see CLAUDE.md.

use jiff::civil::Date;
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::process::ExitCode;

use tztr::{
    assumed_date, assumptions, ignored_zones, matches_bytes, resolve_tz, timezone_aliases,
    translate_bytes, Assumptions, Format, Match,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
Usage: tztr [options] [file]

Timezone Translator - convert timestamps between timezones. Reads from stdin or file.

    -f, --from TZ                    Input timezone (default: auto-detect)
    -t, --to TZ                      Output timezone (default: $TZ, else UTC)
    -l, --list                       List timezone aliases
    -i, --in-place                   Edit file in place
    -F, --format FMT                 Output format: iso, short, time (default: preserve input)
    -d, --date DATE                  Reference date for time-only inputs (resolves DST)
    -j, --json                       Emit a JSON array of matches
    -J, --ndjson                     Emit newline-delimited JSON (one object per match)
        --detect                     Report detected format/zone without converting
    -v, --verbose                    Print diagnostics to stderr
    -V, --version                    Show version
    -h, --help                       Show this help

Environment:
  TZ    Default timezone for input and output (overridden by -f / -t)

Examples:
  echo '2026-04-03T12:00:00Z' | tztr -t sf
  echo '15:30 UTC' | tztr -t pst
  echo '12:00 EST' | tztr -t -8
  tail -f app.log | tztr -t nyc
  echo '15:30 UTC' | tztr -t pst -j
  tail -f app.log | tztr -t nyc -J
  echo '2026-04-03T12:00:00Z' | tztr --detect -j
  echo '15:30' | tztr -f pacific -t utc -d 2026-01-15";

struct Options {
    from: Option<String>,
    /// `from` was defaulted from `$TZ` rather than chosen with `-f`.
    from_is_implicit: bool,
    to: String,
    format: Option<Format>,
    date: Option<String>,
    inplace: bool,
    json: bool,
    ndjson: bool,
    detect: bool,
    verbose: bool,
    files: Vec<String>,
}

/// An error about one of our inputs names it, and the run carries on with the
/// rest, as cat and sed -i do — stopping would leave -i half-applied. A broken
/// pipe is about stdout and stdin has no name, so neither is about an input
/// file: both fall through to the bare form.
fn file_error(file: &str, e: io::Error) -> String {
    if e.kind() == io::ErrorKind::BrokenPipe {
        e.to_string()
    } else {
        format!("{file}: {e}")
    }
}

/// Short flags that take a value (so a bundle like `-tsf` means `-t sf`).
fn is_value_short(c: char) -> bool {
    matches!(c, 'f' | 't' | 'd' | 'F')
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("tztr: {msg}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let local_tz = env::var("TZ").ok().filter(|s| !s.is_empty());

    let mut from: Option<String> = None;
    let mut to_arg: Option<String> = None;
    let mut format: Option<Format> = None;
    let mut date: Option<String> = None;
    let mut inplace = false;
    let mut json = false;
    let mut ndjson = false;
    let mut detect = false;
    let mut verbose = false;
    let mut help = false;
    let mut files: Vec<String> = Vec::new();

    let mut args: VecDeque<String> = env::args().skip(1).collect();
    while let Some(arg) = args.pop_front() {
        // POSIX terminator: everything after it is a filename, flag-shaped or
        // not. Handled before `take_value` borrows `args`.
        if arg == "--" {
            files.extend(args.drain(..));
            break;
        }

        // Resolve a token into (name, inline value). Handles `--opt=value` and
        // bundled short flags (`-vj` -> `-v -j`, `-tsf` -> `-t sf`), mirroring
        // Ruby's OptionParser: a value-taking flag consumes the rest of the
        // cluster as its value (or the next token if the rest is empty), and any
        // trailing boolean flags are requeued.
        let (name, inline): (String, Option<String>) = if arg.starts_with("--") {
            match arg.split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (arg.clone(), None),
            }
        } else if arg.starts_with('-') && arg.len() > 2 {
            let mut chars = arg[1..].chars();
            let c = chars.next().unwrap();
            let rest: String = chars.collect();
            if is_value_short(c) {
                let inline = if rest.is_empty() { None } else { Some(rest) };
                (format!("-{c}"), inline)
            } else {
                if !rest.is_empty() {
                    args.push_front(format!("-{rest}"));
                }
                (format!("-{c}"), None)
            }
        } else {
            (arg.clone(), None)
        };

        let mut take_value = |inline: Option<String>| -> Result<String, String> {
            if let Some(v) = inline {
                return Ok(v);
            }
            args.pop_front()
                .ok_or_else(|| format!("missing argument for {name}"))
        };

        // A no-argument flag written `--name=value` used to accept the value
        // and throw it away, so `--json=true` worked and taught the user
        // nothing -- and `--format=iso` does take one, which makes the shape
        // look plausible. Refuse it instead.
        let no_value = |inline: Option<String>| -> Result<(), String> {
            match inline {
                Some(_) => Err(format!("{name} takes no argument")),
                None => Ok(()),
            }
        };

        match name.as_str() {
            "-f" | "--from" => from = Some(take_value(inline)?),
            "-t" | "--to" => to_arg = Some(take_value(inline)?),
            "-d" | "--date" => date = Some(take_value(inline)?),
            "-F" | "--format" => {
                format = Some(parse_format(&take_value(inline)?)?);
            }
            "-l" | "--list" => {
                no_value(inline)?;
                list_aliases();
                return Ok(ExitCode::SUCCESS);
            }
            "-i" | "--in-place" => {
                no_value(inline)?;
                inplace = true;
            }
            "-j" | "--json" => {
                no_value(inline)?;
                json = true;
            }
            "-J" | "--ndjson" => {
                no_value(inline)?;
                ndjson = true;
            }
            "--detect" => {
                no_value(inline)?;
                detect = true;
            }
            "-v" | "--verbose" => {
                no_value(inline)?;
                verbose = true;
            }
            "-V" | "--version" => {
                no_value(inline)?;
                println!("{VERSION}");
                return Ok(ExitCode::SUCCESS);
            }
            "-h" | "--help" => {
                no_value(inline)?;
                help = true;
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("invalid option: {other}"));
            }
            _ => files.push(arg),
        }
    }

    let json_mode = json || ndjson;

    if help {
        if json_mode {
            let doc = help_doc();
            if ndjson {
                println!("{}", serde_json::to_string(&doc).unwrap());
            } else {
                println!("{}", serde_json::to_string_pretty(&doc).unwrap());
            }
        } else {
            println!("{HELP}");
        }
        return Ok(ExitCode::SUCCESS);
    }

    let zone = |input: &str| resolve_tz(input).map_err(|e| e.to_string());
    let to = zone(to_arg.as_deref().or(local_tz.as_deref()).unwrap_or("UTC"))?;
    let from_is_implicit = from.is_none() && local_tz.is_some();
    let from = match from.as_deref().or(local_tz.as_deref()) {
        Some(f) => Some(zone(f)?),
        None => None,
    };

    let date = match date {
        Some(d) => Some(normalize_date(&d).ok_or_else(|| format!("invalid date: {d}"))?),
        None => None,
    };

    let opts = Options {
        from,
        from_is_implicit,
        to,
        format,
        date,
        inplace,
        json,
        ndjson,
        detect,
        verbose,
        files,
    };

    if opts.inplace && (json_mode || opts.detect) {
        return Err("-i cannot be combined with --json/--ndjson/--detect".to_string());
    }

    // An implicit source zone gets the fuller disclosure below instead, once we
    // know a bare timestamp actually made us lean on it.
    if opts.verbose && !opts.from_is_implicit {
        eprintln!(
            "tztr: from={} to={}",
            opts.from.as_deref().unwrap_or("auto"),
            opts.to
        );
    }

    if opts.files.is_empty() && io::stdin().is_terminal() {
        println!("{HELP}");
        return Ok(ExitCode::SUCCESS);
    }

    if opts.inplace {
        return run_inplace(&opts);
    }

    run_stream(&opts, json_mode)
}

fn run_inplace(opts: &Options) -> Result<ExitCode, String> {
    if opts.files.is_empty() {
        return Err("-i requires a file argument".to_string());
    }
    let mut disclosed = Disclosed::default();
    let mut failed = false;
    for file in &opts.files {
        if let Err(e) = edit_in_place(opts, file, &mut disclosed) {
            eprintln!("tztr: {}", file_error(file, e));
            failed = true;
        }
    }
    Ok(exit_code(failed))
}

fn edit_in_place(opts: &Options, file: &str, disclosed: &mut Disclosed) -> io::Result<()> {
    let content = fs::read(file)?;
    let mut translated: Vec<u8> = Vec::with_capacity(content.len());
    for line in content.split_inclusive(|b| *b == b'\n') {
        // -i rewrites the file on the strength of these assumptions, so it
        // has more reason to state them, not less.
        disclose(opts, line, disclosed);
        translated.extend_from_slice(&translate_bytes(
            line,
            &opts.to,
            opts.from.as_deref(),
            opts.format,
            opts.date.as_deref(),
        ));
    }
    if translated != content {
        fs::write(file, translated)?;
    }
    Ok(())
}

/// What the line loop carries between lines: where output goes, the `-j` buffer
/// (that mode has to see every match before it can print an array), and which
/// assumptions `-v` has already disclosed.
struct Sink<W: Write> {
    out: W,
    collected: Vec<Match>,
    disclosed: Disclosed,
}

fn run_stream(opts: &Options, json_mode: bool) -> Result<ExitCode, String> {
    let stdout = io::stdout();
    let mut sink = Sink {
        out: stdout.lock(),
        collected: Vec::new(),
        disclosed: Disclosed::default(),
    };

    let mut failed = false;
    (|| -> Result<(), String> {
        if opts.files.is_empty() {
            let stdin = io::stdin();
            for_each_line(stdin.lock(), |line| {
                handle_line(opts, json_mode, line, &mut sink)
            })
            .map_err(|e| e.to_string())?;
        } else {
            for file in &opts.files {
                let result = fs::File::open(file).and_then(|f| {
                    for_each_line(BufReader::new(f), |line| {
                        handle_line(opts, json_mode, line, &mut sink)
                    })
                });
                match result {
                    Ok(()) => {}
                    // stdout is gone; no later file can be written either.
                    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => return Err(e.to_string()),
                    Err(e) => {
                        eprintln!("tztr: {}", file_error(file, e));
                        failed = true;
                    }
                }
            }
        }
        if opts.json {
            let arr = Value::Array(
                sink.collected
                    .iter()
                    .map(|m| json_value(m, opts.detect))
                    .collect(),
            );
            writeln!(sink.out, "{}", serde_json::to_string_pretty(&arr).unwrap())
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })()?;

    Ok(exit_code(failed))
}

/// Every file was attempted; any that failed was already reported.
fn exit_code(failed: bool) -> ExitCode {
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Say out loud what a bare timestamp forced us to assume: the source zone,
/// taken from `$TZ` rather than chosen, and today's date for DST. A tester got
/// a wrong answer from exactly these two silent assumptions and only caught it
/// because a bare line and an ISO line in the same output disagreed by an hour.
/// stderr only, so `-j`/`-J` stdout stays clean JSON.
/// State whatever `line` assumes that no earlier line already did, so each
/// assumption is heard once, the first time it is actually made. `disclosed`
/// accumulates across the run.
/// What `-v` has already said, so each thing is said once.
#[derive(Default)]
struct Disclosed {
    assumed: Assumptions,
    ignored: Vec<String>,
}

fn disclose(opts: &Options, line: &[u8], disclosed: &mut Disclosed) {
    if !opts.verbose || opts.detect {
        return;
    }

    // A zone the user wrote but we didn't read, because of its case (Pst).
    for token in ignored_zones(line) {
        if !disclosed.ignored.contains(&token) {
            eprintln!(
                "tztr: ignored \"{token}\": a zone abbreviation is matched in all uppercase or all lowercase"
            );
            disclosed.ignored.push(token);
        }
    }

    let new = assumptions(line).minus(disclosed.assumed);
    if !new.any() {
        return;
    }
    disclosed.assumed = disclosed.assumed.union(new);

    if new.zone {
        if let Some(from) = opts.from.as_deref().filter(|_| opts.from_is_implicit) {
            eprintln!("tztr: from={from} (implicit, from $TZ) to={}", opts.to);
        }
    }
    if new.date && opts.date.is_none() {
        eprintln!(
            "tztr: no -d given, assuming {} for DST resolution",
            assumed_date(line, opts.from.as_deref(), &opts.to).unwrap_or_default()
        );
    }
}

/// Handle one raw input line. Everything here works in bytes: the patterns are
/// ASCII, so a line carrying an undecodable byte still has its timestamps found
/// and converted, and every other byte comes out exactly as it went in. These
/// are people's logs — lossy decoding used to rewrite them.
fn handle_line<W: Write>(
    opts: &Options,
    json_mode: bool,
    line: &[u8],
    sink: &mut Sink<W>,
) -> io::Result<()> {
    disclose(opts, line, &mut sink.disclosed);
    let out = &mut sink.out;

    if json_mode {
        let ms = matches_bytes(
            line,
            &opts.to,
            opts.from.as_deref(),
            opts.format,
            opts.detect,
            opts.date.as_deref(),
        );
        if opts.ndjson {
            for m in ms {
                writeln!(out, "{}", to_json(&m, opts.detect))?;
            }
        } else {
            sink.collected.extend(ms);
        }
    } else if opts.detect {
        for m in matches_bytes(line, &opts.to, opts.from.as_deref(), None, true, None) {
            writeln!(
                out,
                "{}\t{}\t{}",
                m.original,
                m.detected_format,
                m.detected_tz.unwrap_or_default()
            )?;
        }
    } else {
        out.write_all(&translate_bytes(
            line,
            &opts.to,
            opts.from.as_deref(),
            opts.format,
            opts.date.as_deref(),
        ))?;
    }
    Ok(())
}

/// Iterate lines preserving their trailing newline, like Ruby's `each_line`.
/// Stays in bytes: decoding happens per line, so one bad byte cannot rewrite
/// the rest of the stream.
fn for_each_line<R: BufRead>(
    mut reader: R,
    mut f: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        f(&buf)?;
    }
    Ok(())
}

fn parse_format(v: &str) -> Result<Format, String> {
    match v {
        "iso" => Ok(Format::Iso),
        "short" => Ok(Format::Short),
        "time" => Ok(Format::Time),
        _ => Err(format!("invalid format: {v} (expected iso, short, time)")),
    }
}

fn list_aliases() {
    for (k, v) in timezone_aliases() {
        println!("{k:<12} {v}");
    }
}

fn json_value(m: &Match, detect: bool) -> Value {
    let mut obj = Map::new();
    obj.insert("original".into(), Value::String(m.original.clone()));
    obj.insert(
        "detected_format".into(),
        Value::String(m.detected_format.clone()),
    );
    obj.insert(
        "detected_tz".into(),
        m.detected_tz.clone().map_or(Value::Null, Value::String),
    );
    if !detect {
        obj.insert(
            "translated".into(),
            m.translated.clone().map_or(Value::Null, Value::String),
        );
    }
    if let Some(group) = &m.group {
        obj.insert(
            "group".into(),
            json!({ "type": group.kind, "members": group.members }),
        );
    }
    Value::Object(obj)
}

fn to_json(m: &Match, detect: bool) -> String {
    serde_json::to_string(&json_value(m, detect)).unwrap()
}

/// Structured description of the CLI, emitted by `-h -j` / `-h -J` so agents can
/// read the option schema instead of scraping the text help. Mirrors bin/tztr's
/// `HELP_DOC` (same fields and option order).
fn help_doc() -> Value {
    json!({
        "name": "tztr",
        "version": VERSION,
        "usage": "tztr [options] [file]",
        "summary": "Timezone Translator - convert timestamps between timezones. Reads from stdin or file.",
        "options": [
            {"short": "-f", "long": "--from", "arg": "TZ", "description": "Input timezone (default: auto-detect)"},
            {"short": "-t", "long": "--to", "arg": "TZ", "description": "Output timezone (default: $TZ, else UTC)"},
            {"short": "-l", "long": "--list", "arg": null, "description": "List timezone aliases"},
            {"short": "-i", "long": "--in-place", "arg": null, "description": "Edit file in place"},
            {"short": "-F", "long": "--format", "arg": "FMT", "description": "Output format: iso, short, time (default: preserve input)"},
            {"short": "-d", "long": "--date", "arg": "DATE", "description": "Reference date for time-only inputs (resolves DST)"},
            {"short": "-j", "long": "--json", "arg": null, "description": "Emit a JSON array of matches"},
            {"short": "-J", "long": "--ndjson", "arg": null, "description": "Emit newline-delimited JSON (one object per match)"},
            {"short": null, "long": "--detect", "arg": null, "description": "Report detected format/zone without converting"},
            {"short": "-v", "long": "--verbose", "arg": null, "description": "Print diagnostics to stderr"},
            {"short": "-V", "long": "--version", "arg": null, "description": "Show version"},
            {"short": "-h", "long": "--help", "arg": null, "description": "Show this help"},
        ],
        "environment": [
            {"name": "TZ", "description": "Default timezone for input and output (overridden by -f / -t)"}
        ],
        "examples": [
            "echo '2026-04-03T12:00:00Z' | tztr -t sf",
            "echo '15:30 UTC' | tztr -t pst",
            "echo '12:00 EST' | tztr -t -8",
            "tail -f app.log | tztr -t nyc",
            "echo '15:30 UTC' | tztr -t pst -j",
            "tail -f app.log | tztr -t nyc -J",
            "echo '2026-04-03T12:00:00Z' | tztr --detect -j",
            "echo '15:30' | tztr -f pacific -t utc -d 2026-01-15"
        ]
    })
}

/// Normalize a `-d` date to `YYYY-MM-DD`, or `None` if it is not one of the
/// documented forms — `YYYY-MM-DD`, `YYYY/MM/DD`, `YYYYMMDD`, `Month D, YYYY`,
/// `D Month YYYY`. Ambiguous day-first/month-first slash dates (`1/15/2026`)
/// are rejected rather than guessed at.
fn normalize_date(input: &str) -> Option<String> {
    use regex::Regex;
    // YYYY-MM-DD / YYYY/MM/DD / YYYYMMDD — two digits, no `2026-1-5`, one
    // separator throughout. Exactly Ruby's set: no trimming, single spaces.
    let ymd = Regex::new(
        r"^([0-9]{4})(?:-([0-9]{2})-([0-9]{2})|/([0-9]{2})/([0-9]{2})|([0-9]{2})([0-9]{2}))$",
    )
    .unwrap();
    if let Some(c) = ymd.captures(input) {
        let group =
            |a: usize, b: usize, d: usize| c.get(a).or_else(|| c.get(b)).or_else(|| c.get(d));
        return build_date(&c[1], group(2, 4, 6)?.as_str(), group(3, 5, 7)?.as_str());
    }

    // "Month D, YYYY" / "Mon D YYYY"
    let mdy = Regex::new(r"^([A-Za-z]+)\.? ([0-9]{1,2}),? ([0-9]{4})$").unwrap();
    if let Some(c) = mdy.captures(input) {
        return build_date(&c[3], &month_number(&c[1])?.to_string(), &c[2]);
    }

    // "D Month YYYY"
    let dmy = Regex::new(r"^([0-9]{1,2}) ([A-Za-z]+)\.?,? ([0-9]{4})$").unwrap();
    if let Some(c) = dmy.captures(input) {
        return build_date(&c[3], &month_number(&c[2])?.to_string(), &c[1]);
    }

    None
}

/// Format the date, rejecting one the calendar does not have — `2026-02-30`
/// used to pass a day <= 31 check and then silently no-op downstream.
fn build_date(year: &str, month: &str, day: &str) -> Option<String> {
    let date = Date::new(year.parse().ok()?, month.parse().ok()?, day.parse().ok()?).ok()?;
    Some(date.strftime("%Y-%m-%d").to_string())
}

/// The full month name, or exactly its first three letters. Nothing between:
/// `Sep` and `September` are forms, `Sept` is not.
fn month_number(name: &str) -> Option<u32> {
    let n = name.to_lowercase();
    let months = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    months
        .iter()
        .position(|m| *m == n || (n.len() == 3 && m.starts_with(&n)))
        .map(|i| i as u32 + 1)
}

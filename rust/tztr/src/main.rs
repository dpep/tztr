//! `tztr` CLI — Rust port of `bin/tztr`. Kept functionally identical to the
//! Ruby reference (same flags, output, and behavior); see CLAUDE.md.

use serde_json::{Map, Value};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};
use std::process::ExitCode;

use tztr::{matches, resolve_tz, timezone_aliases, translate, Format, Match};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const HELP: &str = "\
Usage: tztr [options] [file]

Timezone Translator - convert timestamps timezone. Reads from stdin or file.

    -f, --from TZ        Input timezone (default: auto-detect)
    -t, --to TZ          Output timezone (default: $TZ, else UTC)
    -l, --list           List timezone aliases
    -i, --in-place       Edit file in place
    -F, --format FMT     Output format: iso, short, time (default: preserve input)
    -d, --date DATE      Reference date for time-only inputs (resolves DST)
    -j, --json           Emit a JSON array of matches
    -J, --ndjson         Emit newline-delimited JSON (one object per match)
        --detect         Report detected format/zone without converting
    -v, --verbose        Print diagnostics to stderr
    -V, --version        Show version
    -h, --help           Show this help

Environment:
  TZ    Sets default output timezone (overridden by -t)

Examples:
  echo '2026-04-03T12:00:00Z' | tztr -t sf
  echo '15:30 UTC' | tztr -t pst
  echo '12:00 EST' | tztr -t -8
  tail -f app.log | tztr -t nyc
  echo '15:30 UTC' | tztr -t pst -j
  tail -f app.log | tztr -t nyc -J
  echo '2026-04-03T12:00:00Z' | tztr --detect -j
  echo '15:30 PST' | tztr -t utc -d 2026-01-15";

struct Options {
    from: Option<String>,
    to: String,
    format: Option<Format>,
    date: Option<String>,
    local: bool,
    inplace: bool,
    json: bool,
    ndjson: bool,
    detect: bool,
    verbose: bool,
    files: Vec<String>,
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
    let mut files: Vec<String> = Vec::new();

    let mut args = env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        // Split `--opt=value` into name + inline value.
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if arg.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut take_value = |inline: Option<String>| -> Result<String, String> {
            if let Some(v) = inline {
                return Ok(v);
            }
            args.next()
                .ok_or_else(|| format!("missing argument for {name}"))
        };

        match name.as_str() {
            "-f" | "--from" => from = Some(take_value(inline)?),
            "-t" | "--to" => to_arg = Some(take_value(inline)?),
            "-d" | "--date" => date = Some(take_value(inline)?),
            "-F" | "--format" => {
                format = Some(parse_format(&take_value(inline)?)?);
            }
            "-l" | "--list" => {
                list_aliases();
                return Ok(ExitCode::SUCCESS);
            }
            "-i" | "--in-place" => inplace = true,
            "-j" | "--json" => json = true,
            "-J" | "--ndjson" => ndjson = true,
            "--detect" => detect = true,
            "-v" | "--verbose" => verbose = true,
            "-V" | "--version" => {
                println!("{VERSION}");
                return Ok(ExitCode::SUCCESS);
            }
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(ExitCode::SUCCESS);
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("invalid option: {other}"));
            }
            _ => files.push(arg),
        }
    }

    let to = resolve_tz(to_arg.as_deref().or(local_tz.as_deref()).unwrap_or("UTC"));
    let from = from
        .as_deref()
        .map(resolve_tz)
        .or_else(|| local_tz.as_deref().map(resolve_tz));
    let local = to == resolve_tz(local_tz.as_deref().unwrap_or("UTC"));

    let date = match date {
        Some(d) => Some(normalize_date(&d).ok_or_else(|| format!("invalid date: {d}"))?),
        None => None,
    };

    let opts = Options {
        from,
        to,
        format,
        date,
        local,
        inplace,
        json,
        ndjson,
        detect,
        verbose,
        files,
    };

    let json_mode = opts.json || opts.ndjson;

    if opts.inplace && (json_mode || opts.detect) {
        return Err("-i cannot be combined with --json/--ndjson/--detect".to_string());
    }

    if opts.verbose {
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
    for file in &opts.files {
        let content = fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
        let translated: String = content
            .split_inclusive('\n')
            .map(|line| {
                translate(
                    line,
                    &opts.to,
                    opts.from.as_deref(),
                    opts.format,
                    opts.local,
                    opts.date.as_deref(),
                )
            })
            .collect();
        if translated != content {
            fs::write(file, translated).map_err(|e| format!("{file}: {e}"))?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_stream(opts: &Options, json_mode: bool) -> Result<ExitCode, String> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut collected: Vec<Match> = Vec::new();

    let result = (|| -> io::Result<()> {
        if opts.files.is_empty() {
            let stdin = io::stdin();
            for_each_line(stdin.lock(), |line| {
                handle_line(opts, json_mode, line, &mut out, &mut collected)
            })?;
        } else {
            for file in &opts.files {
                let f = fs::File::open(file)?;
                for_each_line(BufReader::new(f), |line| {
                    handle_line(opts, json_mode, line, &mut out, &mut collected)
                })?;
            }
        }
        if opts.json {
            let arr = Value::Array(
                collected
                    .iter()
                    .map(|m| json_value(m, opts.detect))
                    .collect(),
            );
            writeln!(out, "{}", serde_json::to_string_pretty(&arr).unwrap())?;
        }
        Ok(())
    })();

    result.map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

fn handle_line(
    opts: &Options,
    json_mode: bool,
    line: &str,
    out: &mut dyn Write,
    collected: &mut Vec<Match>,
) -> io::Result<()> {
    if json_mode {
        let ms = matches(
            line,
            &opts.to,
            opts.from.as_deref(),
            opts.format,
            opts.local,
            opts.detect,
            opts.date.as_deref(),
        );
        if opts.ndjson {
            for m in ms {
                writeln!(out, "{}", to_json(&m, opts.detect))?;
            }
        } else {
            collected.extend(ms);
        }
    } else if opts.detect {
        for m in matches(
            line,
            &opts.to,
            opts.from.as_deref(),
            None,
            false,
            true,
            None,
        ) {
            writeln!(
                out,
                "{}\t{}\t{}",
                m.original,
                m.detected_format,
                m.detected_tz.unwrap_or_default()
            )?;
        }
    } else {
        write!(
            out,
            "{}",
            translate(
                line,
                &opts.to,
                opts.from.as_deref(),
                opts.format,
                opts.local,
                opts.date.as_deref(),
            )
        )?;
    }
    Ok(())
}

/// Iterate lines preserving their trailing newline, like Ruby's `each_line`.
fn for_each_line<R: BufRead>(
    mut reader: R,
    mut f: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', &mut buf)?;
        if n == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        f(&line)?;
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
    Value::Object(obj)
}

fn to_json(m: &Match, detect: bool) -> String {
    serde_json::to_string(&json_value(m, detect)).unwrap()
}

/// Normalize a flexible date string to `YYYY-MM-DD`, or `None` if unparseable.
/// Covers the common forms Ruby's `Date.parse` accepts for our use; ambiguous
/// day-first slash dates (e.g. `1/15/2026`) are rejected, as in Ruby.
fn normalize_date(input: &str) -> Option<String> {
    use regex::Regex;
    let input = input.trim();

    // ISO and year-first slash: YYYY-MM-DD / YYYY/MM/DD
    let iso = Regex::new(r"^(\d{4})[-/](\d{1,2})[-/](\d{1,2})$").unwrap();
    if let Some(c) = iso.captures(input) {
        return build_date(&c[1], c[2].parse().ok()?, c[3].parse().ok()?);
    }

    // "Month D, YYYY" / "Month D YYYY"
    let mdy = Regex::new(r"(?i)^([a-z]+)\.?\s+(\d{1,2}),?\s+(\d{4})$").unwrap();
    if let Some(c) = mdy.captures(input) {
        let month = month_number(&c[1])?;
        return build_date(&c[3], month, c[2].parse().ok()?);
    }

    // "D Month YYYY"
    let dmy = Regex::new(r"(?i)^(\d{1,2})\s+([a-z]+)\.?,?\s+(\d{4})$").unwrap();
    if let Some(c) = dmy.captures(input) {
        let month = month_number(&c[2])?;
        return build_date(&c[3], month, c[1].parse().ok()?);
    }

    None
}

fn build_date(year: &str, month: u32, day: u32) -> Option<String> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(format!("{year}-{month:02}-{day:02}"))
}

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
        .position(|m| *m == n || m.starts_with(&n) && n.len() >= 3)
        .map(|i| i as u32 + 1)
}

//! End-to-end CLI tests for the `tztr` binary. Mirrors the CLI section of the
//! Ruby spec (spec/tztr_spec.rs).

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_tztr")
}

struct Output {
    stdout: Vec<u8>,
    stderr: String,
    ok: bool,
}

impl Output {
    fn out(&self) -> &str {
        std::str::from_utf8(&self.stdout).expect("stdout is utf-8")
    }
}

/// Run the CLI with args + stdin. `tz` of `None` runs with `TZ` unset.
fn run_tz(input: &[u8], args: &[&str], tz: Option<&str>) -> Output {
    let mut cmd = Command::new(bin());
    cmd.args(args);
    match tz {
        Some(tz) => cmd.env("TZ", tz),
        None => cmd.env_remove("TZ"),
    };
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    Output {
        stdout: out.stdout,
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        ok: out.status.success(),
    }
}

/// Run the CLI with args + stdin under a fixed TZ, returning (stdout, success).
fn run(input: &str, args: &[&str], tz: &str) -> (String, bool) {
    let o = run_tz(input.as_bytes(), args, Some(tz));
    (o.out().to_string(), o.ok)
}

fn stdout(input: &str, args: &[&str]) -> String {
    let (o, ok) = run(input, args, "UTC");
    assert!(ok, "expected success");
    o.trim_end().to_string()
}

/// Run expecting failure, returning the single stderr line (without `\n`).
fn fails(input: &str, args: &[&str]) -> String {
    let o = run_tz(input.as_bytes(), args, Some("UTC"));
    assert!(!o.ok, "expected failure, got stdout {:?}", o.out());
    assert!(o.stdout.is_empty(), "expected no stdout: {:?}", o.out());
    // Only the newline — a trailing space is part of the message for `-t ''`.
    o.stderr.trim_end_matches('\n').to_string()
}

#[test]
fn converts_via_stdin() {
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-t", "America/Los_Angeles"]),
        "2026-04-03T05:00:00-07:00"
    );
}

#[test]
fn handles_multiline_input() {
    let out = stdout(
        "first 2026-04-03T12:00:00Z\nsecond 2026-04-03T13:00:00Z\n",
        &["-t", "America/Los_Angeles"],
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "first 2026-04-03T05:00:00-07:00");
    assert_eq!(lines[1], "second 2026-04-03T06:00:00-07:00");
}

#[test]
fn uses_tz_env_as_default_output() {
    let (o, ok) = run("2026-04-03T12:00:00Z\n", &[], "America/New_York");
    assert!(ok);
    assert_eq!(o.trim_end(), "2026-04-03T08:00:00-04:00");
}

#[test]
fn accepts_alias_and_numeric_offset() {
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-t", "sf"]),
        "2026-04-03T05:00:00-07:00"
    );
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-t", "-7"]),
        "2026-04-03T05:00:00-07:00"
    );
}

#[test]
fn shows_version() {
    assert_eq!(stdout("", &["-V"]), env!("CARGO_PKG_VERSION"));
}

#[test]
fn shows_help() {
    let out = stdout("", &["-h"]);
    assert!(out.contains("Usage: tztr"));
    assert!(out.contains("Timezone Translator"));
}

#[test]
fn bundles_short_flags() {
    // Bundled short flags expand like Ruby's OptionParser: a value-taking flag
    // consumes the rest of the cluster as its value.
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-tsf"]),
        "2026-04-03T05:00:00-07:00"
    );
    // boolean + value-taking: -vtsf == -v -t sf (verbose goes to stderr).
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-vtsf"]),
        "2026-04-03T05:00:00-07:00"
    );
    // -hj == -h -j -> JSON help.
    let help = stdout("", &["-hj"]);
    assert!(serde_json::from_str::<serde_json::Value>(&help).is_ok());
}

#[test]
fn shows_help_as_json() {
    // -h -j emits the option schema as a JSON object (agent-friendly), not text.
    let out = stdout("", &["-h", "-j"]);
    let doc: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(doc["name"], "tztr");
    assert_eq!(doc["version"], env!("CARGO_PKG_VERSION"));

    let opts = doc["options"].as_array().unwrap();
    assert!(opts.iter().any(|o| o["long"] == "--from"));
    assert!(opts.iter().any(|o| o["long"] == "--detect"));

    // Every documented option's long flag also appears in the text help.
    let text = stdout("", &["-h"]);
    for o in opts {
        let long = o["long"].as_str().unwrap();
        assert!(text.contains(long), "text help missing {long}");
    }

    // -h -J emits the same document as a single NDJSON line.
    let nd = stdout("", &["-h", "-J"]);
    assert_eq!(nd.lines().count(), 1);
    let nd_doc: serde_json::Value = serde_json::from_str(&nd).unwrap();
    assert_eq!(nd_doc, doc);
}

#[test]
fn lists_aliases() {
    let out = stdout("", &["-l"]);
    assert!(out.contains("sf"));
    assert!(out.contains("America/Los_Angeles"));
}

#[test]
fn emits_json_array() {
    let out = stdout("from 15:30 UTC to 16:45 UTC", &["-t", "pst", "-j"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["translated"], "08:30 PDT");
    assert_eq!(v[1]["translated"], "09:45 PDT");
    assert_eq!(v[0]["detected_tz"], "UTC");
}

#[test]
fn emits_ndjson() {
    let out = stdout(
        "2026-04-03T12:00:00Z\n2026-04-03T13:00:00Z\n",
        &["-t", "pst", "-J"],
    );
    let converted: Vec<String> = out
        .lines()
        .map(|l| {
            serde_json::from_str::<serde_json::Value>(l).unwrap()["translated"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        converted,
        vec!["2026-04-03T05:00:00-07:00", "2026-04-03T06:00:00-07:00"]
    );
}

#[test]
fn detect_reports_without_translating() {
    let out = stdout("2026-04-03T12:00:00Z", &["--detect"]);
    assert_eq!(out, "2026-04-03T12:00:00Z\tiso\tZ");

    let json = stdout("2026-04-03T12:00:00Z", &["--detect", "-j"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(v[0].get("translated").is_none());
    assert_eq!(v[0]["detected_format"], "iso");
}

#[test]
fn applies_reference_date() {
    assert_eq!(
        stdout("15:30 PST", &["-t", "utc", "-d", "2026-01-15"]),
        "23:30 UTC"
    );
    assert_eq!(
        stdout("15:30 PST", &["-t", "utc", "-d", "January 15, 2026"]),
        "23:30 UTC"
    );
}

#[test]
fn rejects_combining_inplace_with_json() {
    let (out, ok) = run("15:30 PST\n", &["-i", "-j", "/tmp/whatever.txt"], "UTC");
    assert!(!ok);
    assert!(out.is_empty());
}

#[test]
fn aborts_on_unparseable_date() {
    let (_out, ok) = run("15:30 PST\n", &["-d", "not-a-date"], "UTC");
    assert!(!ok);
}

// --- B12/B15: help text and the `--` terminator -----------------------------

#[test]
fn help_option_lines_use_option_parsers_columns() {
    // Ruby renders `-h` through OptionParser: four-space indent, flag column
    // padded to 33, description at column 37. Plain `-h` has to match it byte
    // for byte, so pin the geometry here.
    let text = stdout("", &["-h"]);
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("    -") || l.starts_with("        --"))
        .collect();
    assert_eq!(lines.len(), 12, "expected one line per option");
    for l in lines {
        let b = l.as_bytes();
        assert!(b.len() > 37, "{l:?}");
        assert_eq!(b[36], b' ', "flag column not padded to 33: {l:?}");
        assert_ne!(b[37], b' ', "description not at column 37: {l:?}");
    }
}

#[test]
fn accepts_the_posix_double_dash_terminator() {
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-t", "pst", "--"]),
        "2026-04-03T05:00:00-07:00"
    );
    // ...and everything after it is a file, not a flag.
    let path = std::env::temp_dir().join("-tztr-dashed.log");
    std::fs::write(&path, "2026-04-03T12:00:00Z\n").unwrap();
    let o = run_tz(
        b"",
        &["-t", "pst", "--", path.to_str().unwrap()],
        Some("UTC"),
    );
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(o.out().trim_end(), "2026-04-03T05:00:00-07:00");
    std::fs::remove_file(&path).unwrap();
}

// --- B10: one documented set of -d date forms -------------------------------

#[test]
fn accepts_the_documented_date_forms() {
    for date in [
        "2026-01-15",
        "2026/01/15",
        "20260115",
        "January 15, 2026",
        "Jan 15 2026",
        "15 January 2026",
    ] {
        assert_eq!(
            stdout("15:30 PST", &["-t", "utc", "-d", date]),
            "23:30 UTC",
            "{date}"
        );
    }
}

#[test]
fn rejects_dates_outside_that_set_and_dates_that_do_not_exist() {
    for date in [
        "2026-02-30", // April has 30 days, February does not
        "2026-13-01",
        "2026-00-10",
        "not-a-date",
        "01/15/2026", // ambiguous day-first/month-first
        "15-01-2026",
        "2026-01-15T00:00:00Z",
    ] {
        assert_eq!(
            fails("15:30 PST\n", &["-t", "utc", "-d", date]),
            format!("tztr: invalid date: {date}"),
            "{date}"
        );
    }
}

// --- B8: non-UTF-8 input keeps its bytes ------------------------------------

#[test]
fn passes_invalid_utf8_lines_through_byte_for_byte() {
    // from_utf8_lossy silently rewrote these two bytes to U+FFFD, corrupting
    // the user's log on the way past.
    let input: &[u8] = b"2026-04-03T12:00:00Z \xff\xfe junk\n";
    let o = run_tz(input, &["-t", "pst"], Some("UTC"));
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        b"2026-04-03T05:00:00-07:00 \xff\xfe junk\n".to_vec()
    );
}

#[test]
fn an_invalid_line_does_not_stop_the_valid_ones() {
    let input: &[u8] = b"2026-04-03T12:00:00Z\n\xff\xfe\n2026-04-03T13:00:00Z\n";
    let o = run_tz(input, &["-t", "utc"], Some("UTC"));
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        b"2026-04-03T12:00:00Z\n\xff\xfe\n2026-04-03T13:00:00Z\n".to_vec()
    );
}

#[test]
fn in_place_edit_keeps_bytes_it_cannot_decode() {
    // `-i` used to abort on the whole file ("stream did not contain valid
    // UTF-8") and leave it untranslated.
    let path = std::env::temp_dir().join("tztr-inplace-bytes.log");
    std::fs::write(&path, b"2026-04-03T12:00:00Z \xff\xfe junk\n").unwrap();
    let o = run_tz(
        b"",
        &["-i", "-t", "utc", path.to_str().unwrap()],
        Some("pst"),
    );
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"2026-04-03T12:00:00Z \xff\xfe junk\n".to_vec()
    );
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn structured_output_skips_invalid_lines_and_stays_parseable() {
    // Raw bytes cannot go into a JSON stream; the line is dropped from the
    // structured view rather than corrupting stdout for the reader.
    let input: &[u8] = b"\xff\xfe\n2026-04-03T12:00:00Z\n";
    let o = run_tz(input, &["-t", "pst", "-j"], Some("UTC"));
    assert!(o.ok, "{}", o.stderr);
    let v: serde_json::Value = serde_json::from_str(o.out()).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["translated"], "2026-04-03T05:00:00-07:00");
}

// --- B14: `-F short` always says which zone ---------------------------------

#[test]
fn short_format_always_labels_the_zone() {
    // The zone label used to be dropped whenever the output zone happened to
    // equal $TZ -- which is every invocation that simply omits `-t`. This
    // output gets pasted into tickets; it has to say which zone it is in.
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-F", "short"]),
        "2026-04-03 12:00 UTC"
    );
    let o = run_tz(b"2026-04-03T12:00:00Z\n", &["-F", "short"], None);
    assert_eq!(o.out().trim_end(), "2026-04-03 12:00 UTC");
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-F", "short", "-t", "nyc"]),
        "2026-04-03 08:00 EDT"
    );
    // ...including inside structured output.
    let json = stdout("2026-04-03T12:00:00Z", &["-F", "short", "-j"]);
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v[0]["translated"], "2026-04-03 12:00 UTC");
}

// --- B4/B5/B13: an unresolvable timezone is a hard error --------------------

#[test]
fn rejects_unknown_timezones() {
    for (args, expected) in [
        (["-t", "Bogus/Zone"], "tztr: unknown timezone: Bogus/Zone"),
        (["-t", ""], "tztr: unknown timezone: "),
        (
            ["-t", "America/New York"],
            "tztr: unknown timezone: America/New York",
        ),
        (["-f", "Bogus/Zone"], "tztr: unknown timezone: Bogus/Zone"),
    ] {
        assert_eq!(fails("2026-04-03T12:00:00Z\n", &args), expected);
    }
}

#[test]
fn rejects_out_of_range_numeric_offsets() {
    for arg in ["15", "-13", "99"] {
        assert_eq!(
            fails("2026-04-03T12:00:00Z\n", &["-t", arg]),
            format!("tztr: offset out of range: {arg} (expected -12..14)")
        );
    }
    // ...and accepts the edges.
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-t", "14"]),
        "2026-04-04T02:00:00+14:00"
    );
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-t", "-12"]),
        "2026-04-03T00:00:00-12:00"
    );
}

#[test]
fn rejects_sub_hour_numeric_offsets() {
    // Silently mis-signing these is worse than refusing; half-hour zones are
    // reached by name (`-t ist`).
    assert_eq!(
        fails("2026-04-03T12:00:00Z\n", &["-t", "+5:30"]),
        "tztr: unknown timezone: +5:30"
    );
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["-t", "ist"]),
        "2026-04-03T17:30:00+05:30"
    );
}

#[test]
fn rejects_an_unresolvable_tz_env() {
    let o = run_tz(b"2026-04-03T12:00:00Z\n", &[], Some("Bogus/Zone"));
    assert!(!o.ok);
    assert_eq!(o.stderr.trim_end(), "tztr: unknown timezone: Bogus/Zone");
}

#[test]
fn honors_the_posix_leading_colon_on_tz() {
    // POSIX spells TZ with a leading colon; without this, TimeZone::get failed
    // and *every* conversion silently came out UTC.
    let o = run_tz(b"2026-04-03T12:00:00Z\n", &[], Some(":America/New_York"));
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(o.out().trim_end(), "2026-04-03T08:00:00-04:00");
}

#[test]
fn treats_an_empty_tz_as_unset() {
    let o = run_tz(b"2026-04-03T12:00:00Z\n", &[], Some(""));
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(o.out().trim_end(), "2026-04-03T12:00:00Z");
}

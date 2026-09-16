//! End-to-end CLI tests for the `tztr` binary. Mirrors the CLI section of the
//! Ruby spec (spec/tztr_spec.rs).

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_tztr")
}

/// A scratch file named for this process, so concurrent test runs (the parity
/// harness builds and runs alongside) cannot collide on it.
fn temp_path(stem: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("{stem}-{}.log", std::process::id()))
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
    // A run that fails on its arguments exits before reading stdin, so this
    // write races with the child and loses the pipe. That is the case under
    // test, not a failure of it.
    let mut stdin = child.stdin.take().unwrap();
    if let Err(e) = stdin.write_all(input) {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe,
            "writing stdin: {e}"
        );
    }
    drop(stdin);
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

// --- B16: -v discloses what it assumed --------------------------------------

/// `-v` over a bare time, with `$TZ` supplying the source zone.
fn verbose_bare(args: &[&str]) -> Output {
    run_tz(b"15:30\n", args, Some("America/Los_Angeles"))
}

#[test]
fn verbose_discloses_the_assumptions_a_bare_timestamp_forces() {
    let o = verbose_bare(&["-v", "-t", "nyc"]);
    assert!(o.ok, "{}", o.stderr);
    let lines: Vec<&str> = o.stderr.lines().collect();
    assert!(
        lines.contains(&"tztr: from=America/Los_Angeles (implicit, from $TZ) to=America/New_York"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("tztr: no -d given, assuming ")
                && l.ends_with(" for DST resolution")),
        "{lines:?}"
    );
    assert!(!o.out().contains("tztr:"), "diagnostics leaked to stdout");
}

#[test]
fn verbose_suppresses_the_startup_line_when_the_source_zone_is_implicit() {
    // Otherwise it and the disclosure line say the same thing twice.
    let o = verbose_bare(&["-v", "-t", "nyc"]);
    assert!(
        !o.stderr.contains("tztr: from=America/Los_Angeles to="),
        "{}",
        o.stderr
    );
    // An explicit -f keeps the plain startup line.
    let o = verbose_bare(&["-v", "-f", "utc", "-t", "nyc"]);
    assert!(
        o.stderr.contains("tztr: from=UTC to=America/New_York"),
        "{}",
        o.stderr
    );
    // No -f and no $TZ keeps it too.
    let o = run_tz(b"15:30\n", &["-v"], None);
    assert!(o.stderr.contains("tztr: from=auto to=UTC"), "{}", o.stderr);
}

#[test]
fn verbose_dates_the_dst_assumption_in_the_target_zone() {
    let o = verbose_bare(&["-v", "-t", "Pacific/Auckland"]);
    let expected = format!(
        "tztr: no -d given, assuming {} for DST resolution",
        tztr::today_in_zone("Pacific/Auckland")
    );
    assert!(o.stderr.contains(&expected), "{}", o.stderr);
}

#[test]
fn verbose_discloses_the_date_assumption_even_when_the_match_names_a_zone() {
    // `15:30 UTC` into New York still has to assume a date to know whether the
    // answer is EDT or EST -- the DST exposure is on the *output* side, so a
    // match carrying its own zone is not off the hook.
    let o = run_tz(b"15:30 UTC\n", &["-v", "-t", "nyc"], None);
    assert!(o.ok, "{}", o.stderr);
    let expected = format!(
        "tztr: no -d given, assuming {} for DST resolution",
        tztr::today_in_zone("America/New_York")
    );
    assert!(o.stderr.contains(&expected), "{}", o.stderr);

    // ...and `-d` answers it, so nothing is assumed.
    let o = run_tz(
        b"15:30 UTC\n",
        &["-v", "-t", "nyc", "-d", "2026-01-15"],
        None,
    );
    assert!(!o.stderr.contains("no -d given"), "{}", o.stderr);
    assert_eq!(o.out().trim_end(), "10:30 EST");
}

#[test]
fn verbose_does_not_claim_a_source_zone_the_timestamp_carried_itself() {
    // `15:30 UTC` took its source zone from the timestamp, not from $TZ.
    // Saying otherwise under -v asserts something false, and -v exists to be
    // believed. The date is still assumed, so that line stays.
    let o = verbose_bare(&["-v", "-t", "nyc"]);
    assert!(o.stderr.contains("(implicit, from $TZ)"), "{}", o.stderr);

    let o = run_tz(
        b"15:30 UTC\n",
        &["-v", "-t", "nyc"],
        Some("America/Los_Angeles"),
    );
    assert!(o.ok, "{}", o.stderr);
    assert!(!o.stderr.contains("(implicit, from $TZ)"), "{}", o.stderr);
    assert!(o.stderr.contains("no -d given"), "{}", o.stderr);
}

#[test]
fn verbose_discloses_each_assumption_the_first_time_it_is_made() {
    // The date is assumed on line 1, the source zone only on line 2 — each
    // gets said once, when it first actually happens.
    let o = run_tz(
        b"15:30 UTC\n15:30\n15:30\n",
        &["-v", "-t", "nyc"],
        Some("America/Los_Angeles"),
    );
    assert!(o.ok, "{}", o.stderr);
    assert_eq!(o.stderr.matches("no -d given").count(), 1, "{}", o.stderr);
    assert_eq!(
        o.stderr.matches("(implicit, from $TZ)").count(),
        1,
        "{}",
        o.stderr
    );
    // ...and the date line comes first, having been assumed first.
    let date_at = o.stderr.find("no -d given").unwrap();
    let zone_at = o.stderr.find("(implicit, from $TZ)").unwrap();
    assert!(date_at < zone_at, "{}", o.stderr);
}

#[test]
fn verbose_stays_quiet_when_nothing_was_assumed() {
    // -f and -d given: both assumptions are the user's, not ours.
    let o = verbose_bare(&["-v", "-f", "pst", "-t", "nyc", "-d", "2026-01-15"]);
    assert!(!o.stderr.contains("implicit"), "{}", o.stderr);
    assert!(!o.stderr.contains("no -d given"), "{}", o.stderr);

    // A timestamp carrying its own zone and date assumes nothing.
    let o = run_tz(
        b"2026-04-03T12:00:00Z\n",
        &["-v", "-t", "nyc"],
        Some("America/Los_Angeles"),
    );
    assert!(!o.stderr.contains("implicit"), "{}", o.stderr);
    assert!(!o.stderr.contains("no -d given"), "{}", o.stderr);
}

#[test]
fn verbose_discloses_once_and_leaves_structured_stdout_clean() {
    let o = run_tz(
        b"15:30\n16:30\n17:30\n",
        &["-v", "-t", "nyc", "-j"],
        Some("America/Los_Angeles"),
    );
    assert!(o.ok, "{}", o.stderr);
    serde_json::from_str::<serde_json::Value>(o.out()).expect("stdout is still JSON");
    assert_eq!(o.stderr.matches("(implicit, from $TZ)").count(), 1);
    assert_eq!(o.stderr.matches("no -d given").count(), 1);
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
        "Jan. 15, 2026",
        "Sep 15 2026", // three letters exactly; `Sept` is not a form we take
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
        "Sept 15, 2026", // full name or exactly three letters, nothing between
        "Janu 15 2026",
        "2026-1-5", // numeric forms want two digits
        "2026/1/5",
    ] {
        assert_eq!(
            fails("15:30 PST\n", &["-t", "utc", "-d", date]),
            format!("tztr: invalid date: {date}"),
            "{date}"
        );
    }
}

// --- B9: one clean error line, whichever path hit it ------------------------

#[test]
fn an_error_about_a_file_names_the_file() {
    // With several file arguments the bare message says nothing about which
    // one failed, so both code paths name it.
    let missing = "/nonexistent/tztr-does-not-exist.log";
    assert_eq!(
        fails("", &["-t", "utc", missing]),
        format!("tztr: {missing}: No such file or directory (os error 2)")
    );
    assert_eq!(
        fails("", &["-i", "-t", "utc", missing]),
        format!("tztr: {missing}: No such file or directory (os error 2)")
    );
    assert_eq!(
        fails("", &["-t", "utc", "/tmp"]),
        "tztr: /tmp: Is a directory (os error 21)"
    );
    assert_eq!(
        fails("", &["-i", "-t", "utc", "/tmp"]),
        "tztr: /tmp: Is a directory (os error 21)"
    );
}

#[test]
fn a_broken_pipe_is_not_blamed_on_the_input_file() {
    // `tztr big.log | head -1` is the commonest way this tool gets stopped.
    // EPIPE comes from writing to stdout — the input file was fine, and naming
    // it sends the user to look in the wrong place.
    let path = temp_path("tztr-epipe");
    let body = "2026-04-03T12:00:00Z\n".repeat(50_000);
    std::fs::write(&path, body).unwrap();

    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "{} -t pst {} | head -1",
            bin(),
            path.to_str().unwrap()
        ))
        .env("TZ", "UTC")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    std::fs::remove_file(&path).unwrap();

    assert!(
        stderr.is_empty() || stderr.trim_end() == "tztr: Broken pipe (os error 32)",
        "unexpected stderr: {stderr:?}"
    );
}

#[test]
fn rejects_an_inline_value_on_a_flag_that_takes_none() {
    // Accepting `--json=true` and throwing the value away teaches the user
    // nothing -- and `--format=iso` does work, so the shape looks plausible.
    for flag in [
        "--json",
        "--ndjson",
        "--detect",
        "--verbose",
        "--list",
        "--in-place",
        "--version",
        "--help",
    ] {
        assert_eq!(
            fails("2026-04-03T12:00:00Z\n", &[&format!("{flag}=foo")]),
            format!("tztr: {flag} takes no argument"),
            "{flag}"
        );
    }
}

#[test]
fn still_accepts_an_inline_value_where_one_belongs() {
    assert_eq!(
        stdout("2026-04-03T12:00:00Z", &["--format=short", "--to=nyc"]),
        "2026-04-03 08:00 EDT"
    );
    assert_eq!(
        stdout("15:30 PST", &["--to=utc", "--date=2026-01-15"]),
        "23:30 UTC"
    );
    // Short flags have no `=value` form to police.
    let out = stdout("2026-04-03T12:00:00Z", &["-j", "-t", "utc"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["translated"], "2026-04-03T12:00:00Z");
}

#[test]
fn errors_not_about_a_file_stay_bare() {
    assert_eq!(
        fails("2026-04-03T12:00:00Z\n", &["--bogus"]),
        "tztr: invalid option: --bogus"
    );
    assert_eq!(
        fails("2026-04-03T12:00:00Z\n", &["-t", "Bogus/Zone"]),
        "tztr: unknown timezone: Bogus/Zone"
    );
    assert_eq!(
        fails("15:30\n", &["-d", "not-a-date"]),
        "tztr: invalid date: not-a-date"
    );
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
    let path = temp_path("tztr-inplace-bytes");
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
fn structured_output_still_sees_timestamps_on_an_invalid_line() {
    // One stray byte must not cost the line its timestamp here either. The
    // match itself is ASCII, so the JSON stays well-formed.
    let input: &[u8] = b"2026-04-03T12:00:00Z \xff\xfe junk\n";
    let o = run_tz(input, &["-t", "pst", "-j"], Some("UTC"));
    assert!(o.ok, "{}", o.stderr);
    let v: serde_json::Value = serde_json::from_str(o.out()).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 1);
    assert_eq!(v[0]["original"], "2026-04-03T12:00:00Z");
    assert_eq!(v[0]["translated"], "2026-04-03T05:00:00-07:00");

    let o = run_tz(input, &["-t", "pst", "--detect"], Some("UTC"));
    assert_eq!(o.out().trim_end(), "2026-04-03T12:00:00Z\tiso\tZ");
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

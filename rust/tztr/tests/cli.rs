//! End-to-end CLI tests for the `tztr` binary. Mirrors the CLI section of the
//! Ruby spec (spec/tztr_spec.rs).

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_tztr")
}

/// Run the CLI with args + stdin under a fixed TZ, returning (stdout, success).
fn run(input: &str, args: &[&str], tz: &str) -> (String, bool) {
    let mut child = Command::new(bin())
        .args(args)
        .env("TZ", tz)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (String::from_utf8(out.stdout).unwrap(), out.status.success())
}

fn stdout(input: &str, args: &[&str]) -> String {
    let (o, ok) = run(input, args, "UTC");
    assert!(ok, "expected success");
    o.trim_end().to_string()
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

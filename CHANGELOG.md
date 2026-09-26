###  0.2.0  (2026-09-16)

#### Breaking

Check any script that passes `-t gmt`, a numeric offset or `-v`, and anything
that relied on an unrecognized timezone quietly falling back to UTC.

- `-t gmt` now means UTC. It used to follow British Summer Time — use `-t london` for civil UK time.
- A timezone that can't be resolved is an error (exit 1) instead of silently becoming UTC. That includes `TZ` itself: the POSIX spelling `TZ=:America/New_York` used to make every conversion come out in UTC.
- Numeric offsets are limited to whole hours in `-12..14`. Sub-hour offsets (`-t +5:30`) are refused rather than silently mis-signed — reach a half-hour zone by name (`-t ist`, `-t Asia/Kolkata`).
- `-F short` always labels the timezone. It used to drop the label whenever the output zone matched `$TZ`, which is every run without `-t`.
- `-v` is verbose mode; use `-V/--version` for the version. Verbose now also names the assumptions a bare timestamp forces — the source zone borrowed from `$TZ`, and the date assumed for DST.
- Library: `resolve_tz` raises `Tztr::Error` (Ruby) / returns `Result<String, TzError>` (Rust), and `translate`/`matches` lost their `local` parameter.

#### Added

- `-j/--json` and `-J/--ndjson` structured output — `{original, detected_format, detected_tz, translated}` per match.
- `--detect` reports the detected format and zone without converting.
- `-d/--date` supplies a reference date for time-only inputs, so DST resolves against the right day. It takes `2026-01-15`, `2026/01/15`, `20260115`, `January 15, 2026`, `Jan 15 2026` or `15 January 2026` — month names full or exactly three letters — and validates the real calendar. Anything else is an error.
- 12-hour times: `11:30:00 PM`, `12:30 AM`, `3:45 p.m.`, `11:30 A.M.`, `3:45 PM PST`. One rough edge: a date plus a 12-hour time with no seconds gains a seconds field, so `2026-04-03 3:45 PM` comes back as `2026-04-03 15:45:00 UTC`.

#### Fixed

- Every timestamp on a line converts, whatever its format. Only the first format found used to convert, so in `{"ts":"2026-04-03T12:00:00Z","msg":"at 15:30 UTC"}` the `15:30 UTC` was left alone with no warning. A bare time with no date, zone or AM/PM is left alone when another timestamp on its line names a date, zone or AM/PM. It is most likely a duration, as in `2026-04-03T12:00:00Z took 0:05`.
- The start of a range takes the zone and AM/PM written after its end: `from 3:30 to 4:45 PM PST` is 3:30 PM PST to 4:45 PM PST. The start takes the opposite AM/PM when the same one would run the range backwards (`11:30 to 1:00 PM` starts at 11:30 AM). A range can be joined by `-`, `–`, `—`, `to`, `until`, `till`, `through` or `thru`.
- A time with a UTC offset must include seconds (`12:34:56-05:00`), with the offset within ±14 hours. `15:30-16:45 PST` used to be read as 15:30 at an offset of −16:45.
- A file that can't be read no longer stops a multi-file run. It is reported, every other file is still processed, and the exit status is 1, as with `cat` and `sed -i`. With `-i` it used to leave the job half-done: files before the bad one rewritten, files after it untouched.
- Zone abbreviations are matched against an explicit list instead of "any 2-4 uppercase letters". Log levels stay in the line (`15:30 INFO server started` keeps its `INFO`), and `JST`, `CET`, `AEST` and the rest now actually convert instead of being read as local time. Lowercase spellings (`15:30 pst`) are recognized too, except `est`, `cet`, `et`, `ist`, `ut` and `z`, which are ordinary words that can follow a time.
- An ambiguous wall clock in a repeated fall-back hour resolves to the earlier (daylight) occurrence, matching `date(1)`, Temporal, RFC 5545 and ICU.
- `24:00` and a leap-second `23:59:60` normalize; impossible calendar dates (`2026-02-30`) are left exactly as found instead of being rewritten to a different day.
- Lines that aren't valid UTF-8 keep their bytes, in every output mode including `-i`.
- An error about a file names the file, in both the streaming and `-i` paths.
- Missing files, directories and unknown flags print one line instead of a backtrace.
- The POSIX `--` terminator works.
- `tztr -l` and plain `tztr -h` are byte-identical between the Ruby and Rust builds.

###  0.1.0  (2026-04-19)


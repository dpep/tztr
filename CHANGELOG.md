### Unreleased

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

- `tztr now` prints the current time, as ISO 8601 in `-t`, else `$TZ`, else UTC. Every output flag works with it.
- A dated clock with seconds takes a numeric offset, glued or spaced: `2026-09-25 22:14:42-07:00` (Python, `date --rfc-3339`), `2026-04-03 09:00:00-07` (Postgres). ISO 8601's comma fraction (`…T22:14:42,123456789-07:00`) and a no-break space before AM/PM (`3:45 PM`, as Chrome and macOS write it) are read too.
- An abbreviation the source zone (`-f`, else `$TZ`) itself uses is read in that zone's sense: with `TZ=Asia/Shanghai`, `CST` is China Standard Time, and `date | tztr` works in Manila, Dublin, Jerusalem or Helsinki. Anywhere that doesn't use it, including unset, UTC and the US, the US reading stands.
- More log formats, each converted date and all: nginx/Apache access logs (`[15/Jan/2015:12:31:01 -0700]`), slashed dates (`2026/09/25 23:14:42`, Go's log package and nginx's error log), `ls -lT` (`Sep 25 23:40:39 2026`), and glibc's locale forms of `date` (`Fri 25 Sep 2026 10:14:42 PM PDT`).
- `-j/--json` and `-J/--ndjson` structured output — `{original, detected_format, detected_tz, translated}` per match, plus `group: {type, members}` for a timestamp in a range or list.
- `--detect` reports the detected format and zone without converting.
- `-d/--date` supplies a reference date for time-only inputs, so DST resolves against the right day. It takes `2026-01-15`, `2026/01/15`, `20260115`, `January 15, 2026`, `Jan 15 2026` or `15 January 2026` — month names full or exactly three letters — and validates the real calendar. Anything else is an error.
- Ranges and lists share the zone and AM/PM written at their end: `from 3:30 to 4:45 PM PST` and `3:00, 4:00 or 5:00 PM PST` convert every member. `11:30 to 1:00 PM` starts in the morning, and `9-9:15am` reads the bare `9` as the range's start. A range is joined by `-`, `–`, `—`, `to`, `until`, `till`, `through` or `thru`; a list by commas, `or` and `and`.
- `-v` names a mixed-case zone it passed over (`15:30 Pst`).
- 12-hour times: `11:30:00 PM`, `12:30 AM`, `3:45 p.m.`, `11:30 A.M.`, `3:45 PM PST`, and hours with AM/PM: `9am`, `9 PM PST`.
- A time followed by a unit of time is left alone as a duration: RSpec's `Finished in 1:05 minutes`.

#### Fixed

- A dated timestamp without seconds keeps its date. `2026-01-15 23:30` used to be read as a time alone, resolved against today, an hour off in winter; `2026-12-31T23:30+05:30` came out garbled and `2026-12-31T23:30Z` was ignored.
- In a range or list with no date, a member earlier on the clock than the one before it is on the next day: `11:30 PM to 12:30 AM` ends tomorrow.
- date(1) output and RFC 2822 dates convert as one timestamp. `date | tztr -t est` used to convert only the clock, so `Fri Sep 25 22:14:42 PDT 2026` came out as `Fri Sep 25 01:14:42 EDT 2026`, a day early; it is now `Sat Sep 26 01:14:42 EDT 2026`. A date(1) line with a zone tztr can't resolve (`WIB`) is left as written.
- Fractional seconds keep every digit (Docker's nanoseconds were cut to milliseconds), and Python logging's `12:00:00,123` keeps its comma milliseconds with the seconds.
- Every timestamp on a line converts, whatever its format. Only the first format found used to convert, so in `{"ts":"2026-04-03T12:00:00Z","msg":"at 15:30 UTC"}` the `15:30 UTC` was left alone with no warning. A bare time with no date, zone or AM/PM is left alone when another timestamp on its line names a date, zone or AM/PM. It is most likely a duration, as in `2026-04-03T12:00:00Z took 0:05`.
- A numeric offset glued to a time must follow seconds (`12:34:56-05:00`), and every offset must be within -12..+14. `15:30-16:45 PST` used to be read as 15:30 at an offset of −16:45.
- A file that can't be read no longer stops a multi-file run. It is reported, every other file is still processed, and the exit status is 1, as with `cat` and `sed -i`. With `-i` it used to leave the job half-done: files before the bad one rewritten, files after it untouched.
- Zone abbreviations are matched against an explicit list instead of "any 2-4 uppercase letters". Log levels stay in the line (`15:30 INFO server started` keeps its `INFO`), and `JST`, `CET`, `AEST` and the rest now actually convert instead of being read as local time. Each is the fixed offset it names, as the US ones always were (`CEST` is +02:00 even in January); only the generic `ET`, `CT`, `MT` and `PT` follow DST. Lowercase spellings (`15:30 pst`) are recognized too, except `est`, `cet`, `et`, `ist`, `ut` and `z`, which are ordinary words that can follow a time.
- IPv6 addresses (`fe80::1:23:45`) and SMPTE timecodes (`01:02:03:04`) are left alone instead of having a "time" inside them converted.
- An ambiguous wall clock in a repeated fall-back hour resolves to the earlier (daylight) occurrence, matching `date(1)`, Temporal, RFC 5545 and ICU.
- `24:00` and a leap-second `23:59:60` normalize; impossible calendar dates (`2026-02-30`) are left exactly as found instead of being rewritten to a different day.
- Lines that aren't valid UTF-8 keep their bytes, in every output mode including `-i`.
- An error about a file names the file, in both the streaming and `-i` paths.
- Missing files, directories and unknown flags print one line instead of a backtrace.
- The POSIX `--` terminator works.
- `tztr -l` and plain `tztr -h` are byte-identical between the Ruby and Rust builds.

###  0.1.0  (2026-04-19)


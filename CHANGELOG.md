###  0.2.0  (2026-09-26)

0.1.0 reached users twice: the gem in April, then the Rust crate and Homebrew
formula in June, which already had `-j`, `-J`, `--detect`, `-d` and `-v`/`-V`.
Entries marked *(gem)* are new only to gem users.

#### Breaking

Before upgrading, check any script that passes `-t gmt`, a numeric offset,
`-F`, `-d` or `-v`, or that relies on an unknown timezone falling back to UTC.
Also check logs with zone abbreviations tztr used to ignore (`CET`, `BST`,
`JST`), and runs where `TZ` is set to a zone outside the US.

- `-t gmt` now means UTC. It used to follow British Summer Time; use `-t london` for civil UK time.
- A timezone that can't be resolved is an error (exit 1) instead of silently becoming UTC. That includes `TZ` itself: the POSIX spelling `TZ=:America/New_York` used to make every conversion come out in UTC.
- Numeric offsets are whole hours in `-12..14`. Sub-hour offsets (`-t +5:30`) are refused rather than silently mis-signed; reach a half-hour zone by name (`-t ist`, `-t Asia/Kolkata`).
- Zone abbreviations inside text convert as the fixed offset they name. `JST`, `CET`, `BST`, `IST`, `AEST` and the rest used to be ignored, so the time was read as if it were in the source zone. Only the generic `ET`, `CT`, `MT` and `PT` follow DST, so `CEST` is +02:00 even in January. `CET` is +01:00 even in summer, which means a log that writes `CET` year-round for Berlin time converts an hour off from Berlin's clock in summer.
- An abbreviation your source zone (`-f`, else `$TZ`) uses itself is read in that zone's sense. With `TZ=Asia/Shanghai`, `CST` is China Standard Time (+08:00), not US Central. `PST` in Manila and `IST` in Dublin or Jerusalem change the same way. With `TZ` unset, UTC or anywhere in the US, nothing changes.
- `-F short` always labels the zone. It used to drop the label whenever the output zone matched `$TZ`, which is every run without `-t`. *(gem)* `-F short` and `-F iso` used to carry no zone or offset at all; they now end in `PDT` and `-07:00`.
- `-d` accepts only `2026-01-15`, `2026/01/15`, `20260115`, `January 15, 2026`, `Jan 15 2026` and `15 January 2026`, with two-digit months and days and real calendar dates. Anything else is an error. The gem used to take whatever Ruby's `Date.parse` would (`Sept 15`, `2026-1-5`, a date with no year).
- *(gem)* `-v` is `--verbose`; the version is `-V/--version`.
- Library: `resolve_tz` raises `Tztr::Error` (Ruby) / returns `Result<String, TzError>` (Rust), and `translate`/`matches` no longer take a `local` parameter.

#### Added

- `tztr now` prints the current time, as ISO 8601 in `-t`, else `$TZ`, else UTC. Every flag except `-i` works with it.
- `-v` says what it assumed: the source zone it borrowed from `$TZ`, the date it assumed for DST when there's no `-d`, and any zone abbreviation it passed over for its case (`Pst`) or didn't know (`EEST`).
- `-j`/`-J` add `group: {type, members}` to a timestamp in a range or list. *(gem)* `-j/--json`, `-J/--ndjson`, `--detect` and `-d/--date` are new.
- Ranges and lists share the zone and AM/PM written at their end, and a date written at their start: `from 3:30 to 4:45 PM PST` and `3:00, 4:00 or 5:00 PM PST` convert every member. A range is joined by `-`, `–`, `—`, `to`, `until`, `till`, `through` or `thru`; a list by commas, `or` and `and`.
- More formats, each converted date and all: nginx/Apache access logs (`[15/Jan/2015:12:31:01 -0700]`), slashed dates (`2026/09/25 23:14:42`, Go's log package and nginx's error log), `ls -lT` (`Sep 25 23:40:39 2026`), and glibc's locale form of `date` (`Fri 25 Sep 2026 10:14:42 PM PDT`).
- A dated clock with seconds takes a numeric offset, glued or spaced: `2026-09-25 22:14:42-07:00` (Python, `date --rfc-3339`), `2026-04-03 09:00:00-07` (Postgres). ISO 8601's comma fraction (`…T22:14:42,123456789-07:00`) is read too.
- 12-hour forms: `3:45 p.m.`, `11:30 A.M.`, hours alone (`9am`, `9 PM PST`), and a no-break space before AM/PM, as Chrome and macOS write it.
- Lowercase zone abbreviations (`15:30 pst`), except `est`, `cet`, `et`, `ist`, `ut` and `z`, which are ordinary words that can follow a time.
- A time followed by a unit of time is left alone as a duration: RSpec's `Finished in 1:05 minutes`.

#### Fixed

- A dated timestamp without seconds keeps its date. `2026-01-15 23:30` used to be read as a time alone, resolved against today, an hour off in winter; `2026-12-31T23:30+05:30` came out garbled and `2026-12-31T23:30Z` was ignored.
- `date` output and RFC 2822 dates convert as one timestamp. `date | tztr -t est` used to convert only the clock, so `Fri Sep 25 22:14:42 PDT 2026` came out as `Fri Sep 25 01:14:42 EDT 2026`, a day early; it is now `Sat Sep 26 01:14:42 EDT 2026`. A `date` line with a zone tztr can't resolve (`WIB`) is left as written.
- A 12-hour time is read in its source zone. AM/PM used to be taken for a zone name, so `11:30 PM` was assumed to be in the output zone already and the `PST` of `3:45 PM PST` was ignored.
- Every timestamp on a line converts, whatever its format. Only the first format found used to convert, so in `{"ts":"2026-04-03T12:00:00Z","msg":"at 15:30 UTC"}` the `15:30 UTC` was left alone with no warning. A bare time with no date, zone or AM/PM is still left alone when another timestamp on its line has one, as the `0:05` of `2026-04-03T12:00:00Z took 0:05` is most likely a duration.
- In a range or list with no date, a member earlier on the clock than the one before it is on the next day: `11:30 PM to 12:30 AM` ends tomorrow.
- A numeric offset glued to a time must follow seconds (`12:34:56-05:00`), and every offset must be within -12..+14. `15:30-16:45 PST` used to be read as 15:30 at an offset of −16:45.
- Log levels stay in the line: `15:30 INFO server started` keeps its `INFO`.
- Fractional seconds keep every digit (Docker's nanoseconds were cut to milliseconds), and Python logging's `12:00:00,123` keeps its comma milliseconds.
- IPv6 addresses (`fe80::1:23:45`) and SMPTE timecodes (`01:02:03:04`) are left alone instead of having a "time" inside them converted.
- An ambiguous wall clock in a repeated fall-back hour resolves to the earlier (daylight) occurrence, matching `date(1)`, Temporal, RFC 5545 and ICU.
- `24:00` and a leap-second `23:59:60` normalize; impossible calendar dates (`2026-02-30`) are left exactly as found instead of being rewritten to a different day.
- A file that can't be read no longer stops a multi-file run. It is reported, every other file is still processed, and the exit status is 1, as with `cat` and `sed -i`. With `-i` it used to leave the job half-done: files before the bad one rewritten, files after it untouched.
- Errors are one line, with no backtrace, and an error about a file names it.
- Lines that aren't valid UTF-8 keep their bytes, in every output mode including `-i`.
- Rust build: the POSIX `--` terminator works, and `tztr -h` and `tztr -l` match the gem byte for byte.

###  0.1.0  (2026-04-19)


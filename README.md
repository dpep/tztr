tztr
======
![Gem](https://img.shields.io/gem/dt/tztr?style=plastic)
[![codecov](https://codecov.io/gh/dpep/tztr/branch/main/graph/badge.svg)](https://codecov.io/gh/dpep/tztr)

Timezone Translator: find the timestamps in any text and convert them to
another timezone, leaving everything else alone.

Reads from stdin, files, or the clock (`tztr now`), auto-detects timestamp
formats, and preserves each one's original format by default.


## Install

```bash
brew install dpep/tools/tztr   # the Rust binary
gem install tztr               # Ruby
cargo install tztr             # Rust, from source
```

The gem and the crate are two implementations of the same tool, kept
functionally identical. Same CLI, same output.


## Usage

```bash
echo '2026-04-03T12:00:00Z' | tztr -t America/Los_Angeles
# 2026-04-03T05:00:00-07:00

echo '15:30 UTC' | tztr -t America/New_York
# 11:30 EDT   (a time with no date resolves against today; in January, 10:30 EST)

tail -f app.log | tztr

tztr now -t tokyo
# 2026-09-27T07:28:30+09:00   (whatever the time is)
```

`tztr now` reads the current time as though it were piped in, so every flag
works with it (`-F short`, `-j`, `--detect`) except `-i`. It prints ISO 8601 in
`-t`, else `$TZ`, else UTC. To read a file named `now`, write `./now`.

Several files are processed in order. A file that can't be read is reported
and skipped, the rest still run (and with `-i`, are still rewritten), and the
exit status is 1, as with `cat` and `sed -i`.

### Options

```
-f, --from TZ       Input timezone for timestamps that name none (default: $TZ, else -t)
-t, --to TZ         Output timezone (default: $TZ, else UTC)
-l, --list          List timezone aliases
-i, --in-place      Edit files in place
-F, --format FMT    Output format: iso, short, time (default: preserve input)
-d, --date DATE     Reference date for time-only inputs (resolves DST)
-j, --json          Emit a JSON array of matches
-J, --ndjson        Emit newline-delimited JSON (one object per match)
    --detect        Report detected format/zone without converting
-v, --verbose       Print diagnostics to stderr
-V, --version       Show version
-h, --help          Show this help
```

`-F` replaces the preserved format with one of:

| `-F`    | Output                      |
|---------|-----------------------------|
| `iso`   | `2026-04-03 08:30:00-07:00` |
| `short` | `2026-04-03 08:30 PDT`      |
| `time`  | `08:30:00`                  |


## Timezones

`-f` and `-t` take an IANA name (`America/Los_Angeles`), an alias or
abbreviation (`pst`, `nyc`, `jst`; `tztr -l` lists them all), or a whole-hour
numeric offset from `-12` to `14` (`-8` → `Etc/GMT+8`). Sub-hour offsets
aren't accepted; reach a half-hour zone by name: `-t ist`, `-t Asia/Kolkata`.

Names given to `-f` and `-t` are real zones that follow DST: `-t est` is New
York time, EDT in summer. `gmt` means UTC; for civil UK time, which follows
British Summer Time, use `-t london`.

`TZ` sets the default for both sides, and `-f`/`-t` override it:

```bash
export TZ=America/Los_Angeles
echo '2026-04-03T12:00:00Z' | tztr
# 2026-04-03T05:00:00-07:00
```

A timestamp that names its own zone is read in that zone, whatever `-f` says.
One that doesn't is read in `-f`, else `$TZ`. With neither set, it's taken to
be in the output zone already, so it's labelled rather than converted:

```bash
echo '12:00' | env -u TZ tztr -t pst -d 2026-04-03
# 12:00 PDT
```

A zone that can't be resolved is an error: exit 1, nothing on stdout.

```bash
echo '15:30 UTC' | tztr -t Mars/Phobos
# tztr: unknown timezone: Mars/Phobos
```


## Supported formats

Every timestamp on a line converts, whatever its format, and everything around
it (quotes, JSON, log levels) passes through untouched.

### What converts

- ISO 8601: `2026-04-03T12:00:00Z`, `2026-04-03T12:00:00+05:30`
- Date + time: `2026-04-03 12:00:00 UTC`, and slashed as Go's log package and
  nginx's error log write it, `2026/04/03 12:00:00`
- A numeric offset after the seconds, glued or spaced, as Python,
  `date --rfc-3339` and Postgres write it: `2026-09-25 22:14:42-07:00`,
  `2026-04-03 09:00:00-07`
- `date` output: `Fri Sep 25 22:14:42 PDT 2026`, with or without the weekday
  (`ls -lT`) or zone (ctime), a numeric zone (`+03`), and glibc's locale form,
  `Fri 25 Sep 2026 10:14:42 PM PDT`
- Email and HTTP dates (RFC 2822): `Fri, 25 Sep 2026 22:14:42 -0700`
- nginx/Apache access logs: `[15/Jan/2015:12:31:01 -0700]`
- Time only: `15:30 UTC`, `08:30:45 PDT`, `12:00 +0530`, `12:34:56-05:00`
- 12-hour: `11:30 PM`, `3:45 p.m.`, `11:30 A.M.`, `3:45 PM PST`, and an hour
  with AM/PM: `9am`, `9 PM PST`
- Fractional seconds, kept to the nanosecond: `…T12:00:00.123456789Z`, the ISO
  comma (`…T12:00:00,123456789Z`), and Python logging's comma milliseconds,
  `2026-04-03 12:00:00,123`

Dated timestamps convert date and all, so `Fri Sep 25 22:14:42 PDT 2026` in
New York is `Sat Sep 26 01:14:42 EDT 2026`. The output keeps the input's shape,
seconds and fraction, except that a 12-hour time comes out on the 24-hour
clock (`11:30 PM` → `23:30 UTC`); only `date`'s locale form and RFC 2822 keep
their AM/PM.

**Seconds** are optional (`2026-04-03 15:30`, `2026-04-03T15:30Z`) except in
three places: `date` output, access logs, and a numeric offset glued to the
clock. That last one is what keeps `15:30-16:45` a range rather than 15:30 at
an offset of −16:45. With a space, an offset needs no seconds
(`12:00 +0530`). An offset outside -12..+14 isn't an offset.

A date, time and zone are read together only when they're written together, in
one of the formats above. A date in another column
(`2026-12-31 | 23:30:00 | UTC`), a syslog date with no year
(`Sep 25 22:14:42`), or a word like `tomorrow` isn't attached to the time
beside it, which converts as a time alone.

### Ranges and lists

A range is joined by `-`, `–`, `—`, `to`, `until`, `till`, `through` or `thru`;
a list by commas, `or` and `and`. Its members share what's written around them:

- **The zone and AM/PM at the end apply to every member before it.**
  `from 3:30 to 4:45 PM PST` reads both ends as PST afternoon times, and
  `3:00, 4:00 or 5:00 PM PST` converts all three. AM/PM flips if it would run
  the range backwards: `11:30 to 1:00 PM` starts in the morning.
- **A date at the start applies to every member after it.**
  `2026-04-03 9:00 AM - 10:00 AM PST` puts both ends on April 3.
- **Nothing else crosses.** A zone on the start stays there: in
  `3:30 PST to 4:45 PM`, the end is in your source zone.
- **A member earlier on the clock than the one before it is on the next day.**
  The preserved format shows only the clock; `-F iso` shows the dates:

```bash
echo '11:30 PM to 12:30 AM PST' | tztr -t utc -d 2026-04-03 -F iso
# 2026-04-04 07:30:00Z to 2026-04-04 08:30:00Z
```

A bare hour is just a number, except as the start of a range whose end has
AM/PM: `9-9:15am` and `9 to 10am` convert, but `Room 7 - 3pm` and
`Apr 3 - 5pm` convert only the `3pm` and `5pm`.

Any other word between two times keeps them apart, as in `15:30, then 16:45 PST`.
Here the rule below applies, and the `15:30` is left alone.

### What's left alone

- **A bare time beside a richer one.** A time with no date, zone or AM/PM, like
  the `0:05` in `2026-04-03T12:00:00Z took 0:05`, is probably a duration. It's
  left alone whenever another timestamp on its line has a date, zone or AM/PM.
  Alone on its line (`request took 0:05`), it can't be told from a time, and
  converts as one.
- **A time followed by a unit of time.** `Finished in 1:05 minutes`,
  `took 2:30 hrs` (`sec`, `min`, `hr`, `hour`, spelled out or plural). So
  `at 15:30 hrs` is left alone too.
- **A zone tztr doesn't know.** `Fri Sep 25 22:14:42 EEST 2026` stays as
  written, rather than half-converted, unless your source zone uses `EEST`
  (see below). `-v` names the zone.
- **Colon runs that aren't times.** IPv6 addresses (`fe80::1:23:45`) and SMPTE
  timecodes (`01:02:03:04`).
- **Impossible dates.** `2026-02-30` stays exactly as found rather than rolling
  to another day.

### Zone abbreviations

- **Standard and daylight names are fixed offsets**, whatever the date: `PST`
  is -08:00 even in July, `CEST` is +02:00 even in January. Only the generic
  `ET`, `CT`, `MT` and `PT` follow DST.
- **Ambiguous names are read the US way** (`CST` is US Central) and `IST` as
  India, **unless your source zone (`-f`, else `$TZ`) uses the name itself**.
  Then it means what it means there: with `TZ=Asia/Shanghai`, `CST` is China
  Standard Time, and with `TZ=Europe/Helsinki`, `date`'s `EEST` converts. With
  `$TZ` unset, UTC or anywhere in the US, the US reading stands.
- **Uppercase or lowercase, never mixed.** `PST` and `pst` both work; `Pst`
  doesn't. Six are uppercase only, because their lowercase spellings are words
  that can follow a time: `est`, `cet`, `et`, `ist`, `ut` and `z`. In
  `à 15:30 est annulée`, `est` is French for "is", not Eastern time.


## JSON output (for agents and scripts)

`-j/--json` emits a JSON array; `-J/--ndjson` emits one object per line, which
streams (works with `tail -f`). Each match is an object:

```bash
echo 'meeting at 15:30 UTC' | tztr -t pst -j
# [
#   {
#     "original": "15:30 UTC",
#     "detected_format": "time",
#     "detected_tz": "UTC",
#     "translated": "08:30 PDT"
#   }
# ]
```

(`08:30 PDT` in summer; `07:30 PST` in winter, as the time has no date.)

A timestamp in a range or list also carries `group`, which lists every member
in order, so the two ends of a range can be read together (abridged):

```bash
echo 'standup 9:00 to 9:15 AM PST' | tztr -t utc -j
# [
#   { "original": "9:00", "detected_tz": "PST", "translated": "17:00 UTC",
#     "group": { "type": "range", "members": ["9:00", "9:15 AM PST"] }, ... },
#   { "original": "9:15 AM PST", "detected_tz": "PST", "translated": "17:15 UTC",
#     "group": { "type": "range", "members": ["9:00", "9:15 AM PST"] }, ... }
# ]
```

`--detect` reports what was found without translating (omits `translated`),
and composes with `-j`/`-J`:

```bash
echo '2026-04-03T12:00:00Z' | tztr --detect -j
# [ { "original": "2026-04-03T12:00:00Z", "detected_format": "iso", "detected_tz": "Z" } ]
```

Combine `-h` with `-j`/`-J` to get the help itself as JSON: the full option
schema (flags, args, descriptions, examples), so an agent can read it instead
of scraping the text.

```bash
tztr -h -j
# { "name": "tztr", "version": "...", "options": [ { "short": "-f", "long": "--from", ... } ], ... }
```


## Diagnostics (`-v`)

`-v` explains itself on stderr, leaving stdout untouched. Each note is printed
once, the first time it applies:

- the source and output zones, when `-f` is given or `$TZ` is unset
- the source zone it borrowed from `$TZ`, when a timestamp needed one
- the date it assumed for DST, when a timestamp had none and there's no `-d`
- a zone it passed over for its case (`Pst`), or didn't know (`EEST`)

```bash
echo '15:30' | tztr -f pacific -t utc -v
# tztr: from=America/Los_Angeles to=UTC
# tztr: no -d given, assuming 2026-09-26 for DST resolution   (whatever today is)
# 22:30 UTC
```


## DST caveats

### Time-only inputs

A time with no date can't say whether it was standard or daylight time.
`tztr` resolves it against **today's** date where the time was written (in the
zone it names, else the source zone). That can be off by an hour for a
timestamp from the other side of a DST boundary:

```bash
echo '15:30' | tztr -f pacific -t utc
# 22:30 UTC   (run in summer: pacific -> PDT, UTC-7. In January: 23:30 UTC)
```

Two ways to remove the guess:

```bash
# Supply the date the time belongs to:
echo '15:30' | tztr -f pacific -t utc -d 2026-01-15
# 23:30 UTC   (January -> PST, UTC-8)
echo '15:30' | tztr -f pacific -t utc -d 'January 15, 2026'

# Or sidestep DST entirely with a fixed numeric offset instead of a named zone:
echo '15:30' | tztr -f -8 -t utc
# 23:30 UTC   (-8 -> Etc/GMT+8, never observes daylight time)
```

`-d` takes one of `2026-01-15`, `2026/01/15`, `20260115`, `January 15, 2026`,
`Jan 15 2026` or `15 January 2026`. Month names are full or exactly three
letters (`Sep`, not `Sept`); the numeric forms need two digits for month and
day. Anything else, including a date that isn't on the calendar, is
`tztr: invalid date: ...` and exit 1.

Inputs that carry a date (`2026-04-03 15:30`) or a fixed offset are
unaffected.

### The repeated hour

A fall-back repeats an hour, so the same wall clock happens twice, dated or
not. `tztr` takes the **earlier** (daylight) occurrence, matching macOS
`date(1)`, Temporal, RFC 5545 and ICU:

```bash
echo '2026-11-01 01:30:00' | tztr -f pacific -t utc
# 2026-11-01 08:30:00 UTC   (01:30 PDT; the later 01:30 PST would be 09:30)
```


## Library

From Ruby:

```ruby
require "tztr"

Tztr.translate("log 2026-04-03T12:00:00Z event", to: "America/Los_Angeles")
# => "log 2026-04-03T05:00:00-07:00 event"
```

`Tztr.matches` returns the `-j` objects for a line, and `Tztr.resolve_tz`
raises `Tztr::Error` for a zone it can't resolve. Both `translate` and
`matches` set `ENV['TZ']` as a side effect; save and restore it if the rest of
your process cares.

The Rust crate exposes the same functions; see
[`rust/tztr`](rust/tztr). The Ruby gem is the reference implementation, and
[`rust/REPORT.md`](rust/REPORT.md) has the port notes and a perf comparison.

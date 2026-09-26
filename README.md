tztr
======
![Gem](https://img.shields.io/gem/dt/tztr?style=plastic)
[![codecov](https://codecov.io/gh/dpep/tztr/branch/main/graph/badge.svg)](https://codecov.io/gh/dpep/tztr)

Timezone Translator - convert timestamps to local time.

Reads from stdin, files, or the clock (`tztr now`), auto-detects timestamp
formats, and preserves the original format by default.


## Install

```bash
brew install dpep/tools/tztr
```


## Usage

```bash
echo '2026-04-03T12:00:00Z' | tztr -t America/Los_Angeles
# 2026-04-03T05:00:00-07:00

echo '15:30 UTC' | tztr -t America/New_York
# 11:30 EDT   (a time with no date resolves against today; in January, 10:30 EST)

tail -f app.log | tztr

tztr now -t tokyo
# 2026-09-26T23:45:04+09:00
```

`now` is the current time, read as though it were piped in, so every flag
works with it (`-F short`, `-j`, `--detect`). It prints ISO 8601 in `-t`,
else `$TZ`, else UTC. To read a file named `now`, write `./now`.

Several files are processed in order. A file that can't be read is reported
and skipped, the rest still run (and with `-i`, are still rewritten), and
the exit status is 1, as with `cat` and `sed -i`.

### Options

```
-f, --from TZ       Input timezone (default: auto-detect)
-t, --to TZ         Output timezone (default: $TZ, else UTC)
-F, --format FMT    Output format: iso, short, time (default: preserve input)
-d, --date DATE     Reference date for time-only inputs (resolves DST)
-l, --list          List timezone aliases
-i, --in-place      Edit file in place
-j, --json          Emit a JSON array of matches
-J, --ndjson        Emit newline-delimited JSON (one object per match)
    --detect        Report detected format/zone without converting
-v, --verbose       Print diagnostics to stderr
-V, --version       Show version
-h, --help          Show this help
```

### Timezones

`-f` and `-t` take an IANA name (`America/Los_Angeles`), an alias or
abbreviation (`pst`, `nyc`, `jst` — `tztr -l` lists all of them), or a
whole-hour numeric offset from `-12` to `14` (`-8` → `Etc/GMT+8`). Sub-hour
offsets are not accepted; reach a half-hour zone by name: `-t ist`,
`-t Asia/Kolkata`.

`gmt` means UTC. For civil UK time, which follows British Summer Time, use
`-t london`.

A zone that can't be resolved is an error — exit 1, nothing on stdout:

```bash
echo '15:30 UTC' | tztr -t Mars/Phobos
# tztr: unknown timezone: Mars/Phobos
```

### Supported Formats

- ISO 8601: `2026-04-03T12:00:00Z`, `2026-04-03T12:00:00+05:30`
- Date + time: `2026-04-03 12:00:00 UTC`, `2026/04/03 12:00:00` (Go's log
  package, nginx's error log)
- date(1) output: `Fri Sep 25 22:14:42 PDT 2026`, and the shapes around it:
  no zone (ctime), no weekday (`ls -lT`), a numeric zone (`+03`), and glibc's
  locale forms (`Fri 25 Sep 2026 10:14:42 PM PDT`). A zone tztr doesn't know
  (`EEST`, `WIB`) leaves the whole date as written, rather than half-converting
  it, and `-v` names it.
- Email and HTTP dates (RFC 2822): `Fri, 25 Sep 2026 22:14:42 -0700`
- nginx/Apache access logs: `[15/Jan/2015:12:31:01 -0700]`
- Time only: `15:30 UTC`, `08:30:45 PDT`
- 12-hour: `11:30 PM`, `3:45 p.m.`, `11:30 A.M.`, `3:45 PM PST`
- Hour only, with AM/PM: `9am`, `9 PM PST`. A bare hour with no AM/PM is just a
  number, except as the start of a range whose end has one: `9-9:15am` or
  `9 to 10am`, but not `Room 7 - 3pm` or `Apr 3 - 5pm`.
- Fractional seconds, kept to the nanosecond: `2026-04-03T12:00:00.123456789Z`,
  an ISO comma (`…T12:00:00,123456789Z`), and Python logging's comma
  milliseconds, `2026-04-03 12:00:00,123` (a comma before another comma is a
  CSV column instead: `12:00:00,200,OK`)
- A dated clock with seconds and a numeric offset, glued or spaced, as Python,
  GNU `date --rfc-3339` and Postgres write it: `2026-09-25 22:14:42-07:00`,
  `2026-04-03 09:00:00-07`

Seconds are optional in every dated format (`2026-04-03 15:30`,
`2026-04-03T15:30Z`), and the output keeps them only if the input had them.

Input is plain text, so JSON and NDJSON work too. Every timestamp on a line
converts, whatever its format, and everything around it, including quotes,
passes through untouched. One exception: a bare time like `0:05`, with no
date, zone or AM/PM, is left alone when another timestamp on the same line
names any of those, since it is most likely a duration (`...Z took 0:05`).
A time followed by a unit of time is a duration too, and left alone:
`Finished in 1:05 minutes`, `took 2:30 hrs` (`sec`, `min`, `hr`, `hour`,
spelled out or plural), which also means `at 15:30 hrs` is left alone.
Otherwise a duration alone on its line
(`request took 0:05`) can't be told from a time, and converts as one.

A range or list shares the zone and AM/PM written at its end.
`from 3:30 to 4:45 PM PST` converts both ends as PST afternoon times,
`3:00, 4:00 or 5:00 PM PST` converts all three, and `11:30 to 1:00 PM` starts
in the morning. A range is joined by `-`, `–`, `—`, `to`, `until`, `till`,
`through` or `thru`; a list by commas, `or` and `and`. Any other word in
between keeps the two apart: in `15:30, then 16:45 PST`, the `15:30` is left
unconverted, as a bare time beside one that names its zone. The zone travels
from the end back to the start, never forward: in `3:30 PST to 4:45 PM`, the
end takes your default zone. A range whose start carries a date shares it with
the end: `2026-04-03 9:00 AM - 10:00 AM PST`.

A numeric offset glued to a time needs seconds (`12:34:56-05:00`,
`12:34:56-0500`), so `15:30-16:45` and `15:30-1645` are read as ranges; with a
space it needs none (`12:00 +0530`). Offsets outside -12..+14 are not offsets.

In a range or list with no date, a member that is earlier on the clock than
the one before it is on the next day. The preserved format shows only the
clock; `-F iso` shows the dates too:

```bash
echo '11:30 PM to 12:30 AM PST' | tztr -t utc -d 2026-04-03 -F iso
# 2026-04-04 07:30:00Z to 2026-04-04 08:30:00Z
```

A date, time and zone are read together only when they are written together,
in one of the formats above. A date in another column
(`2026-12-31 | 23:30:00 | UTC`), a syslog date with no year
(`Sep 25 22:14:42`), or a word like `tomorrow` is not attached to the time
beside it, which converts as a time alone.

Zone abbreviations inside text are recognized in uppercase (`PST`) or
lowercase (`pst`), but not mixed case. Six are uppercase only, because
their lowercase spellings are ordinary words that can follow a time:
`est`, `cet`, `et`, `ist`, `ut` and `z`. In `à 15:30 est annulée`, `est` is
French for "is", not Eastern time. `-v` says when it passed over a mixed-case
one (among its other notes):

```bash
echo '15:30 Pst' | tztr -v
# tztr: ignored "Pst": a zone abbreviation is matched in all uppercase or all lowercase
```

Some abbreviations mean different zones in different places. By default tztr
reads them the US way (`CST` is US Central, `PST` US Pacific) and `IST` as
India. When the source zone (`-f`, else `$TZ`) itself uses an abbreviation,
tztr reads it in that zone's sense instead: with `TZ=Asia/Shanghai`, `CST` is
China Standard Time, and with `TZ=Europe/Helsinki`, `date`'s `EEST` converts
rather than being left as written. With `$TZ` unset, UTC, or anywhere in the
US, nothing changes.

An abbreviation that names standard or daylight time is that fixed offset,
whatever the date: `CEST` is +02:00 even in January, `PST` is -08:00 even in
July. The generic `ET`, `CT`, `MT` and `PT` follow DST, as do the same names
given to `-f` and `-t` (`-t est` is New York time).

### JSON output (for agents & scripts)

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

A timestamp in a range or list also carries `group`, which lists every member
in order, so the two ends of a range can be read together:

```bash
echo 'standup 9:00 to 9:15 AM PST' | tztr -t utc -j
# [
#   { "original": "9:00", "detected_tz": "PST", "translated": "17:00 UTC",
#     "group": { "type": "range", "members": ["9:00", "9:15 AM PST"] }, ... },
#   { "original": "9:15 AM PST", "detected_tz": "PST", "translated": "17:15 UTC",
#     "group": { "type": "range", "members": ["9:00", "9:15 AM PST"] }, ... }
# ]
```

`--detect` reports what was found without translating (omits `translated`), and
composes with `-j`/`-J`:

```bash
echo '2026-04-03T12:00:00Z' | tztr --detect -j
# [ { "original": "...", "detected_format": "iso", "detected_tz": "Z" } ]
```

Combine `-h` with `-j`/`-J` to get the help itself as JSON — the full option
schema (flags, args, descriptions, examples), so an agent can read it instead of
scraping the text:

```bash
tztr -h -j
# { "name": "tztr", "version": "...", "options": [ { "short": "-f", "long": "--from", ... } ], ... }
```

### Environment

Set `TZ` to change the default timezone for input and output (overridden by `-f` / `-t`):

```bash
export TZ=America/Los_Angeles
echo '2026-04-03T12:00:00Z' | tztr
# 2026-04-03T05:00:00-07:00
```

### Caveat: time-only inputs and DST

A time-only input carries no date, so a DST-observing zone on either side of
the conversion can't tell whether it was standard or daylight time. `tztr`
resolves it against **today's** date where the time was written (in the zone
it names, else the source zone), which can be off by an hour for a timestamp
from the other side of a DST boundary:

```bash
echo '15:30' | tztr -f pacific -t utc
# 22:30 UTC   (run in summer: pacific -> PDT, UTC-7. In January: 23:30 UTC)
```

`-v` names the guess on stderr whenever a bare timestamp forces one:

```bash
echo '15:30' | tztr -f pacific -t utc -v
# tztr: from=America/Los_Angeles to=UTC
# tztr: no -d given, assuming 2026-09-26 for DST resolution   (whatever today is)
# 22:30 UTC
```

With no `-f`, it also names the source zone it borrowed from `$TZ`.

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
`Jan 15 2026`, `15 January 2026`. Month names are full or exactly three letters
(`Sep`, not `Sept`); the numeric forms need two digits for month and day.
Anything else, including a date that isn't on the calendar, is
`tztr: invalid date: ...` and exit 1.

Inputs that already carry a date (`2026-04-03 15:30`) or a fixed offset are
unaffected.

One more DST wrinkle, for dated inputs too: a fall-back repeats an hour, so the
same wall clock happens twice. `tztr` takes the **earlier** (daylight)
occurrence, matching macOS `date(1)`, Temporal, RFC 5545 and ICU:

```bash
echo '2026-11-01 01:30:00' | tztr -f pacific -t utc
# 2026-11-01 08:30:00 UTC   (01:30 PDT; the later 01:30 PST would be 09:30)
```


## Library

`tztr` is also published as a Ruby gem and a Rust crate:

```bash
gem install tztr      # Ruby
cargo install tztr    # Rust
```

Use it from Ruby:

```ruby
require "tztr"

Tztr.translate("log 2026-04-03T12:00:00Z event", to: "America/Los_Angeles")
# => "log 2026-04-03T05:00:00-07:00 event"
```

It ships two implementations kept functionally identical: the Ruby gem
(reference) and a [Rust crate](rust/tztr) — Homebrew installs the Rust binary.
Same CLI, output, and behavior; see [`rust/REPORT.md`](rust/REPORT.md) for port
notes and a perf comparison.

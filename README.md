tztr
======
![Gem](https://img.shields.io/gem/dt/tztr?style=plastic)
[![codecov](https://codecov.io/gh/dpep/tztr/branch/main/graph/badge.svg)](https://codecov.io/gh/dpep/tztr)

Timezone Translator - convert timestamps to local time.

Reads from stdin or file, auto-detects timestamp formats, and preserves the original format by default.


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
```

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
- Date + time: `2026-04-03 12:00:00 UTC`
- date(1) output: `Fri Sep 25 22:14:42 PDT 2026` (zone optional, as ctime writes it)
- Email and HTTP dates (RFC 2822): `Fri, 25 Sep 2026 22:14:42 -0700`
- Time only: `15:30 UTC`, `08:30:45 PDT`
- 12-hour: `11:30 PM`, `3:45 p.m.`, `11:30 A.M.`, `3:45 PM PST`
- Hour only, with AM/PM: `9am`, `9 PM PST`. A bare hour with no AM/PM is just a
  number, except as the start of a range whose end has one (`9-9:15am`).
- Fractional seconds: `2026-04-03T12:00:00.123Z`

Input is plain text, so JSON and NDJSON work too. Every timestamp on a line
converts, whatever its format, and everything around it, including quotes,
passes through untouched. One exception: a bare time like `0:05`, with no
date, zone or AM/PM, is left alone when another timestamp on the same line
names any of those, since it is most likely a duration (`...Z took 0:05`).
The check only looks at the line itself: a duration alone on its line
(`request took 0:05`) is indistinguishable from a time and converts as one.

A range or list shares the zone and AM/PM written at its end.
`from 3:30 to 4:45 PM PST` converts both ends as PST afternoon times,
`3:00, 4:00 or 5:00 PM PST` converts all three, and `11:30 to 1:00 PM` starts
in the morning. A range is joined by `-`, `–`, `—`, `to`, `until`, `till`,
`through` or `thru`; a list by commas, `or` and `and`. Any other word in
between (`15:30, then 16:45 PST`) keeps the two apart. The zone travels from
the end back to the start, never forward: in `3:30 PST to 4:45 PM`, the end
takes your default zone.

A time with a UTC offset needs seconds (`12:34:56-05:00`), so `15:30-16:45` is
read as a range.

When a range crosses midnight, the preserved format shows only the clock.
`-F iso` shows the dates too:

```bash
echo '3:30 to 4:45 PM PST' | tztr -t utc -d 2026-04-03 -F iso
# 2026-04-03 23:30:00Z to 2026-04-04 00:45:00Z
```

Zone abbreviations inside text are recognized in uppercase (`PST`) or
lowercase (`pst`), but not mixed case. Six are uppercase only, because
their lowercase spellings are ordinary words that can follow a time:
`est`, `cet`, `et`, `ist`, `ut` and `z`. In `à 15:30 est annulée`, `est` is
French for "is", not Eastern time. `-v` says when it passed over a mixed-case
one:

```bash
echo '15:30 Pst' | tztr -v
# tztr: ignored "Pst": a zone abbreviation is matched in all uppercase or all lowercase
```

One dent in the format-preserving promise: a date paired with a 12-hour time
and no seconds gains a seconds field it never had — `2026-04-03 3:45 PM`
converts to `2026-04-03 15:45:00 UTC`.

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
resolves it against **today's** date — which can be off by an hour for a
timestamp from the other side of a DST boundary:

```bash
echo '15:30' | tztr -f pacific -t utc
# 22:30 UTC   (run in summer: pacific -> PDT, UTC-7. In January: 23:30 UTC)
```

`-v` names the guess on stderr whenever a bare timestamp forces one:

```bash
echo '15:30' | tztr -f pacific -t utc -v
# tztr: from=America/Los_Angeles to=UTC
# tztr: no -d given, assuming 2026-09-16 for DST resolution   (whatever today is)
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

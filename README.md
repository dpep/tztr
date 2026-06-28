tztr
======
![Gem](https://img.shields.io/gem/dt/tztr?style=plastic)
[![codecov](https://codecov.io/gh/dpep/tztr/branch/main/graph/badge.svg)](https://codecov.io/gh/dpep/tztr)

Timezone Translator - convert timestamps to local time.

Reads from stdin or file, auto-detects timestamp formats, and preserves the original format by default.


## Install

With Homebrew:

```bash
brew install dpep/tools/tztr
```

With RubyGems:

```bash
gem install tztr
```


## Usage

```bash
echo '2026-04-03T12:00:00Z' | tztr -t America/Los_Angeles
# 2026-04-03T05:00:00-07:00

echo '15:30 UTC' | tztr -t America/New_York
# 11:30 EDT

tail -f app.log | tztr
```

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

`--detect` reports what was found without translating (omits `translated`), and
composes with `-j`/`-J`:

```bash
echo '2026-04-03T12:00:00Z' | tztr --detect -j
# [ { "original": "...", "detected_format": "iso", "detected_tz": "Z" } ]
```

### Environment

Set `TZ` to change the default output timezone (overridden by `-t`):

```bash
export TZ=America/Los_Angeles
echo '2026-04-03T12:00:00Z' | tztr
# 2026-04-03T05:00:00-07:00
```

### Supported Formats

- ISO 8601: `2026-04-03T12:00:00Z`, `2026-04-03T12:00:00+05:30`
- Date + time: `2026-04-03 12:00:00 UTC`
- Time only: `15:30 UTC`, `08:30:45 PDT`
- Fractional seconds: `2026-04-03T12:00:00.123Z`

### Caveat: time-only inputs and DST

A time-only input (e.g. `15:30 PST`) carries no date, so there's no way to know
whether it fell during standard or daylight time. By default `tztr` resolves it
against **today's** date, which can be off by an hour for timestamps from the
other side of a DST boundary. Inputs with a date (`2026-04-03 15:30 PST`) are
unaffected.

Two ways to handle it:

```bash
# Supply the date the times belong to (accepts flexible formats):
echo '15:30 PST' | tztr -t utc -d 2026-01-15
echo '15:30 PST' | tztr -t utc -d 'January 15, 2026'

# Or sidestep DST entirely with a fixed numeric offset instead of a named zone:
echo '15:30' | tztr -f -8 -t utc   # -8 -> Etc/GMT+8, never observes daylight time
```


## Library

```ruby
require "tztr"

Tztr.translate("log 2026-04-03T12:00:00Z event", to: "America/Los_Angeles")
# => "log 2026-04-03T05:00:00-07:00 event"
```


----
## Contributing

Yes please  :)

1. Fork it
1. Create your feature branch (`git checkout -b my-feature`)
1. Ensure the tests pass (`bundle exec rspec`)
1. Commit your changes (`git commit -am 'awesome new feature'`)
1. Push your branch (`git push origin my-feature`)
1. Create a Pull Request

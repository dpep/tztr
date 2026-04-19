tztr
======
![Gem](https://img.shields.io/gem/dt/tztr?style=plastic)
[![codecov](https://codecov.io/gh/dpep/tztr/branch/main/graph/badge.svg)](https://codecov.io/gh/dpep/tztr)

Timezone Translator - convert timestamps to local time.

Reads from stdin or file, auto-detects timestamp formats, and preserves the original format by default.


## Install

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
-t, --to TZ         Output timezone (default: UTC)
-F, --format FMT    Output format: iso, short, time (default: preserve input)
-v, --version       Show version
-h, --help          Show this help
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

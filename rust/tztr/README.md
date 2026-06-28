# tztr (Rust)

Timezone Translator — convert timestamps between timezones. Reads from stdin or
a file, auto-detects timestamp formats, and preserves the original format by
default.

This is the Rust implementation. It is a port of the [Ruby reference
implementation](https://github.com/dpep/tztr) and is kept functionally
identical — same detection, format preservation, aliases, and DST handling
(both read the system tz database).

## Install

```bash
cargo install tztr
# or, via Homebrew:
brew install dpep/tools/tztr
```

## Usage

```bash
echo '2026-04-03T12:00:00Z' | tztr -t America/Los_Angeles
# 2026-04-03T05:00:00-07:00

echo '15:30 UTC' | tztr -t nyc
# 11:30 EDT

# Structured output for scripts/agents:
echo '15:30 UTC' | tztr -t pst -j
tail -f app.log | tztr -t nyc -J

# Report detected format/zone without converting:
echo '2026-04-03T12:00:00Z' | tztr --detect

# Reference date so time-only inputs resolve DST correctly:
echo '15:30 PST' | tztr -t utc -d 2026-01-15
```

Run `tztr -h` for all options.

## Library

```rust
use tztr::translate;

let out = translate("log 2026-04-03T12:00:00Z event", "America/Los_Angeles", None, None, false, None);
assert_eq!(out, "log 2026-04-03T05:00:00-07:00 event");
```

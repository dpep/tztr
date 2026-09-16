# Ruby → Rust port notes

`tztr` ships two implementations from one repo: the Ruby gem (reference) and
this Rust crate. They are kept functionally identical; CI runs a CLI parity
harness (`script/parity.rb`) that diffs both binaries across a matrix of
inputs, args, and `TZ` values.

## Approach

- **One crate, library + binary** (`src/lib.rs` + `src/main.rs`), the idiomatic
  Rust shape (cf. ripgrep). `cargo install tztr` builds the CLI; `tztr = "0.x"`
  pulls the library.
- **Timezone math via [`jiff`](https://docs.rs/jiff).** jiff reads the system
  tz database — the same source Ruby's `Time` uses via `ENV['TZ']` — so DST
  transitions and zone abbreviations match without bundling tzdata into the
  binary.
- **Hand-rolled arg parsing** (no clap) to mirror Ruby's `OptionParser`
  surface exactly and keep the dependency tree small.
- **`resolve_tz` is the validating boundary.** It returns
  `Result<String, TzError>`; a zone that does not resolve is an error the CLI
  exits on, never a silent fall back to UTC that answers in the wrong zone.

## Zone detection

Both implementations detect zone abbreviations from an explicit allowlist
(`ZONE_ABBREVIATIONS`), not a `[A-Z]{2,4}` wildcard. The wildcard used to eat
log levels (`15:30 INFO server started` lost the `INFO`) and meridiems, and it
matched abbreviations the parser then silently ignored — `15:30 JST` came out
as a *local* time, nine hours wrong, with no signal.

The list is the union of the abbreviations Ruby's `Time.parse` resolves
natively (`UT UTC GMT` plus E/C/M/P × ST/DT) and the abbreviation-shaped keys of
`TIMEZONE_ALIASES` (`Z ET CT MT PT HST AKST AKDT CET CEST BST IST JST KST HKT
AEST AEDT NZST NZDT`). City nicknames (`sf`, `nyc`) are deliberately absent:
they are `-t`/`-f` values, not things to look for inside text.

How a detected abbreviation resolves depends on which half it came from:

- The native ones are **fixed offsets** — `PST` is always −08:00, whatever the
  date, because that is what `Time.parse` does.
- The rest resolve **through the alias table to a real zone**, so they carry DST
  rules: `15:30 CET` is 14:30 UTC in January and 13:30 UTC in July.

## Quirks replicated for parity

- **`from` is bypassed when an embedded zone is present**, matching Ruby's
  branch order. `-f pst` has no effect on `12:00 JST`.
- **"Today" fills date-less inputs**, taken in the zone the timestamp is
  expressed in. This is the documented DST caveat for time-only inputs;
  `-d/--date` supplies the missing date, and `-v` now says out loud when the
  assumption is being made.
- **Ambiguous wall-clock times take the earlier occurrence.** A repeated hour at
  a DST fall-back resolves to the daylight side — jiff's `compatible`
  disambiguation, which also matches macOS `date(1)`, Temporal, RFC 5545 and
  ICU. Ruby was changed to agree; there is a test pinning it on this side so a
  jiff default change cannot move it silently.

## Accepted divergences

Deliberate, and excluded from the parity matrix:

- **Long-option abbreviation and `-F` value completion.** Ruby's `OptionParser`
  accepts any unambiguous prefix (`--js`, `--det`, `-F i`) for free. The Rust
  parser requires exact spellings. Making Ruby strict would mean fighting
  OptionParser; making Rust lenient is code nobody asked for. `--` itself *is*
  supported on both sides.

Note that the parity harness compares **stdout and exit status only**
(`script/parity.rb` discards stderr), so the error strings below are not covered
by the gate and have to be kept in step by hand.
## Error strings

One line, no backtrace, exit 1, byte-identical with Ruby. The `-i` path uses the
same wording as the streaming path — no filename prefix:

    tztr: unknown timezone: Bogus/Zone
    tztr: offset out of range: 15 (expected -12..14)
    tztr: invalid date: 2026-02-30
    tztr: No such file or directory (os error 2)
    tztr: Is a directory (os error 21)
    tztr: invalid option: --bogus

## Known defect, deferred

`EST` means two different things. Inside text it is a fixed −05:00 (that is what
`Time.parse` does), but as `-f est` it resolves through the alias table to
`America/New_York` and is DST-aware — the same token, an hour apart in summer.
Same class as the abbreviation bug above; it needs its own parity pass and was
deliberately left alone this round.

## Performance

Measured on Apple Silicon (release build, system tzdb). Indicative, not a
rigorous benchmark.

| Metric | Ruby | Rust | Speedup |
|---|---|---|---|
| Startup (per invocation, single line) | ~44 ms | ~6.4 ms | ~6.8× |
| Throughput (100k lines, 2 timestamps each) | 1.19 s | 0.19 s | ~6.2× |
| Throughput (same, `-J` ndjson) | 1.32 s | 0.25 s | ~5.2× |

**Binary size:** the Rust release binary is ~1.9 MB, self-contained (no Ruby
runtime needed). Because jiff uses the system tzdb, no timezone data is baked
in. The Ruby implementation has no standalone binary — it needs a Ruby
interpreter plus the (tiny) gem.

Reproduce with `make build` then `ruby script/parity.rb` for correctness, or the
commands in this repo's history for the timings.

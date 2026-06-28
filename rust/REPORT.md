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

## Quirks replicated for parity

The Rust port faithfully reproduces a few non-obvious behaviors of the Ruby
reference (which leans on `Time.parse`):

- **Unrecognized zone abbreviations are ignored.** `12:00 CET` (CET isn't in
  Ruby's `Time.parse` zone table) is parsed as if it were in the *target* zone,
  not CET. Only `UTC/GMT/UT/Z` and the US abbreviations (E/C/M/P × ST/DT) plus
  numeric offsets are recognized.
- **`from` is bypassed when an embedded zone is present** — even an
  unrecognized one — matching Ruby's branch order.
- **"Today" fills date-less inputs**, taken in the same zone Ruby's `ENV['TZ']`
  would be set to for that branch. This is the documented DST caveat for
  time-only inputs; `-d/--date` supplies the missing date.

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

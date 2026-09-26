# Ruby → Rust port notes

`tztr` ships two implementations from one repo: the Ruby gem (reference) and
this Rust crate. They are kept functionally identical; CI runs a CLI parity
harness (`script/parity.rb`) that diffs both binaries' stdout, stderr and exit
status across a matrix of inputs, args, and `TZ` values.

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
natively (`UT UTC GMT Z` plus E/C/M/P × ST/DT) and the abbreviation-shaped keys
of `TIMEZONE_ALIASES` (`ET CT MT PT HST AKST AKDT CET CEST BST IST JST KST HKT
AEST AEDT NZST NZDT`). City nicknames (`sf`, `nyc`) are deliberately absent:
they are `-t`/`-f` values, not things to look for inside text.

Each is matched uppercase, or wholly lowercase unless the lowercase spelling
is a word likely to follow a time (`WORD_ABBREVIATIONS`: French `est`/`cet`/`et`,
German `ist`, plus `ut` and `z`). Mixed case never matches. A lowercase match
resolves exactly as its uppercase form: `pst` is a fixed −08:00 like `PST`.

Every abbreviation that names standard or daylight time is a **fixed offset**
(`ZONE_OFFSETS` / `zone_offset_seconds`): `PST` is always −08:00 and `CEST`
always +02:00, whatever the date, as `Time.parse` always read the US ones.
Only the generic `ET`, `CT`, `MT` and `PT` resolve **through the alias table
to a real zone**, so they follow DST.

**The source zone overrides the table.** The abbreviations the source zone
(`-f`, else `$TZ`) itself uses are read in its sense: Ruby's
`local_abbreviations` samples the zone's `%Z` in mid-January and mid-July of
the current year, and Rust mirrors it. With `TZ=Asia/Shanghai`, `CST` is
+08:00; with `TZ=Europe/Helsinki`, `EEST` (not in the list at all) converts.
A US, UTC or unset source zone uses no abbreviation that differs from the
table, so nothing changes there.

**An unknown zone leaves the timestamp alone.** A `date`-shaped line accepts
any capitalized word as its zone (`DATE_ZONE`), so that one tztr can't resolve
(`WIB`) leaves the whole date as written instead of half-converting it; `-v`
names it.

## Quirks replicated for parity

- **`from` is bypassed when an embedded zone is present**, matching Ruby's
  branch order. `-f pst` has no effect on `12:00 JST`.
- **"Today" fills date-less inputs**, taken where the timestamp was written:
  the zone it names, else the source zone. Both implementations compute it
  explicitly per timestamp (Ruby's `today_where`, Rust's `anchor`) rather than
  leaving it to `Time.parse`, so a range's rolled end shares its start's date
  and `-v` names the date actually used. `-d/--date` supplies it instead.
- **Ambiguous wall-clock times take the earlier occurrence.** A repeated hour at
  a DST fall-back resolves to the daylight side — jiff's `compatible`
  disambiguation, which also matches macOS `date(1)`, Temporal, RFC 5545 and
  ICU. Ruby was changed to agree; there is a test pinning it on this side so a
  jiff default change cannot move it silently.

## Option parsing

Both sides require exact spellings. Ruby's `OptionParser` would accept any
unambiguous prefix (`--js`, `-F i`); `bin/tztr` sets `require_exact` and
validates `-F` itself, so `--jso` and `-F i` are errors in both builds. `--`
ends the options in both.

## Error strings

One line, no backtrace, exit 1, byte-identical with Ruby (the parity harness
checks stderr). An error **about a particular file names it**, in both the
streaming and the `-i` path, since with several file arguments the bare
message would not say which one failed. That file is skipped and the run
carries on:

    tztr: app.log: No such file or directory (os error 2)
    tztr: app.log: Is a directory (os error 21)

Everything else stays bare, and stops the run:

    tztr: unknown timezone: Bogus/Zone
    tztr: offset out of range: 15 (expected -12..14)
    tztr: invalid date: 2026-02-30
    tztr: invalid format: i (expected iso, short, time)
    tztr: invalid option: --bogus
    tztr: missing argument for -t
    tztr: --json takes no argument
    tztr: -i requires a file argument
    tztr: -i cannot be combined with --json/--ndjson/--detect
    tztr: -i cannot be combined with now

## Known asymmetry

An abbreviation means two different things depending on where it appears.
Inside text, `EST` is a fixed −05:00 (as `Time.parse` reads it); as `-f est` or
`-t est` it resolves through the alias table to `America/New_York` and follows
DST, so the same token is an hour apart in summer. The README documents this
as intended (`-t est` is New York time). Changing either side needs its own
parity pass.

## Performance

Measured on Apple Silicon (release build, system tzdb, Ruby 4.0), single
runs on a machine that wasn't quiet. Indicative, not a rigorous benchmark.

| Metric | Ruby | Rust | Speedup |
|---|---|---|---|
| Startup (per invocation, single line) | ~50 ms | ~6 ms | ~8× |
| Throughput (100k lines, 2 timestamps each) | 18 s | 0.64 s | ~29× |
| Throughput (same, `-J` ndjson) | 24 s | 0.97 s | ~25× |

The 0.1.0 figures were 1.2 s (Ruby) and 0.19 s (Rust) for the same shape of
input. Both slowed as detection grew, Ruby by far the most.

**Binary size:** the Rust release binary is ~2.1 MB, self-contained (no Ruby
runtime needed). Because jiff uses the system tzdb, no timezone data is baked
in. The Ruby implementation has no standalone binary — it needs a Ruby
interpreter plus the (tiny) gem.

Reproduce with `make parity` for correctness, or the commands in this repo's
history for the timings.

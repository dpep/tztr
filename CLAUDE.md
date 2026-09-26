# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`tztr` is a small CLI + library that translates timestamps between timezones. It auto-detects timestamp formats in arbitrary text, converts them, and preserves the surrounding text and original format by default.

- Library entry point: `lib/tztr.rb` (single file, `Tztr` module)
- Executable: `bin/tztr` (uses OptionParser, streams stdin/files line-by-line)
- Required Ruby: `>= 3.2`. CI runs against 3.3, 3.4, and 4.0.

## Dual implementation — Ruby (reference) + Rust (port)

This repo ships **two implementations kept functionally identical**: the Ruby
gem at the root (`lib/`, `bin/`) and a Rust crate under `rust/` (`cargo install
tztr`; Homebrew installs the Rust binary). Ruby is the **reference**; Rust
mirrors it.

**Parity is the contract.** Any behavior change must land in *both* and keep the
CLI outputs identical:
1. Change Ruby (`lib/tztr.rb` / `bin/tztr`), add/adjust specs, `bundle exec rspec`.
2. Port the change to Rust under `rust/tztr/`, add/adjust tests, `make check`
   (`cargo fmt --check` + clippy `-D warnings` + `cargo test`).
3. `make parity` (or `ruby script/parity.rb`) — the Ruby ↔ Rust CLI parity
   harness diffs both binaries across a matrix of inputs/args/`TZ`; JSON modes
   compared semantically, everything else byte-for-byte. Must be 100%.

CI (`.github/workflows/rust.yml`) runs the Rust gate + parity; `make hooks`
installs a pre-push hook that runs rspec + `make check` + `make parity`.
Timezone math in Rust uses `jiff` (system tzdb, same source as Ruby's `Time`),
so DST matches. Watch the replicated `Time.parse` quirks documented in
`rust/REPORT.md` (unrecognized abbreviations ignored; `from` bypassed when an
embedded zone is present).

## Commands

```bash
bundle install                 # install deps
bundle exec rspec              # run all tests
bundle exec rspec spec/tztr_spec.rb:42   # run a single test by line number
bin/tztr ...                   # run the CLI from a working copy (no install needed)
gem build tztr.gemspec         # build the gem
```

There is no Rubocop / linter configured — only RSpec + SimpleCov. `--require spec_helper` is set in `.rspec`, so specs don't need to require it explicitly.

## Architecture notes

A few things that aren't obvious from a quick read:

**`Tztr.translate` mutates `ENV['TZ']`.** Both `translate` and `parse` set `ENV['TZ']` as a side effect to coerce Ruby's `Time` parsing into the right zone. The spec helper resets `ENV['TZ'] = 'UTC'` in a `before(:each)` to keep tests isolated — anything new that exercises parsing should rely on that, or save/restore `TZ` itself.

**Pattern order is priority, not exclusivity.** `PATTERNS` in `lib/tztr.rb` is joined into one alternation (`TIMESTAMP`, and `timestamp()` in Rust) and scanned in a single pass, so every timestamp on a line converts whatever its format. At each position the alternatives are tried top-to-bottom, so more specific patterns (ISO with timezone) must come before less specific ones (bare time) or a shorter format will match inside a longer one. A bare time (`BARE_TIME`) beside a timestamp that names its date or zone is left alone as a probable duration (`Tztr.timestamps`); only a line of nothing more specific converts its bare times. Before that, each member of a range or list (`3:30 to 4:45 PM PST`, `3:00, 4:00 or 5:00 PM`; `RANGE_JOIN` / `LIST_JOIN`) takes the zone and meridiem after it, and `-j` reports the members as `group`. A bare hour counts only as the start of a range (`RANGE_HOUR`, the 9 of `9-9:15am`). A numeric offset glued to a time needs seconds, so `15:30-16:45` is a range. When adding a new format, place it carefully and add tests covering lines that mix formats.

**A match's text and its reading differ.** Each match is a `Stamp` with the `text` it replaces and the `effective` reading it is parsed as (`Tztr.reading`): a named-month date (date(1), RFC 2822, CLF) normalized to `YYYY-MM-DD`, `9am` to `9:00am`, a slashed date dashed, a comma fraction dotted, plus whatever zone, meridiem or date it shares with its range. Parse the reading; format from the text, so the output keeps the input's shape.

**Zones inside text are fixed offsets; zones given to `-f`/`-t` are IANA.** `CEST` in a line is always +02:00 (`ZONE_OFFSETS`), but `-t est` is New York with DST. Only the generic `ET`/`CT`/`MT`/`PT` follow DST inside text. An abbreviation the source zone itself uses this year wins over the table (`local_abbreviations`): `CST` under `TZ=Asia/Shanghai` is +08:00; under any zone that doesn't use it, the US-centric table stands. A dateless timestamp's date is today *where it was written* (`today_where` in Ruby, `anchor` in Rust), never Time.parse's own guess.

**Timezone resolution has three layers** (`Tztr.resolve_tz`):
1. Numeric offset string (e.g. `"-7"`) → `Etc/GMT±N` — note the POSIX sign inversion (`-7` becomes `Etc/GMT+7`).
2. Lowercased + underscored lookup in `TIMEZONE_ALIASES` (covers tz abbreviations like `pst`, plus city nicknames like `sf`, `nyc`).
3. Validated against the system tzdb as an IANA name like `America/Los_Angeles` — an unknown name raises `Tztr::Error` rather than passing through.

**Output format preservation** (`format_time`) inspects the *original* matched substring and rebuilds the output to mirror it (ISO `T`, space-separated, time-only, with/without fractional seconds). Explicit `--format iso|short|time` short-circuits this.

**CLI streaming.** `bin/tztr` sets `$stdout.sync = true` and processes input line-by-line so it works with `tail -f`. `-i/--in-place` reads, translates, and writes back only if content changed.

**Structured output is agent-facing.** `-j/--json` (array) and `-J/--ndjson` (one object per line, streaming-friendly) emit `{original, detected_format, detected_tz, translated}` per match, backed by `Tztr.matches`. `--detect` reports format/zone only (omits `translated`). **Directive: every new CLI option must work in `-j`/`-J` modes** — when adding a flag, make sure it composes with structured output (e.g. `-F` shapes the `translated` field) and add a spec covering it. `-i` is the one exception: it's mutually exclusive with `-j`/`-J`/`--detect` and aborts.

## Testing beyond the suites

The parity harness only proves the two builds agree; several rounds of bugs were ones both builds shared. `script/parity.rb` has a `golden` group of absolute expectations for exactly those. Add to it whenever a bug is found in both builds at once.

User-testing rounds (logs and reports archived locally under `research/`, which is gitignored) found the most with these kinds of input. Reuse them for any matching change:

- **Real machine logs, read-only**: macOS `/var/log/*.log`, `log show --style syslog|default`, `ls -lT`, `git log` (`--format=fuller`, `--date=iso|rfc`), real `date`, `date -u`, `date -R` and `TZ=<far zone> date` output, and Rails `log/*.log` and nginx access/error logs found in local checkouts.
- **Formats from memory, where no real sample exists**: Lograge, ActiveJob, Sidekiq JSON/text, RSpec output (`Finished in 1:05 minutes`), crontab lines and cron syslog, RFC 5424/journald, Postgres, Docker/k8s RFC3339Nano, GitHub Actions, Go's `log` package, Python `logging` (comma milliseconds), log4j, multi-field JSON/NDJSON.
- **Prose**: meeting invites, standup notes and on-call handoffs with ranges and lists written every way people write them (`9-9:15am`, `3:00, 4:00 or 5:00 PM`, `11:30 PM to 12:30 AM`), deadlines next to dates (`Apr 3 - 5pm`), and French and German text (`à 15:30 est annulée`).
- **Things that aren't timestamps**: durations, line numbers (`foo.rb:42:10`), `ip:port`, versions, ratios, scores, cron fields, ticket numbers (`#12 - 9:30am`).
- **Adversarial input**: Latin-1 and invalid UTF-8 bytes beside a match, fullwidth and Arabic-Indic digits, glued offsets (`15:30-1645`), impossible clocks (`99:14`), and 100k+ character lines, timed to catch quadratic paths.
- **Date boundaries**: midnight, month, leap-day and year ends toward far zones (`Pacific/Kiritimati` +14, `Pacific/Pago_Pago` −11, `Asia/Kathmandu` +5:45), DST transition days in the US, UK and Australia, and anything that depends on today's date (run with `-t 14` and `-t -12`).

## Release / distribution

Distributed via RubyGems (`gem install tztr`) and Homebrew (`brew install dpep/tools/tztr`). Version lives in `lib/tztr/version.rb`. Dependabot auto-approves and auto-merges minor/patch dependency PRs (see `.github/workflows/dependabot.yml`).

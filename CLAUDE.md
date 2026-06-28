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

**Pattern matching is ordered and first-match-wins.** `PATTERNS` in `lib/tztr.rb` is iterated top-to-bottom; the first regex that matches the line is the *only* one used (`break result` after `gsub!`). More specific patterns (ISO with timezone) must come before less specific ones (bare time). When adding a new format, place it carefully and add tests covering ambiguous lines.

**Timezone resolution has three layers** (`Tztr.resolve_tz`):
1. Numeric offset string (e.g. `"-7"`) → `Etc/GMT±N` — note the POSIX sign inversion (`-7` becomes `Etc/GMT+7`).
2. Lowercased + underscored lookup in `TIMEZONE_ALIASES` (covers tz abbreviations like `pst`, plus city nicknames like `sf`, `nyc`).
3. Pass-through — assumed to be a valid IANA name like `America/Los_Angeles`.

**Output format preservation** (`format_time`) inspects the *original* matched substring and rebuilds the output to mirror it (ISO `T`, space-separated, time-only, with/without fractional seconds). Explicit `--format iso|short|time` short-circuits this.

**CLI streaming.** `bin/tztr` sets `$stdout.sync = true` and processes input line-by-line so it works with `tail -f`. `-i/--in-place` reads, translates, and writes back only if content changed.

**Structured output is agent-facing.** `-j/--json` (array) and `-J/--ndjson` (one object per line, streaming-friendly) emit `{original, detected_format, detected_tz, translated}` per match, backed by `Tztr.matches`. `--detect` reports format/zone only (omits `translated`). **Directive: every new CLI option must work in `-j`/`-J` modes** — when adding a flag, make sure it composes with structured output (e.g. `-F` shapes the `translated` field) and add a spec covering it. `-i` is the one exception: it's mutually exclusive with `-j`/`-J`/`--detect` and aborts.

## Release / distribution

Distributed via RubyGems (`gem install tztr`) and Homebrew (`brew install dpep/tools/tztr`). Version lives in `lib/tztr/version.rb`. Dependabot auto-approves and auto-merges minor/patch dependency PRs (see `.github/workflows/dependabot.yml`).

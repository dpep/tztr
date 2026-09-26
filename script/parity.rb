#!/usr/bin/env ruby
# frozen_string_literal: true

# Ruby <-> Rust CLI differential tester. Runs a matrix of (args, stdin, TZ,
# files) through both binaries and diffs stdout, stderr, exit code, and any
# files the run touched -- all byte-for-byte. Exits non-zero on any mismatch.
#
#   make parity                      # preferred: picks a Ruby >= 3.2 for you
#   ruby script/parity.rb            # uses rust/target/release/tztr
#   RUST_BIN=path ruby script/parity.rb
#   PARITY_JOBS=1 ruby script/parity.rb        # serial, for debugging
#   PARITY_GROUPS=errors,files ruby script/parity.rb   # run a subset
#
# Build the Rust binary first: cargo build --release --manifest-path rust/Cargo.toml
#
# Adding coverage: append to a GROUP section below. A case is a differential
# check by default; give it `expect:` to also assert an absolute expectation
# (see the "golden" group -- for behaviors where "both agree" isn't enough
# because both could be wrong the same way).
#
# This file must PARSE on Ruby 2.6 even though it refuses to RUN there: Ruby
# parses the whole file before executing a line, so modern syntax would bury
# the version guard below under syntax errors. Hence no hash-value omission
# (`foo:` for `foo: foo`) and nothing else newer than 2.6 above the guard.

require "etc"
require "fileutils"
require "json"
require "open3"
require "rbconfig"
require "tmpdir"

# --- Ruby version guard -------------------------------------------------------
# bin/tztr's `#!/usr/bin/env ruby` and a bare `ruby` both resolve to macOS
# system Ruby 2.6 when rbenv's shims aren't on PATH. That produces a wall of
# "failures" that have nothing to do with parity, so refuse to run at all.
MIN_RUBY = "3.2"
if Gem::Version.new(RUBY_VERSION) < Gem::Version.new(MIN_RUBY)
  abort <<~MSG
    script/parity.rb: needs Ruby >= #{MIN_RUBY}, got #{RUBY_VERSION}
      interpreter: #{RbConfig.ruby}
    The gemspec requires >= #{MIN_RUBY}; running the harness on an older Ruby reports
    parity failures that are really just bin/tztr failing to load.
    Use `make parity` (it finds a qualifying Ruby), or point it at one yourself:
      make parity RUBY=/path/to/ruby
  MSG
end

ROOT = File.expand_path("..", __dir__)
require_relative "../lib/tztr" # for TIMEZONE_ALIASES -- the alias sweep stays current on its own

RUST_BIN = ENV["RUST_BIN"] || File.join(ROOT, "rust", "target", "release", "tztr")
abort "rust binary not found: #{RUST_BIN} (build it first)" unless File.executable?(RUST_BIN)

# Invoke the Ruby CLI through *this* interpreter rather than its shebang, so the
# version guard above actually governs what runs and PATH cannot swap it out.
RUBY_CMD = [RbConfig.ruby, File.join(ROOT, "bin", "tztr")].freeze
RUST_CMD = [RUST_BIN].freeze

# --- Case model ---------------------------------------------------------------
# args    - argv. Runs in a scratch dir when files/dirs are given, so any path a
#           binary prints is relative and identical on both sides.
# stdin   - exact bytes on stdin (no newline is appended for you)
# tz      - TZ value, or :unset to remove it from the environment
# files   - { "name" => contents } created before the run
# dirs    - directory names created before the run
# expect  - optional ->(stdout, stderr, exitstatus) checked against both binaries
CASES = []

def add(group:, args: [], stdin: "", tz: "UTC", files: nil, dirs: nil, expect: nil)
  CASES << { group: group, args: args, stdin: stdin, tz: tz, files: files, dirs: dirs, expect: expect }
end

# ==============================================================================
# GROUP: core -- the (TZ x args x line) cross product
# ==============================================================================

CORE_ENVS = ["UTC", "America/Los_Angeles", "America/New_York"].freeze

CORE_LINES = [
  # --- well-formed, in-range
  "2026-04-03T12:00:00Z",
  "2026-04-03T12:00:00.123Z",
  "2026-04-03T05:00:00-07:00",
  "2026-04-03 12:00:00 UTC",
  "2026-04-03 12:00:00 PST",
  "2026-07-15 12:00:00 EST",
  "2026-04-03T12:00:00",
  "15:30 UTC",
  "15:30:45 UTC",
  "08:30 PDT",
  "12:34:56",
  "12:34",
  "log 2026-04-03T12:00:00Z something happened",
  "from 15:30 UTC to 16:45 UTC",
  "no timestamps here",
  "meeting at 12:00 EST and 09:00 PST",
  "12:00 CET",
  "2026-12-25 23:59:59 UTC",

  # --- DST transitions: the ambiguous hour, the nonexistent hour
  "2026-11-01 01:30:00",       # US fall back -- this wall clock happens twice
  "2026-11-01 01:30:00 EST",
  "2026-10-25 02:30:00",       # EU fall back
  "2026-03-08 02:30:00",       # US spring forward -- this wall clock never happens
  "2026-03-08 02:30:00 PST",

  # --- out-of-range / impossible components
  "24:00",
  "23:59:60",
  "2026-06-30 23:59:60 UTC",
  "2026-02-29T12:00:00Z",      # 2026 is not a leap year
  "2026-02-30T12:00:00Z",
  "2026-04-03T12:00:00.123456789Z",

  # --- zone-abbreviation allowlist: log levels must survive, real zones convert
  "15:30 INFO server started",
  "15:30 WARN disk low",
  "2026-04-03 12:00:00 ERROR db failed",
  "15:30 JST",
  "15:30 CET",
  "15:30 AEST",

  # --- 12-hour times
  "11:30:00 PM",
  "12:30:00 AM",
  "12:30:00 PM",
  "1:00:00 PM",
  "2026-04-03 03:45:00 PM",
  "3:45 PM PST",
  "11:30 p.m.",
  "at 11:30 A.M. sharp",
  "3:45 P.M. PST",
  "11:30 a.m",
  "It starts at 11:30 PM.",

  # --- lowercase abbreviations: resolved like uppercase, unless also a word
  "15:30 utc",
  "3:45 pm pst",
  "12:00 pdt",
  "15:30 jst",
  "2026-04-03 12:00:00 cest",
  "15:30 Pst",
  "à 15:30 est annulée",
  "à 15:30 cet après-midi",
  "um 15:30 ist es",
  "15:30 et al",

  # --- several formats on one line: every one converts
  %q({"ts":"2026-04-03T12:00:00Z","created":"2026-04-03 13:00:00 UTC","msg":"at 15:30 UTC"}),
  "15:30 UTC then 2026-04-03T12:00:00Z",
  "2026-04-03T12:00:00 and 2026-04-03 12:00:00 and 12:00",
  "3:45 PM PST, 2026-04-03T12:00:00+05:30, 15:30 JST, 09:00",
  "2026-04-03T12:00:00Z took 0:05",
  "15:30 UTC, retry in 0:30",
  "from 15:30 to 16:45",
  "3:45 PM and 16:00",
  "2026-04-03 12:00:00 then 09:00:00",
  "meeting 3:30 PM, took 0:05",

  # --- ranges share the trailing zone and meridiem; offsets need seconds
  "from 15:30 to 16:45 PST",
  "from 3:30 to 4:45 PM",
  "from 3:30 to 4:45 PM PST",
  "11:30 to 1:00 PM PST",
  "10:00 until 2:00 AM PST",
  "from 15:30 to 4:45 PM PST",
  "15:30-16:45 PST",
  "15:30 – 16:45 JST",
  "9:00 TO 5:00 p.m. CET",
  "1:00 to 2:00 to 3:00 PM",
  "15:30, then 16:45 PST",
  "3:30 PM to 4:45 PM PST",
  "3:30 PST to 4:45 PM",
  "15:30-16:45",
  "12:34:56-05:00",
  "12:34:56-16:45",
  "12:34-05:00",

  # --- lists share like ranges; hour-only times; mixed-case zones
  "Options at 3:00, 4:00 or 5:00 PM PST",
  "at 3:00 and 4:00 PM",
  "11:00, 12:00, or 1:00 PM PST",
  "between 3:30 and 4:45 PM",
  "2026-04-03T12:00:00Z,15:30",
  "0:05, 3:00 PM",
  "Meeting at 9am PST",
  "at 9 PM",
  "at 9 p.m. sharp",
  "Standup 9-9:15am PST",
  "9 to 10am PST",
  "11-1pm PST",
  "9–10 AM JST",
  "won 3-10",
  "page 9, 10am standup",
  "v1.9-10am",
  "item 42-10am",
  "15:30 Pst",
  "3:30 to 4:45 PM Pst",
  "15:30 Utc and 16:00 utc",

  # --- named-month dates: date(1)/ctime and RFC 2822
  "Fri Sep 25 22:14:42 PDT 2026",
  "Fri Sep 25 22:14:42 2026",
  "Sat Sep  5 22:14:42 UTC 2026",
  "Fri Sep 25 22:14:42 JST 2026",
  "Fri Sep 25 22:14:42 pdt 2026",
  "Mon Feb 30 12:00:00 UTC 2026",
  "Sun Nov  1 01:30:00 2026",
  "Fri, 25 Sep 2026 22:14:42 -0700",
  "25 Sep 2026 22:14 PDT",
  "Date: Sat, 5 Sep 2026 12:00:00 GMT",
  "Fri, 25 Sep 2026 22:14:42 +0530 then 0:05",

  # --- date boundaries: dated timestamps without seconds; ranges past midnight
  "2026-01-15 23:30",
  "2026-01-15 23:30 UTC",
  "2026-12-31T23:30+05:30",
  "2026-12-31T23:30Z",
  "2026-12-31T23:30",
  "2026-04-03 3:45 PM",
  "2026-02-28T23:30-10:00",
  "2028-02-28T23:30-10:00",
  "11:30 PM to 12:30 AM PST",
  "11:30 PM, 12:15 AM or 1:00 AM PST",
  "22:00-02:00 UTC",
  "9:00 to 17:00 UTC",
  "23:00 to 01:00 to 03:00",

  # --- fixed abbreviations, dated starts, single-digit hours, glued offsets
  "15:30 CET",
  "15:30 CEST",
  "15:30 BST",
  "15:30 AEDT",
  "15:30 PT",
  "2026-07-15 15:30 CET",
  "2026-04-03 9:00 AM - 10:00 AM PST",
  "2026-04-03 15:30 - 16:30",
  "2026-04-03 23:00 - 01:00",
  "2026-04-03T12:00:00Z - 13:00",
  "2026-01-15 9:00 UTC",
  "2026-01-15T9:00:00Z",
  "15:30-1645",
  "12:00+0530",
  "12:00 +0530",
  "12:00:00-14:59",
  "12:00:00+14:00",
  "2026-04-03 12:00:00 -1645",
  "2026-09-25 99:14:42 PDT",
  "99am",
  "Fri Sep 25 25:14:42 PDT 2026",
].freeze

CORE_ARGS = [
  [],
  ["-t", "America/Los_Angeles"],
  ["-t", "sf"],
  ["-t", "nyc"],
  ["-t", "utc"],
  ["-t", "gmt"],
  ["-t", "-7"],
  ["-t", "+9"],
  ["-f", "America/Los_Angeles", "-t", "UTC"],
  ["-f", "sf", "-t", "tokyo"],
  ["-t", "America/Los_Angeles", "-F", "iso"],
  ["-t", "America/Los_Angeles", "-F", "short"],
  ["-t", "America/Los_Angeles", "-F", "time"],
  ["-t", "pst", "-j"],
  ["-t", "pst", "-J"],
  ["--detect"],
  ["--detect", "-j"],
  ["--detect", "-J"],
  ["-t", "utc", "-d", "2026-01-15"],
  ["-t", "utc", "-d", "2026-07-15"],
  ["-t", "utc", "-d", "January 15, 2026"],
  ["-t", "nyc", "-d", "2026-11-01"],   # reference date *is* the DST transition
  ["-t", "nyc", "-d", "2026-03-08"],
  ["-h", "-j"],
  ["-h", "-J"],
  ["-hj"],                             # bundled short flags
  ["-tsf"],                            # value attached to a bundled flag
  ["-vj"],
  ["-v", "-t", "utc"],                 # stderr disclosure
  ["--detect", "-v"],                  # detection assumes nothing
  ["-v", "-f", "utc", "-t", "pst"],    # explicit -f => startup line, no disclosure
  # -F crossed with the structured modes (CLAUDE.md: every option must work in -j/-J)
  ["-F", "short", "-t", "utc", "-j"],
  ["-F", "iso", "-t", "pst", "-j"],
  ["-F", "time", "-t", "pst", "-J"],
  # long forms and --opt=value
  ["--to", "sf"],
  ["--from", "utc", "--to", "nyc"],
  ["--to=sf"],
  ["--format=short", "--to=utc"],
  ["--json", "--to", "pst"],
  ["--ndjson", "--format", "iso", "--to", "pst"],
  ["--verbose", "--to", "utc"],
  ["--date=2026-01-15", "--to=utc"],
  ["-t", "nyc", "-d", "2026-11-01", "-j"],  # -d must shape -j too
  ["--detect", "-F", "iso"],                # -F has nothing to shape under --detect
].freeze

CORE_ENVS.each do |tz|
  CORE_ARGS.each do |args|
    CORE_LINES.each { |line| add(group: "core", args: args, stdin: "#{line}\n", tz: tz) }
  end
end

# ==============================================================================
# GROUP: payloads -- stdin shapes rather than line contents
# ==============================================================================

PAYLOADS = [
  "",                                        # empty stdin
  "15:30 UTC",                               # no trailing newline
  "\n",                                      # blank line
  "15:30 UTC\n16:45 PST\nno stamps\n",       # multi-line
  "2026-04-03T12:00:00Z\n2026-02-30T12:00:00Z", # multi-line, no trailing newline
  "15:30 UTC\r\n",                           # CRLF
  "  15:30 UTC  \n",                         # surrounding whitespace
  "15:30 UTC \xFF\xFE tail\n".b,             # invalid UTF-8 alongside a match
  "\xFF\xFE\n".b,                            # invalid UTF-8, nothing to match
  "caf\xE9 standup 9-10am PST\n".b,          # Latin-1 byte before a bare-hour range
  "\xFF \xC3\xA915:30 UTC\n".b,              # invalid byte, then a letter glued to a time
  "\xFF 15:30 UTC\xC3\xA9\n".b,              # a zone glued to a letter
  "Fri Sep ２５ 22:14:42 2026\n",            # fullwidth digits
  "٢٥ Sep 2026 22:14 GMT\n",                 # Arabic-Indic digits
  "１２:３０ UTC took 0:05\n",               # fullwidth time beside a duration
].freeze

PAYLOAD_ARGS = [
  [],
  ["-t", "pst"],
  ["-t", "pst", "-j"],
  ["-t", "pst", "-J"],
  ["--detect"],
  ["-v", "-t", "pst"],
].freeze

["UTC", "America/Los_Angeles"].each do |tz|
  PAYLOAD_ARGS.each do |args|
    PAYLOADS.each { |stdin| add(group: "payloads", args: args, stdin: stdin, tz: tz) }
  end
end

# ==============================================================================
# GROUP: env -- the TZ environment variable itself
# ==============================================================================

ENV_VALUES = [
  :unset,
  "",                     # POSIX: empty TZ means UTC, not "a zone named ''"
  ":America/New_York",    # POSIX leading colon
  "Not/AZone",            # invalid
  "Asia/Kolkata",         # sub-hour offset (+05:30)
  "Asia/Kathmandu",       # sub-hour, non-half-hour offset (+05:45)
  "PST8PDT",              # POSIX-style zone name
  "Etc/GMT+7",
].freeze

ENV_ARGS = [
  [],
  ["-t", "utc"],
  ["-t", "sf", "-j"],
  ["--detect"],
  ["-v"],
  ["-F", "short"],
].freeze

ENV_LINES = [
  "2026-04-03T12:00:00Z",
  "2026-04-03 12:00:00 UTC",
  "12:34",
  "15:30 UTC",
  "11:30:00 PM",
  "no timestamps here",
].freeze

ENV_VALUES.each do |tz|
  ENV_ARGS.each do |args|
    ENV_LINES.each { |line| add(group: "env", args: args, stdin: "#{line}\n", tz: tz) }
  end
end

# ==============================================================================
# GROUP: aliases -- every alias as a -t target and as a -f source
# ==============================================================================

Tztr::TIMEZONE_ALIASES.each_key do |name|
  add(group: "aliases", args: ["-t", name], stdin: "2026-04-03T12:00:00Z\n")
  add(group: "aliases", args: ["-f", name, "-t", "utc"], stdin: "12:34\n")
end

# ==============================================================================
# GROUP: errors -- stderr and exit codes must match byte-for-byte
# ==============================================================================

ERROR_ARGS = [
  # unknown / malformed option
  ["-z"],
  ["--bogus"],
  ["--tox", "utc"],
  ["--f", "utc"],                   # ambiguous abbreviation of --from/--format
  # missing flag argument
  ["-t"],
  ["--to"],
  ["-f"],
  ["-F"],
  ["-d"],
  # bad -F value
  ["-F", "bogus", "-t", "utc"],
  ["-F", "ISO", "-t", "utc"],       # wrong case
  ["--format=nope"],
  ["-F", ""],
  # bad -d value
  ["-d", "not-a-date", "-t", "utc"],
  ["-d", "2026-1-5", "-t", "utc"],  # single-digit month/day
  ["-d", "2026-13-01", "-t", "utc"],
  ["-d", "2026-02-30", "-t", "utc"],
  ["-d", "", "-t", "utc"],
  ["-d", "Jan 15 2026", "-t", "utc"],
  ["-d", "15 January 2026", "-t", "utc"],
  ["-d", "20260115", "-t", "utc"],
  ["-d", "2026/01/15", "-t", "utc"],
  # unknown timezone
  ["-t", "Mars/Phobos"],
  ["-t", "xyz"],
  ["-t", ""],
  ["-f", "nope", "-t", "utc"],
  ["-t", "America/Los Angeles"],
  ["-t", "../etc/passwd"],
  # numeric offsets: boundaries, out of range, sub-hour
  ["-t", "0"],
  ["-t", "+0"],
  ["-t", "-0"],
  ["-t", "14"],
  ["-t", "-12"],
  ["-t", "15"],
  ["-t", "-13"],
  ["-t", "+99"],
  ["-t", "5.5"],
  ["-t", "+5:30"],
  ["-t", "-3.5"],
].freeze

ERROR_ARGS.each { |args| add(group: "errors", args: args, stdin: "15:30 UTC\n") }

# ==============================================================================
# GROUP: modes -- whole modes the matrix used to skip entirely
# ==============================================================================

[
  ["-l"], ["--list"], ["-l", "-j"], ["-l", "-t", "nope"],
  ["-h"], ["--help"], ["--help", "--json"], ["-h", "--ndjson"],
  ["-V"], ["--version"], ["-V", "-j"],
  ["--"], ["-t", "utc", "--"],
].each { |args| add(group: "modes", args: args, stdin: "15:30 UTC\n") }

# ==============================================================================
# GROUP: files -- file arguments, and -i compared by resulting file bytes
# ==============================================================================

LOG_A = "2026-04-03T12:00:00Z start\n15:30 UTC tick\nno stamps\n"
LOG_B = "2026-04-03 12:00:00 PST build\n11:30:00 PM deploy\n"

[
  [["a.log"],                      { "a.log" => LOG_A }],
  [["-t", "sf", "a.log"],          { "a.log" => LOG_A }],
  [["-t", "sf", "a.log", "b.log"], { "a.log" => LOG_A, "b.log" => LOG_B }],
  [["-t", "sf", "-j", "a.log", "b.log"], { "a.log" => LOG_A, "b.log" => LOG_B }],
  [["-t", "sf", "empty.log"],      { "empty.log" => "" }],
  [["-t", "sf", "missing.log"],    {}],
  [["-t", "sf", "a.log", "missing.log"], { "a.log" => LOG_A }],
  # a bad operand in the middle: the rest still run, exit 1
  [["-t", "sf", "a.log", "missing.log", "b.log"], { "a.log" => LOG_A, "b.log" => LOG_B }],
  [["-t", "sf", "-j", "a.log", "missing.log", "b.log"], { "a.log" => LOG_A, "b.log" => LOG_B }],
  [["-t", "sf", "--", "a.log"],    { "a.log" => LOG_A }],
  [["-v", "-t", "sf", "a.log"],    { "a.log" => LOG_A }],
].each { |args, files| add(group: "files", args: args, files: files) }

# a directory where a file is expected
add(group: "files", args: ["-t", "sf", "adir"], dirs: ["adir"])
add(group: "files", args: ["-i", "-t", "sf", "adir"], dirs: ["adir"])

# -i: the interesting output is the file afterwards, which the runner diffs.
[
  ["-i", "-t", "sf", "a.log"],
  ["-i", "-t", "sf", "a.log", "b.log"],
  ["-i", "-t", "sf", "-F", "iso", "a.log"],
  ["--in-place", "--to", "sf", "a.log"],
  ["-i", "-t", "sf", "empty.log"],
  ["-i", "-t", "sf", "missing.log"],
  ["-i", "-t", "sf", "a.log", "missing.log", "b.log"],
  ["-i", "-t", "sf"],               # -i with no file argument
  ["-i", "-j", "a.log"],            # mutually exclusive
  ["-i", "--ndjson", "a.log"],
  ["-i", "--detect", "a.log"],
  ["-i", "-v", "-t", "sf", "a.log"],
].each do |args|
  add(group: "files", args: args,
      files: { "a.log" => LOG_A, "b.log" => LOG_B, "empty.log" => "" })
end

# ==============================================================================
# GROUP: argv -- argument-vector shapes: repeats, ordering, case, terminators
# ==============================================================================

[
  # repeated flags -- last one wins, or does it
  ["-t", "utc", "-t", "sf"],
  ["-f", "utc", "-f", "pst", "-t", "utc"],
  ["-F", "short", "-F", "time", "-t", "utc"],
  ["-d", "2026-01-15", "-d", "2026-07-15", "-t", "utc"],
  ["-v", "-v", "-t", "utc"],
  # both structured modes at once
  ["-j", "-J", "-t", "pst"],
  ["-J", "-j", "-t", "pst"],
  ["-t", "utc", "--detect", "-j", "-F", "iso"],
  # alias casing and the space -> underscore fold
  ["-t", "SF"],
  ["-t", "Pst"],
  ["-t", "hong kong"],
  ["-t", "Hong Kong"],
  # unique-prefix abbreviations: OptionParser accepts them for both long
  # options and -F's value list, so a script can depend on one binary's answer
  ["--fr", "utc"],
  ["--fro", "utc"],
  ["--jso"],
  ["--nd"],
  ["--det"],
  ["--verb", "-t", "utc"],
  ["-F", "i", "-t", "utc"],
  ["-F", "s", "-t", "utc"],
  ["-F", "t", "-t", "utc"],
  ["-F", "is", "-t", "utc"],
  ["--format=sh", "--to=utc"],
  # An inline value on a flag that takes none. Splitting --name=value and
  # keeping only the name discards the value in silence, the same class of bug
  # as reading a log level as a zone -- so every no-argument flag is swept.
  ["--json=foo"],
  ["--ndjson=x"],
  ["--detect=x"],
  ["--verbose=1"],
  ["--list=x"],
  ["--in-place=x"],
  ["--version=x"],
  ["--help=x"],
  # ...and the value-taking flags alongside them, so the sweep above cannot
  # pass by rejecting every --name=value form.
  ["--format=iso", "--to=utc"],
  ["--to=utc"],
  ["--from=utc", "--to=pst"],
  ["--date=2026-01-15", "--to=utc"],
  # the short-flag neighbour: a tail after a flag that takes no argument
  ["-jfoo"],
  ["-vtsf"],
  # terminators and dash-shaped operands
  ["--", "-t"],            # a file literally named "-t"
  ["-t", "utc", "--", "--detect"],
  ["-"],                   # a file literally named "-"
].each { |args| add(group: "argv", args: args, stdin: "15:30 UTC\n") }

# --in-place=x against a real file: rejecting it must also leave the file alone.
add(group: "argv", args: ["--in-place=x", "-t", "sf", "a.log"], files: { "a.log" => LOG_A })

# Options after a file operand: OptionParser permutes argv by default.
[
  ["a.log", "-t", "sf"],
  ["-t", "sf", "a.log", "-F", "iso"],
  ["a.log", "adir"],
  ["-t", "sf", "adir", "a.log"],
  # a bad operand after a good one: does the good one's output/rewrite survive?
  ["-i", "-t", "sf", "a.log", "missing.log"],
  ["-i", "-t", "sf", "missing.log", "a.log"],
].each do |args|
  add(group: "argv", args: args, files: { "a.log" => LOG_A }, dirs: ["adir"])
end

# -i compared by resulting bytes, on files that stress the rewrite itself.
{
  "nonl.log"    => "15:30 UTC tick",                   # no trailing newline
  "nostamp.log" => "nothing to translate here\n",      # unchanged -> must not be rewritten
  "binary.log"  => "15:30 UTC \xFF\xFE tail\n".b,      # invalid UTF-8 must survive the round trip
  "crlf.log"    => "15:30 UTC tick\r\n",
}.each do |name, body|
  add(group: "argv", args: ["-i", "-t", "sf", name], files: { name => body })
  add(group: "argv", args: ["-t", "sf", name], files: { name => body })
end

# ==============================================================================
# GROUP: golden -- absolute expectations, checked against BOTH binaries.
# Reserved for behaviors where agreement isn't enough: both could be wrong the
# same way. Keep this list short; per-implementation detail belongs in specs.
# ==============================================================================

# An expectation returns nil when satisfied, or a sentence saying what it wanted.

add(group: "golden", args: ["-v", "-t", "utc"], stdin: "12:34\n", tz: "America/New_York",
    expect: lambda { |out, err, code|
      # implicit -f is announced by the disclosure, not by a second startup line
      next if code.zero? && !out.empty? && err.lines.size == 2

      "expected exit 0 and exactly 2 stderr lines, got exit #{code} / #{err.lines.size} lines"
    })

add(group: "golden", args: ["-d", "2026-1-5", "-t", "utc"], stdin: "15:30 UTC\n",
    expect: lambda { |_out, err, code|
      next if code == 1 && err == "tztr: invalid date: 2026-1-5\n"

      "expected exit 1 and 'tztr: invalid date: 2026-1-5', got exit #{code} / #{err.inspect}"
    })

add(group: "golden", args: ["-t", "pst", "-j"], stdin: "15:30 UTC \xFF\xFE tail\n".b,
    expect: lambda { |out, _err, code|
      # matched substrings are ASCII by construction, so stray bytes elsewhere
      # in the line must not suppress the match
      parsed = begin
        JSON.parse(out)
      rescue JSON::ParserError
        nil
      end
      next if code.zero? && parsed&.size == 1

      "expected exit 0 and 1 JSON match, got exit #{code} / #{out.inspect}"
    })

# -l and -V act inside the option block itself. Validation has to come first,
# or the list/version is already on stdout by the time the argument is refused
# -- and both binaries doing that would still look like parity.
[["--list=x", "--list"], ["--version=x", "--version"]].each do |args, flag|
  add(group: "golden", args: [args], stdin: "15:30 UTC\n",
      expect: lambda { |out, err, code|
        next if code == 1 && out.empty? && err == "tztr: #{flag} takes no argument\n"

        "expected exit 1, empty stdout and 'tztr: #{flag} takes no argument', " \
          "got exit #{code} / out #{out.inspect} / err #{err.inspect}"
      })
end

add(group: "golden", args: ["-t", "sf", "missing.log"], files: {},
    expect: lambda { |_out, err, code|
      next if code == 1 && err == "tztr: missing.log: No such file or directory (os error 2)\n"

      "expected exit 1 and a message naming missing.log, got exit #{code} / #{err.inspect}"
    })

# Behaviors both builds could get wrong together, pinned to an absolute answer.
# Dateless input needs -d here so the answer doesn't depend on today.
[
  ["Deadline: Friday, Apr 3 - 5pm PST", "Deadline: Friday, Apr 3 - 01:00 UTC"],
  ["Due 4/3 - 5pm PST",                 "Due 4/3 - 01:00 UTC"],
  ["Ticket #12 - 9:30am PST",           "Ticket #12 - 17:30 UTC"],
  ["Room 7 - 3pm PST",                  "Room 7 - 23:00 UTC"],
  ["Standup 9-9:15am PST",              "Standup 17:00 UTC-17:15 UTC"],
  ["Date: 15 Apr 2026 22:14:42 -0700",  "Date: 16 Apr 2026 05:14:42 +0000"],
  ["01 Aug 2026 10:00 GMT",             "01 Aug 2026 10:00 UTC"],
  ["Fri Sep 25 22:14:42 PDT 2026",      "Sat Sep 26 05:14:42 UTC 2026"],
  ["11:30 PM to 12:30 AM PST",          "07:30 UTC to 08:30 UTC"],
].each do |line, expected|
  add(group: "golden", args: ["-t", "utc", "-d", "2026-04-03"], stdin: "#{line}\n",
      expect: lambda { |out, _err, code|
        next if code.zero? && out == "#{expected}\n"

        "#{line.inspect}: expected #{expected.inspect}, got exit #{code} / #{out.inspect}"
      })
end

# With no -d, a range past midnight is an hour long whichever zone's "today"
# it was read in -- the answer depends on the clock, so assert the length.
[["-t", "14"], ["-t", "-12"], ["-t", "tokyo"]].each do |args|
  add(group: "golden", args: [*args, "-F", "iso"], stdin: "11:30 PM to 12:30 AM PST\n",
      expect: lambda { |out, _err, code|
        start, finish = out.chomp.split(" to ").map { |t| Time.parse(t) rescue nil }
        next if code.zero? && start && finish && finish - start == 3600

        "#{args.inspect}: expected a one-hour range, got #{out.inspect}"
      })
end

# ==============================================================================
# Runner
# ==============================================================================

def prepare(dir, kase)
  FileUtils.mkdir_p(dir)
  kase[:files]&.each { |name, body| File.binwrite(File.join(dir, name), body) }
  kase[:dirs]&.each { |name| FileUtils.mkdir_p(File.join(dir, name)) }
end

# Every file the run left behind, so `-i` is compared by result rather than by
# its (empty) stdout.
def snapshot(dir)
  Dir.glob("**/*", base: dir).sort.map do |rel|
    path = File.join(dir, rel)
    [rel, File.file?(path) ? File.binread(path) : :dir]
  end
end

def run(cmd, kase, dir)
  out, err, status = Open3.capture3(
    { "TZ" => (kase[:tz] == :unset ? nil : kase[:tz]) },
    *cmd, *kase[:args],
    stdin_data: kase[:stdin], chdir: dir
  )
  [out.b, err.b, status.exitstatus, dir == ROOT ? nil : snapshot(dir)]
end

def check(kase, index, scratch)
  sandboxed = kase[:files] || kase[:dirs]
  rb_dir = sandboxed ? File.join(scratch, "#{index}-rb") : ROOT
  rs_dir = sandboxed ? File.join(scratch, "#{index}-rs") : ROOT

  if sandboxed
    prepare(rb_dir, kase)
    prepare(rs_dir, kase)
  end

  rb = run(RUBY_CMD, kase, rb_dir)
  rs = run(RUST_CMD, kase, rs_dir)

  problems = []
  problems << "stdout" unless rb[0] == rs[0]
  problems << "stderr" unless rb[1] == rs[1]
  problems << "exit"   unless rb[2] == rs[2]
  problems << "files"  unless rb[3] == rs[3]

  if (expect = kase[:expect])
    { "ruby" => rb, "rust" => rs }.each do |name, (out, err, code, _)|
      result = expect.call(out, err, code)
      problems << "#{name} expectation: #{result}" if result.is_a?(String)
    end
  end

  problems.empty? ? nil : { kase: kase, problems: problems, rb: rb, rs: rs }
end

def describe(kase)
  parts = ["TZ=#{kase[:tz] == :unset ? '<unset>' : kase[:tz].inspect}",
           "args=#{kase[:args].inspect}"]
  parts << "stdin=#{kase[:stdin].inspect}" unless kase[:stdin].empty?
  parts << "files=#{kase[:files].keys.inspect}" if kase[:files]&.any?
  parts << "dirs=#{kase[:dirs].inspect}" if kase[:dirs]
  parts.join("  ")
end

def report(failure)
  kase = failure[:kase]
  puts "MISMATCH [#{kase[:group]}] #{failure[:problems].join(', ')}"
  puts "  #{describe(kase)}"
  %w[ruby rust].zip([failure[:rb], failure[:rs]]).each do |name, (out, err, code, files)|
    puts "  #{name} exit=#{code} out=#{out.inspect} err=#{err.inspect}"
    puts "  #{name} files=#{files.inspect}" if files
  end
  puts
end

groups = ENV["PARITY_GROUPS"]&.split(",")&.map(&:strip)
cases = groups ? CASES.select { |c| groups.include?(c[:group]) } : CASES
abort "no cases match PARITY_GROUPS=#{ENV['PARITY_GROUPS']}" if cases.empty?

# Ruby process startup dominates the runtime, and it is all waiting on IO, so a
# small pool cuts wall time several-fold. Results stay indexed, so the report
# order is identical to a serial run.
jobs = Integer(ENV["PARITY_JOBS"] || Etc.nprocessors)
failures = Array.new(cases.size)
started = Time.now

Dir.mktmpdir("tztr-parity") do |scratch|
  queue = (0...cases.size).to_a
  lock = Mutex.new
  [jobs, cases.size].min.times.map do
    Thread.new do
      loop do
        i = lock.synchronize { queue.shift }
        break if i.nil?

        failures[i] = check(cases[i], i, scratch)
      end
    end
  end.each(&:join)
end

failures.compact!
failures.each { |f| report(f) }

by_group = cases.group_by { |c| c[:group] }.transform_values(&:size)
failed_by_group = failures.group_by { |f| f[:kase][:group] }.transform_values(&:size)

puts "cases by group:"
by_group.each { |group, n| puts format("  %-10s %5d  (%d failing)", group, n, failed_by_group.fetch(group, 0)) }
puts
puts "#{cases.size - failures.size}/#{cases.size} cases match  [#{jobs} jobs, #{format('%.1fs', Time.now - started)}]"
exit(failures.empty? ? 0 : 1)

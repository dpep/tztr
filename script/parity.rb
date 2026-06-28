#!/usr/bin/env ruby
# frozen_string_literal: true

# Ruby <-> Rust CLI parity harness. Runs a matrix of (args, input, TZ) through
# both binaries and diffs stdout. JSON modes are compared semantically (parsed),
# everything else byte-for-byte. Exits non-zero on any mismatch.
#
#   ruby script/parity.rb            # uses rust/target/release/tztr
#   RUST_BIN=path ruby script/parity.rb
#
# Build the Rust binary first: cargo build --release --manifest-path rust/Cargo.toml

require "open3"
require "json"

ROOT     = File.expand_path("..", __dir__)
RUBY_BIN = File.join(ROOT, "bin", "tztr")
RUST_BIN = ENV["RUST_BIN"] || File.join(ROOT, "rust", "target", "release", "tztr")

abort "rust binary not found: #{RUST_BIN} (build it first)" unless File.executable?(RUST_BIN)

# A fixed reference TZ keeps date-less inputs deterministic across both binaries
# (they share the same wall clock, so "today" agrees).
ENVS = ["UTC", "America/Los_Angeles", "America/New_York"].freeze

INPUTS = [
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
  "12:00 CET",      # unrecognized abbrev -> ignored, parsed in target
  "2026-12-25 23:59:59 UTC",
].freeze

ARG_SETS = [
  [],
  ["-t", "America/Los_Angeles"],
  ["-t", "sf"],
  ["-t", "nyc"],
  ["-t", "utc"],
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
  ["-t", "utc", "-d", "2026-01-15"],
  ["-t", "utc", "-d", "2026-07-15"],
  ["-t", "utc", "-d", "January 15, 2026"],
].freeze

def run(bin, args, input, tz)
  out, _err, status = Open3.capture3({ "TZ" => tz }, bin, *args, stdin_data: input)
  [out, status.exitstatus]
end

def json_mode?(args)
  args.include?("-j") || args.include?("--json") || args.include?("-J") || args.include?("--ndjson")
end

def normalize(out, args)
  return out unless json_mode?(args)

  if args.include?("-J") || args.include?("--ndjson")
    out.each_line.map { |l| l.strip.empty? ? nil : JSON.parse(l) }.compact
  else
    JSON.parse(out)
  end
rescue JSON::ParserError
  out # fall back to raw compare if it isn't valid JSON
end

fails = 0
total = 0

ENVS.each do |tz|
  ARG_SETS.each do |args|
    INPUTS.each do |input|
      total += 1
      stdin = input + "\n"
      rb_out, rb_code = run(RUBY_BIN, args, stdin, tz)
      rs_out, rs_code = run(RUST_BIN, args, stdin, tz)

      ok = rb_code == rs_code && normalize(rb_out, args) == normalize(rs_out, args)
      next if ok

      fails += 1
      puts "MISMATCH  TZ=#{tz}  args=#{args.inspect}  input=#{input.inspect}"
      puts "  ruby (exit #{rb_code}): #{rb_out.inspect}"
      puts "  rust (exit #{rs_code}): #{rs_out.inspect}"
    end
  end
end

puts
puts "#{total - fails}/#{total} cases match"
exit(fails.zero? ? 0 : 1)

# frozen_string_literal: true

require 'time'
require_relative 'tztr/version'

module Tztr
  Error = Class.new(StandardError)

  # The tzdb Ruby's Time reads through $TZ, and the Rust port reads through
  # jiff -- the authority on whether a zone name means anything.
  ZONEINFO_DIRS = [ENV['TZDIR'], '/usr/share/zoneinfo', '/etc/zoneinfo'].compact.freeze

  # Abbreviations Ruby's Time.parse resolves on its own.
  NATIVE_ABBREVIATIONS = %w[UT UTC GMT Z EST EDT CST CDT MST MDT PST PDT].freeze

  # Abbreviations Time.parse silently ignores -- it would read them as local
  # time, so we resolve them through TIMEZONE_ALIASES ourselves.
  ALIASED_ABBREVIATIONS = %w[
    ET CT MT PT HST AKST AKDT CET CEST BST IST JST KST HKT AEST AEDT NZST NZDT
  ].freeze

  # Only these count as a zone inside text. A bare [A-Z]{2,4} swallows the next
  # word instead -- INFO, WARN, ERROR, PM.
  ZONE_ABBREVIATIONS = (NATIVE_ABBREVIATIONS + ALIASED_ABBREVIATIONS).freeze

  # Longest first, so UTC is not read as UT.
  ABBREVIATION = Regexp.union(ZONE_ABBREVIATIONS.sort_by { |abbr| [-abbr.length, abbr] })
  ZONE = /(?:#{ABBREVIATION})\b|[+-]\d{4}\b/
  MERIDIEM = /[AaPp]\.?[Mm]\.?/

  PATTERNS = [
    # ISO 8601 with Z or offset: 2026-04-03T12:34:56Z, 2026-04-03T12:34:56.123+00:00
    /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})/,
    # ISO 8601 without timezone: 2026-04-03T12:34:56
    /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?/,
    # Date space 12-hour time: 2026-04-03 03:45:00 PM, 2026-04-03 03:45 PM PST
    /\d{4}-\d{2}-\d{2} \d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{MERIDIEM}\b(?: ?#{ZONE})?/,
    # Date space time with tz: 2026-04-03 12:34:56 UTC
    /\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)? ?#{ZONE}/,
    # Date space time: 2026-04-03 12:34:56
    /\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?/,
    # Time with tz: 12:34:56 UTC, 12:34 PST
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{ZONE}/,
    # Time with offset: 12:34:56+00:00
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?[+-]\d{2}:?\d{2}\b/,
    # 12-hour time: 11:30 PM, 3:45 p.m., 3:45 PM PST
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{MERIDIEM}\b(?: ?#{ZONE})?/,
    # Bare time: 12:34:56, 12:34
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?\b/,
  ].freeze

  TIMEZONE_ALIASES = {
    # UTC
    'utc' => 'UTC', 'gmt' => 'UTC', 'z' => 'UTC',
    # US Eastern
    'est' => 'America/New_York', 'edt' => 'America/New_York', 'et' => 'America/New_York',
    'eastern' => 'America/New_York',
    # US Central
    'cst' => 'America/Chicago', 'cdt' => 'America/Chicago', 'ct' => 'America/Chicago',
    'central' => 'America/Chicago',
    # US Mountain
    'mst' => 'America/Denver', 'mdt' => 'America/Denver', 'mt' => 'America/Denver',
    'mountain' => 'America/Denver',
    # US Pacific
    'pst' => 'America/Los_Angeles', 'pdt' => 'America/Los_Angeles', 'pt' => 'America/Los_Angeles',
    'pacific' => 'America/Los_Angeles',
    # US Other
    'hst' => 'Pacific/Honolulu', 'akst' => 'America/Anchorage', 'akdt' => 'America/Anchorage',
    # Europe
    'cet' => 'Europe/Berlin', 'cest' => 'Europe/Berlin',
    'bst' => 'Europe/London',
    'ist' => 'Asia/Kolkata',
    # Asia/Pacific
    'jst' => 'Asia/Tokyo', 'kst' => 'Asia/Seoul',
    'cst_china' => 'Asia/Shanghai', 'hkt' => 'Asia/Hong_Kong',
    'aest' => 'Australia/Sydney', 'aedt' => 'Australia/Sydney',
    'nzst' => 'Pacific/Auckland', 'nzdt' => 'Pacific/Auckland',
    # Cities
    'sf' => 'America/Los_Angeles', 'la' => 'America/Los_Angeles', 'seattle' => 'America/Los_Angeles',
    'denver' => 'America/Denver',
    'chicago' => 'America/Chicago',
    'nyc' => 'America/New_York', 'boston' => 'America/New_York', 'miami' => 'America/New_York',
    'london' => 'Europe/London',
    'paris' => 'Europe/Paris', 'berlin' => 'Europe/Berlin', 'amsterdam' => 'Europe/Amsterdam',
    'tokyo' => 'Asia/Tokyo',
    'sydney' => 'Australia/Sydney',
    'mumbai' => 'Asia/Kolkata', 'delhi' => 'Asia/Kolkata',
    'shanghai' => 'Asia/Shanghai', 'beijing' => 'Asia/Shanghai',
    'hong_kong' => 'Asia/Hong_Kong', 'hongkong' => 'Asia/Hong_Kong',
    'singapore' => 'Asia/Singapore',
    'seoul' => 'Asia/Seoul',
    'honolulu' => 'Pacific/Honolulu', 'hawaii' => 'Pacific/Honolulu',
    'anchorage' => 'America/Anchorage', 'alaska' => 'America/Anchorage',
    'toronto' => 'America/Toronto',
    'vancouver' => 'America/Vancouver',
    'auckland' => 'Pacific/Auckland',
    'dubai' => 'Asia/Dubai',
    'sao_paulo' => 'America/Sao_Paulo',
  }.freeze

  module_function

  def resolve_tz(input)
    return if input.nil?

    input = input.delete_prefix(':') # POSIX spells it TZ=:America/New_York

    # Numeric offset: -7 -> Etc/GMT+7 (POSIX sign is inverted)
    if input.match?(/\A[+-]?\d{1,2}\z/)
      n = input.to_i
      return 'UTC' if n.zero?
      raise Error, "offset out of range: #{input} (expected -12..14)" unless (-12..14).cover?(n)

      return "Etc/GMT#{n.positive? ? '-' : '+'}#{n.abs}"
    end

    alias_zone = TIMEZONE_ALIASES[input.downcase.tr(' ', '_')]
    return alias_zone if alias_zone
    raise Error, "unknown timezone: #{input}" unless known_zone?(input)

    input
  end

  def known_zone?(name)
    return false unless name.match?(%r{\A[A-Za-z0-9_+-]+(?:/[A-Za-z0-9_+-]+)*\z})

    ZONEINFO_DIRS.any? { |dir| File.file?(File.join(dir, name)) }
  end

  def translate(line, to: 'UTC', from: nil, format: nil, local: false, date: nil)
    to = resolve_tz(to)
    from = resolve_tz(from)
    ENV['TZ'] = to
    result = line.dup

    PATTERNS.each do |pattern|
      next unless result.match?(pattern)

      result.gsub!(pattern) do |match|
        convert_match(match, from:, to:, format:, local:, date:) || match
      end

      break result
    end

    result
  end

  # Per-match structured analysis of a line. Returns an array of hashes, one
  # per detected timestamp: { original:, detected_format:, detected_tz:,
  # translated: }. With detect: true, translation is skipped and :translated is
  # omitted.
  def matches(line, to: 'UTC', from: nil, format: nil, local: false, detect: false, date: nil)
    to = resolve_tz(to)
    from = resolve_tz(from)
    ENV['TZ'] = to
    results = []

    PATTERNS.each do |pattern|
      next unless line.match?(pattern)

      line.scan(pattern) do |match|
        info = {
          original: match,
          detected_format: detect_format(match),
          detected_tz: detect_zone(match),
        }
        info[:translated] = convert_match(match, from:, to:, format:, local:, date:) unless detect
        results << info
      end

      break
    end

    results
  end

  def convert_match(match, from:, to:, format:, local:, date: nil)
    time = parse(match, from:, to:, date:)
    format_time(time.localtime, format, match, local:)
  rescue ArgumentError
    nil
  end

  def detect_format(str)
    case str
    when /\A\d{4}-\d{2}-\d{2}T/ then 'iso'
    when /\A\d{4}-\d{2}-\d{2} / then 'datetime'
    else 'time'
    end
  end

  def detect_zone(str)
    m = str.match(/ ?(#{ABBREVIATION}|[+-]\d{2}:?\d{2})\z/)
    m && m[1]
  end

  def parse(str, from: nil, to: 'UTC', date: nil)
    # Time-only inputs carry no date, so DST can't be resolved correctly. A
    # reference date supplies the missing context (see README caveat).
    str = "#{date} #{str}" if date && time_only?(str)
    # Time.parse reads the "p" of "3:45 p.m." as the military zone P (-03:00).
    str = str.sub(/([AaPp])\.([Mm])\.?/, '\1\2')

    abbr = detect_zone(str)
    zone = aliased_zone(abbr)

    if zone
      in_zone(str.sub(/ ?#{abbr}\z/, ''), zone, to)
    elsif abbr
      Time.parse(str)
    elsif from
      in_zone(str, from, to)
    else
      ENV['TZ'] = to
      earliest_occurrence(Time.parse(str))
    end
  end

  # The IANA zone an abbreviation names, for the ones Time.parse can't resolve.
  def aliased_zone(abbr)
    return if abbr.nil? || NATIVE_ABBREVIATIONS.include?(abbr)

    TIMEZONE_ALIASES[abbr.downcase]
  end

  def in_zone(str, zone, to)
    ENV['TZ'] = zone
    utc = earliest_occurrence(Time.parse(str)).utc
    ENV['TZ'] = to
    utc.localtime
  end

  # A wall clock repeated by a DST fall-back resolves to the earlier
  # (daylight) occurrence, as Temporal, ICU, RFC 5545 and date(1) do.
  # Time.parse picks the later one.
  def earliest_occurrence(time)
    earlier = time - 3600
    earlier.strftime('%F %T') == time.strftime('%F %T') ? earlier : time
  end

  def time_only?(str)
    str.match?(/\A\d{1,2}:/)
  end

  def format_time(time, fmt, original, local: false)
    tz = time.utc_offset == 0 ? 'Z' : time.strftime('%:z')

    case fmt
    when :time then return time.strftime('%H:%M:%S')
    when :iso then return time.strftime('%Y-%m-%d %H:%M:%S') + tz
    when :short
      base = time.strftime('%Y-%m-%d %H:%M')
      return base if local

      return base + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    end

    # Preserve input format

    case original
    when /^\d{4}-\d{2}-\d{2}T/
      has_frac = original.match?(/T\d{2}:\d{2}:\d{2}\.\d+/)
      base = has_frac ? time.strftime('%Y-%m-%dT%H:%M:%S.%L') : time.strftime('%Y-%m-%dT%H:%M:%S')
      base + tz
    when /^\d{4}-\d{2}-\d{2} /
      has_frac = original.match?(/ \d{2}:\d{2}:\d{2}\.\d+/)
      base = has_frac ? time.strftime('%Y-%m-%d %H:%M:%S.%L') : time.strftime('%Y-%m-%d %H:%M:%S')
      base + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    when /^\d{1,2}:\d{2}(?::\d{2})/
      time.strftime('%H:%M:%S') + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    when /^\d{1,2}:\d{2}/
      time.strftime('%H:%M') + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    else
      time.strftime('%Y-%m-%d %H:%M:%S') + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    end
  end
end

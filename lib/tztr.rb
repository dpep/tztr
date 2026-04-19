# frozen_string_literal: true

require 'time'
require_relative 'tztr/version'

module Tztr
  PATTERNS = [
    # ISO 8601 with Z or offset: 2026-04-03T12:34:56Z, 2026-04-03T12:34:56.123+00:00
    /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})/,
    # ISO 8601 without timezone: 2026-04-03T12:34:56
    /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?/,
    # Date space time with tz: 2026-04-03 12:34:56 UTC
    /\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)? ?(?:UTC|GMT|[A-Z]{2,4}|[+-]\d{4})/,
    # Date space time: 2026-04-03 12:34:56
    /\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?/,
    # Time with tz: 12:34:56 UTC, 12:34 PST
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?(?:UTC|GMT|[A-Z]{2,4}|[+-]\d{4})\b/,
    # Time with offset: 12:34:56+00:00
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?[+-]\d{2}:?\d{2}\b/,
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
    'gmt' => 'Europe/London', 'bst' => 'Europe/London',
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
    return input if input.nil?

    # Numeric offset: -7 -> Etc/GMT+7 (POSIX sign is inverted)
    if input.match?(/\A[+-]?\d{1,2}\z/)
      n = input.to_i
      return 'UTC' if n == 0

      return "Etc/GMT#{n > 0 ? '-' : '+'}#{n.abs}"
    end

    TIMEZONE_ALIASES[input.downcase.tr(' ', '_')] || input
  end

  def translate(line, to: 'UTC', from: nil, format: nil)
    to = resolve_tz(to)
    from = resolve_tz(from)
    ENV['TZ'] = to
    result = line.dup

    PATTERNS.each do |pattern|
      next unless result.match?(pattern)

      result.gsub!(pattern) do |match|
        begin
          time = parse(match, from:, to:)
          format_time(time.localtime, format, match)
        rescue ArgumentError
          match
        end
      end

      break result
    end

    result
  end

  def parse(str, from: nil, to: 'UTC')
    if has_timezone?(str)
      Time.parse(str)
    elsif from
      ENV['TZ'] = from
      t = Time.parse(str).utc
      ENV['TZ'] = to
      t.localtime
    else
      ENV['TZ'] = to
      Time.parse(str)
    end
  end

  def has_timezone?(str)
    str.match?(/Z$|[+-]\d{2}:?\d{2}$| ?(?:UTC|GMT|[A-Z]{2,4}|[+-]\d{4})$/)
  end

  def format_time(time, fmt, original)
    case fmt
    when :short then return time.strftime('%Y-%m-%d %H:%M')
    when :time then return time.strftime('%H:%M:%S')
    when :iso then return time.strftime('%Y-%m-%d %H:%M:%S')
    end

    # Preserve input format
    tz = time.utc_offset == 0 ? 'Z' : time.strftime('%:z')

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

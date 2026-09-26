# frozen_string_literal: true

require 'date'
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

  # Lowercase spellings that are also words likely to follow a time -- French
  # "est"/"cet"/"et", German "ist" -- so "à 15:30 est annulée" is left alone.
  WORD_ABBREVIATIONS = %w[EST CET ET IST UT Z].freeze

  # Uppercase, or wholly lowercase unless that is also a word. Not mixed case.
  ZONE_SPELLINGS = (ZONE_ABBREVIATIONS + (ZONE_ABBREVIATIONS - WORD_ABBREVIATIONS).map(&:downcase)).freeze

  # Longest first, so UTC is not read as UT.
  ABBREVIATION = Regexp.union(ZONE_SPELLINGS.sort_by { |abbr| [-abbr.length, abbr] })
  ZONE = /(?:#{ABBREVIATION})\b|[+-]\d{4}\b/
  # A dotted meridiem takes its closing dot; an undotted one leaves a
  # following full stop to the sentence.
  MERIDIEM = /[AaPp](?:\.[Mm]\.|\.?[Mm]\b)/

  PATTERNS = [
    # ISO 8601 with Z or offset: 2026-04-03T12:34:56Z, 2026-04-03T12:34:56.123+00:00
    /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})/,
    # ISO 8601 without timezone: 2026-04-03T12:34:56
    /\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?/,
    # Date space 12-hour time: 2026-04-03 03:45:00 PM, 2026-04-03 03:45 PM PST
    /\d{4}-\d{2}-\d{2} \d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{MERIDIEM}(?: ?#{ZONE})?/,
    # Date space time with tz: 2026-04-03 12:34:56 UTC
    /\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)? ?#{ZONE}/,
    # Date space time: 2026-04-03 12:34:56
    /\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}(?:\.\d+)?/,
    # Time with tz: 12:34:56 UTC, 12:34 PST
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{ZONE}/,
    # Time with offset: 12:34:56+00:00
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?[+-]\d{2}:?\d{2}\b/,
    # 12-hour time: 11:30 PM, 3:45 p.m., 3:45 PM PST
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{MERIDIEM}(?: ?#{ZONE})?/,
    # Bare time: 12:34:56, 12:34
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?\b/,
  ].freeze

  # One pass over the line: at each position the alternatives are tried in the
  # order above, so a longer format wins over a shorter one inside it, and every
  # timestamp on the line converts whatever its format.
  TIMESTAMP = Regexp.union(PATTERNS)
  BARE_TIME = /\A(?:#{PATTERNS.last})\z/

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

  MONTHS = %w[
    january february march april may june
    july august september october november december
  ].freeze

  # The -d forms the README documents. Date.parse accepts far more than the
  # Rust port's hand-rolled parser does, so both narrow to this set.
  def normalize_date(input)
    parts =
      case input
      when /\A(\d{4})-(\d{2})-(\d{2})\z/, %r{\A(\d{4})/(\d{2})/(\d{2})\z}, /\A(\d{4})(\d{2})(\d{2})\z/
        [$1.to_i, $2.to_i, $3.to_i]
      when /\A([A-Za-z]+)\.? (\d{1,2}),? (\d{4})\z/ # January 15, 2026 / Jan 15 2026
        [$3.to_i, month_number($1), $2.to_i]
      when /\A(\d{1,2}) ([A-Za-z]+)\.?,? (\d{4})\z/ # 15 January 2026
        [$3.to_i, month_number($2), $1.to_i]
      end

    raise Error, "invalid date: #{input}" unless parts&.all? && Date.valid_date?(*parts)

    format('%04d-%02d-%02d', *parts)
  end

  def month_number(name)
    name = name.downcase
    index = MONTHS.index { |month| month == name || (name.length == 3 && month.start_with?(name)) }
    index && index + 1
  end

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

  def translate(line, to: 'UTC', from: nil, format: nil, date: nil)
    to = resolve_tz(to)
    from = resolve_tz(from)
    ENV['TZ'] = to
    line = scannable(line)
    skip_bare = anchored?(line.scan(TIMESTAMP))

    line.gsub(TIMESTAMP) do |match|
      next match if skip_bare && match.match?(BARE_TIME)

      convert_match(match, from:, to:, format:, date:) || match
    end
  end

  # Per-match structured analysis of a line. Returns an array of hashes, one
  # per detected timestamp: { original:, detected_format:, detected_tz:,
  # translated: }. With detect: true, translation is skipped and :translated is
  # omitted.
  def matches(line, to: 'UTC', from: nil, format: nil, detect: false, date: nil)
    to = resolve_tz(to)
    from = resolve_tz(from)
    ENV['TZ'] = to
    timestamps(scannable(line)).map do |match|
      info = {
        # Patterns only ever match ASCII, whatever the rest of the line is.
        original: match.force_encoding(Encoding::UTF_8),
        detected_format: detect_format(match),
        detected_tz: detect_zone(match),
      }
      info[:translated] = convert_match(match, from:, to:, format:, date:) unless detect
      info
    end
  end

  # Which assumptions a line's timestamps force on us, for -v to disclose.
  # A timestamp with no date needs one to resolve DST in the *target* zone,
  # whether or not it names its own; without a zone as well, the source zone
  # comes from $TZ too.
  def assumptions(line)
    dateless = timestamps(scannable(line)).select { |match| detect_format(match) == 'time' }
    return [] if dateless.empty?
    return %i[date zone] if dateless.any? { |match| detect_zone(match).nil? }

    [:date]
  end

  # A bare time beside a timestamp that names its date or zone is most likely a
  # duration ("took 0:05"): that zone belongs to the timestamp naming it.
  def timestamps(line)
    found = line.scan(TIMESTAMP)
    anchored?(found) ? found.grep_v(BARE_TIME) : found
  end

  def anchored?(found)
    found.any? { |match| detect_format(match) != 'time' || detect_zone(match) }
  end

  def convert_match(match, from:, to:, format:, date: nil)
    time = parse(match, from:, to:, date:)
    format_time(time.localtime, format, match)
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
    # Time.parse rolls 2026-02-30 forward to 2026-03-02, moving a logged event
    # to another day with no signal. Leave impossible dates untranslated.
    raise ArgumentError, "impossible date: #{str}" unless real_date?(str)

    # Time.parse reads the "p" of "3:45 p.m." as the military zone P (-03:00).
    str = str.sub(/([AaPp])\.([Mm])\.?/, '\1\2')

    # Canonical case, so a lowercase abbreviation resolves exactly as its
    # uppercase one does -- pst a fixed -08:00, not Los Angeles with DST.
    if (abbr = detect_zone(str))
      str = str.delete_suffix(abbr) + abbr.upcase
      abbr = abbr.upcase
    end
    zone = aliased_zone(abbr)

    if zone
      # Strip the abbreviation: left in place, Time.parse's own zone table
      # would win over the IANA zone we just resolved it to.
      in_zone(str.delete_suffix(abbr).rstrip, zone, to)
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

  # A working copy safe to scan and rewrite. Every pattern is ASCII, so a line
  # carrying stray bytes is matched as bytes and the rest comes through
  # untouched -- rather than killing the run on an encoding error.
  def scannable(line)
    line.valid_encoding? ? line.dup : line.b
  end

  def real_date?(str)
    m = str.match(/\A(\d{4})-(\d{2})-(\d{2})/)
    m.nil? || Date.valid_date?(m[1].to_i, m[2].to_i, m[3].to_i)
  end

  def time_only?(str)
    str.match?(/\A\d{1,2}:/)
  end

  def format_time(time, fmt, original)
    tz = time.utc_offset == 0 ? 'Z' : time.strftime('%:z')

    case fmt
    when :time then return time.strftime('%H:%M:%S')
    when :iso then return time.strftime('%Y-%m-%d %H:%M:%S') + tz
    when :short
      # Always labelled: a cross-timezone tool whose output doesn't say which
      # zone it is in gets pasted into a ticket and read wrong.
      return time.strftime('%Y-%m-%d %H:%M') + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
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

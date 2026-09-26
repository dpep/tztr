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
  # A numeric offset within the -12..+14 real zones occupy.
  NUM_OFFSET = /[+-](?:0\d|1[0-3])[0-5]\d\b|[+-]1400\b/
  ZONE = /(?:#{ABBREVIATION})\b|#{NUM_OFFSET}/

  # The offset each abbreviation names. Standard and daylight ones are fixed
  # whatever the date -- CEST is +02:00 even in January -- as Time.parse reads
  # the US ones. Only the generic ET, CT, MT and PT follow DST.
  ZONE_OFFSETS = {
    'UTC' => '+00:00', 'GMT' => '+00:00', 'UT' => '+00:00', 'Z' => '+00:00',
    'EST' => '-05:00', 'EDT' => '-04:00', 'CST' => '-06:00', 'CDT' => '-05:00',
    'MST' => '-07:00', 'MDT' => '-06:00', 'PST' => '-08:00', 'PDT' => '-07:00',
    'HST' => '-10:00', 'AKST' => '-09:00', 'AKDT' => '-08:00',
    'CET' => '+01:00', 'CEST' => '+02:00', 'BST' => '+01:00', 'IST' => '+05:30',
    'JST' => '+09:00', 'KST' => '+09:00', 'HKT' => '+08:00',
    'AEST' => '+10:00', 'AEDT' => '+11:00', 'NZST' => '+12:00', 'NZDT' => '+13:00',
  }.freeze
  # A dotted meridiem takes its closing dot; an undotted one leaves a
  # following full stop to the sentence.
  MERIDIEM = /[AaPp](?:\.[Mm]\.|\.?[Mm]\b)/

  DAY = /(?:Mon|Tue|Wed|Thu|Fri|Sat|Sun)/
  MONTH_ABBRS = %w[Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec].freeze
  MON = /(?:#{MONTH_ABBRS.join('|')})/

  # Dates with named months, read whole so the weekday and day roll over with
  # the clock. date(1)/ctime: Fri Sep 25 22:14:42 PDT 2026, zone optional.
  UNIX_DATE = /\A#{DAY} (?<mon>#{MON})  ?(?<day>\d{1,2}) (?<time>\S+) (?:(?<zone>\S+) )?(?<year>\d{4})\z/
  # RFC 2822 / HTTP: Fri, 25 Sep 2026 22:14:42 -0700
  RFC_DATE = /\A(?<weekday>#{DAY}, )?(?<day>\d{1,2}) (?<mon>#{MON}) (?<year>\d{4}) (?<time>\S+) (?<zone>\S+)\z/

  PATTERNS = [
    # date(1) and ctime
    /\b#{DAY} #{MON}  ?\d{1,2} \d{2}:\d{2}:\d{2} (?:(?:#{ABBREVIATION}) )?\d{4}\b/,
    # RFC 2822
    /\b(?:#{DAY}, )?\d{1,2} #{MON} \d{4} \d{2}:\d{2}(?::\d{2})? (?:[+-]\d{4}\b|(?:#{ABBREVIATION})\b)/,
    # ISO 8601 with Z or offset: 2026-04-03T12:34:56Z, 2026-04-03T12:34:56.123+00:00
    /\d{4}-\d{2}-\d{2}T\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?(?:Z|[+-]\d{2}:?\d{2})/,
    # ISO 8601 without timezone: 2026-04-03T12:34:56
    /\d{4}-\d{2}-\d{2}T\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?/,
    # Date space 12-hour time: 2026-04-03 03:45:00 PM, 2026-04-03 03:45 PM PST
    /\d{4}-\d{2}-\d{2} \d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{MERIDIEM}(?: ?#{ZONE})?/,
    # Date space time with tz: 2026-04-03 12:34:56 UTC
    /\d{4}-\d{2}-\d{2} \d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{ZONE}/,
    # Date space time: 2026-04-03 12:34:56
    /\d{4}-\d{2}-\d{2} \d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?/,
    # Time with tz: 12:34:56 UTC, 12:34 PST, 12:34 +0530. A numeric offset
    # glued to the clock needs seconds, or 15:30-1645 would read as one.
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)? ?#{ZONE}| ?(?:#{ABBREVIATION})\b| #{NUM_OFFSET})/,
    # Time with offset: 12:34:56+00:00. Seconds required and the offset in range,
    # so the hyphen of a range like 15:30-16:45 is not read as one.
    /\b\d{1,2}:\d{2}:\d{2}(?:\.\d+)?[+-](?:(?:0\d|1[0-3]):?[0-5]\d|14:?00)\b/,
    # 12-hour time: 11:30 PM, 3:45 p.m., 3:45 PM PST
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)? ?#{MERIDIEM}(?: ?#{ZONE})?/,
    # Hour with a meridiem: 9am, 9 PM PST
    /\b\d{1,2} ?#{MERIDIEM}(?: ?#{ZONE})?/,
    # Bare time: 12:34:56, 12:34
    /\b\d{1,2}:\d{2}(?::\d{2}(?:\.\d+)?)?\b/,
  ].freeze

  # One pass over the line: at each position the alternatives are tried in the
  # order above, so a longer format wins over a shorter one inside it, and every
  # timestamp on the line converts whatever its format.
  TIMESTAMP = Regexp.union(PATTERNS)
  BARE_TIME = /\A(?:#{PATTERNS.last})\z/

  # A time-only timestamp in pieces, for sharing a range's zone and meridiem.
  TIME_PARTS = /\A(?<clock>(?<hour>\d{1,2}):\d{2}(?::\d{2}(?:\.\d+)?)?)(?: ?(?<meridiem>#{MERIDIEM}))?(?: ?(?<zone>#{ZONE}))?\z/
  RANGE_WORDS = '-|–|—|to|until|till|through|thru'
  # What joins the ends of a range (15:30-16:45, 3:30 to 4:45 PM) or the items
  # of a list (3:00, 4:00 or 5:00 PM).
  RANGE_JOIN = /\A[ \t]*(?:#{RANGE_WORDS})[ \t]*\z/i
  LIST_JOIN = /\A[ \t]*(?:,|(?:,[ \t]*)?(?:or|and))[ \t]*\z/i
  # A bare hour starting a range, the 9 of "9-10am" or "9 to 10am": at the
  # start of the text or after a space or "(", and hyphenated tight or joined
  # by a word. A spaced hyphen ("Room 7 - 3pm", "Apr 3 - 5pm") is not enough.
  RANGE_HOUR = /(?:\A|[ \t(])(?<hour>\d{1,2})(?:[-–—]|[ \t]+(?:to|until|till|through|thru)[ \t]+)\z/i
  # A day of the month, not an hour: the 1 of "Oct 1 thru 5pm".
  AFTER_MONTH = /\b(?:#{MONTH_ABBRS.join('|')})[a-z]*\.?[ \t]*\z/i

  # A timestamp found in a line: its byte offset, its text, what it is read as
  # (the text plus any zone or meridiem it shares with the end of its range or
  # list), and that range or list for -j.
  Stamp = Data.define(:offset, :text, :effective, :group, :days)

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
    out = line.byteslice(0, 0)
    pos = 0

    timestamps(scannable(line)).each do |stamp|
      out << line.byteslice(pos, stamp.offset - pos) << (convert_stamp(stamp, from:, to:, format:, date:) || stamp.text)
      pos = stamp.offset + stamp.text.bytesize
    end

    out << line.byteslice(pos, line.bytesize - pos)
  end

  # Per-match structured analysis of a line. Returns an array of hashes, one
  # per detected timestamp: { original:, detected_format:, detected_tz:,
  # translated: }. With detect: true, translation is skipped and :translated is
  # omitted.
  def matches(line, to: 'UTC', from: nil, format: nil, detect: false, date: nil)
    to = resolve_tz(to)
    from = resolve_tz(from)
    ENV['TZ'] = to
    timestamps(scannable(line)).map do |stamp|
      info = {
        # Patterns only ever match ASCII, whatever the rest of the line is.
        original: stamp.text.dup.force_encoding(Encoding::UTF_8),
        detected_format: detect_format(reading(stamp.text)),
        detected_tz: detect_zone(stamp.effective),
      }
      info[:translated] = convert_stamp(stamp, from:, to:, format:, date:) unless detect
      info[:group] = stamp.group if stamp.group
      info
    end
  end

  # Zone abbreviations written right after a timestamp but not read as one
  # because of their case (Pst), for -v to point out.
  def ignored_zones(line)
    line = scannable(line)
    tokens = []
    line.scan(TIMESTAMP) do
      token = $~.post_match[/\A ?([A-Za-z]{2,4})\b/, 1]
      next unless token && token != token.upcase && token != token.downcase

      tokens << token if ZONE_ABBREVIATIONS.include?(token.upcase)
    end
    tokens.uniq
  end

  # Which assumptions a line's timestamps force on us, for -v to disclose.
  # A timestamp with no date needs one to resolve DST in the *target* zone,
  # whether or not it names its own; without a zone as well, the source zone
  # comes from $TZ too.
  def assumptions(line)
    readings = timestamps(scannable(line)).map(&:effective)
    [
      (:zone if readings.any? { |time| detect_zone(time).nil? }),
      (:date if readings.any? { |time| time_only?(time) }),
    ].compact
  end

  # The timestamps in a line, in order. A bare time beside one that names a
  # date, zone or meridiem is most likely a duration ("took 0:05"), and is left
  # out: that zone belongs to the timestamp naming it.
  def timestamps(line)
    stamps = with_range_hours(line, scan_stamps(line))
    joins = stamps.each_cons(2).map { |head, tail| join_between(line, head, tail) }

    # Each member takes the zone and meridiem written after the one it joins:
    # "3:30 to 4:45 PM PST" starts at 3:30 PM PST. Walked backwards, so a chain
    # passes them all the way down.
    (joins.length - 1).downto(0) do |i|
      next unless joins[i]

      date, clock = split_date(stamps[i].effective)
      stamps[i] = stamps[i].with(effective: "#{date}#{range_start(clock, stamps[i + 1].effective)}")
    end

    # Forwards, each member in the same zone as the one before it takes that
    # one's date, and the next day if it is earlier on the clock: 11:30 PM to
    # 12:30 AM ends tomorrow.
    joins.each_with_index do |join, i|
      head, tail = stamps[i], stamps[i + 1]
      next unless join && detect_zone(head.effective) == detect_zone(tail.effective)

      date, clock = split_date(head.effective)
      rolls = minute_of_day(tail.effective) < minute_of_day(clock) ? 1 : 0
      stamps[i + 1] =
        if date
          tail.with(effective: "#{(Date.parse(date) + rolls).strftime('%F')} #{tail.effective}")
        else
          tail.with(days: head.days + rolls)
        end
    end

    group_ids = joins.reduce([0]) { |ids, join| ids << (join ? ids.last : ids.last + 1) }
    kept = stamps.each_index.reject { |i| stamps[i].effective.match?(BARE_TIME) }
    kept = stamps.each_index.to_a if kept.empty?

    groups = kept.group_by { |i| group_ids[i] }.transform_values do |members|
      next if members.size < 2

      type = members[0...-1].all? { |m| joins[m] == :range } ? 'range' : 'list'
      { type:, members: members.map { |m| stamps[m].text } }
    end
    kept.map { |i| (group = groups[group_ids[i]]) ? stamps[i].with(group:) : stamps[i] }
  end

  # A reading split into its date with separator, if any, and its clock.
  def split_date(time)
    m = time.match(/\A(\d{4}-\d{2}-\d{2}[T ])?(.*)\z/m)
    [m[1], m[2]]
  end

  def scan_stamps(line)
    stamps = []
    line.scan(TIMESTAMP) do
      offset, text = $~.byteoffset(0)[0], $~[0]
      stamps << Stamp.new(offset:, text:, effective: reading(text), group: nil, days: 0)
    end
    stamps
  end

  # What a timestamp is parsed as: a named-month date as YYYY-MM-DD, an hour
  # alone as its o'clock (9am is 9:00am), anything else as written.
  def reading(text)
    if (m = text.match(UNIX_DATE) || text.match(RFC_DATE))
      time = m[:time].count(':') == 1 ? "#{m[:time]}:00" : m[:time]
      date = format('%s-%02d-%02d', m[:year], MONTH_ABBRS.index(m[:mon]) + 1, m[:day].to_i)
      [date, time, m[:zone]].compact.join(' ')
    elsif text.match?(/\A\d{1,2} ?[AaPp]/)
      text.sub(/\A\d{1,2}/, '\0:00')
    else
      text
    end
  end

  # A bare hour is only a time as the start of a range whose end has a
  # meridiem: the 9 of "9-9:15am". Anywhere else it is just a number.
  def with_range_hours(line, stamps)
    prev_end = 0
    stamps.flat_map do |stamp|
      gap = text_between(line, prev_end, stamp.offset)
      gap_start = prev_end
      prev_end = stamp.offset + stamp.text.bytesize
      next [stamp] unless stamp.effective.match(TIME_PARTS)&.[](:meridiem)

      m = gap&.match(RANGE_HOUR)
      next [stamp] unless m && (1..12).cover?(m[:hour].to_i)
      next [stamp] if gap.byteslice(0, m.byteoffset(:hour)[0]).match?(AFTER_MONTH)

      head = Stamp.new(offset: gap_start + m.byteoffset(:hour)[0], text: m[:hour], effective: "#{m[:hour]}:00", group: nil, days: 0)
      [head, stamp]
    end
  end

  # How two neighbouring time-only timestamps are joined: :range, :list or nil.
  def join_between(line, head, tail)
    return unless split_date(head.effective)[1].match?(TIME_PARTS) && tail.effective.match?(TIME_PARTS)

    gap = text_between(line, head.offset + head.text.bytesize, tail.offset)
    if gap&.match?(RANGE_JOIN) then :range
    elsif gap&.match?(LIST_JOIN) then :list
    end
  end

  # The text between two byte offsets, or nil if it is not valid UTF-8.
  def text_between(line, from, to)
    text = line.byteslice(from, to - from).force_encoding(Encoding::UTF_8)
    text if text.valid_encoding?
  end

  def minute_of_day(time)
    m = time.match(TIME_PARTS)
    hour = m[:hour].to_i
    hour = hour % 12 + (m[:meridiem].start_with?('P', 'p') ? 12 : 0) if m[:meridiem]
    hour * 60 + m[:clock].split(':')[1].to_i
  end

  def range_start(head, tail)
    h = head.match(TIME_PARTS)
    t = tail.match(TIME_PARTS)
    return head unless h && t && h[:zone].nil?

    [h[:clock], h[:meridiem] || shared_meridiem(h[:hour].to_i, t), t[:zone]].compact.join(' ')
  end

  # The end's meridiem, unless that would run the range backwards: 11:30 to
  # 1:00 PM starts in the morning. A 24-hour start takes none.
  def shared_meridiem(hour, tail)
    return unless tail[:meridiem] && (1..12).cover?(hour)

    pm = tail[:meridiem].start_with?('P', 'p')
    pm = !pm if hour % 12 > tail[:hour].to_i % 12
    pm ? 'PM' : 'AM'
  end

  # Parsed as its effective reading, formatted to mirror what was written.
  def convert_stamp(stamp, from:, to:, format:, date:)
    if time_only?(stamp.effective)
      base = date ? Date.parse(date) : today_where(stamp.effective, from, to)
      date = (base + stamp.days).strftime('%F')
    end
    time = parse(stamp.effective, from:, to:, date:)
    format_time(time.localtime, format, stamp.text)
  rescue ArgumentError
    nil
  end

  # Today where a dateless timestamp was written: in the zone it names, or else
  # the source zone. What -d stands in for, and what -v names when it is absent.
  def today_where(time, from, to)
    abbr = detect_zone(time)&.upcase
    offset = zone_offset(abbr)
    return Time.now.getlocal(offset).to_date if offset

    ENV['TZ'] = aliased_zone(abbr) || from || to
    today = Time.now.to_date
    ENV['TZ'] = to
    today
  end

  # The fixed offset an abbreviation or numeric zone names, as +HH:MM.
  def zone_offset(abbr)
    return if abbr.nil?
    return abbr.sub(/\A([+-]\d{2}):?(\d{2})\z/, '\1:\2') if abbr.match?(/\A[+-]\d{2}:?\d{2}\z/)

    ZONE_OFFSETS[abbr]
  end

  # The date -v says it assumed for this line's first dateless timestamp.
  def assumed_date(line, from: nil, to: 'UTC')
    to = resolve_tz(to)
    from = resolve_tz(from)
    stamp = timestamps(scannable(line)).find { |s| time_only?(s.effective) }
    stamp && today_where(stamp.effective, from, to).strftime('%F')
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

    # Time.parse accepts 99:14 in some shapes; the Rust port never does.
    h, m, sec = str.match(/(\d{1,2}):(\d{2})(?::(\d{2}))?/)&.captures&.map(&:to_i)
    unless h && m <= 59 && sec.to_i <= 60 && (h < 24 || (h == 24 && m.zero? && sec.to_i.zero?))
      raise ArgumentError, "impossible clock: #{str}"
    end

    # A fixed abbreviation becomes the offset it names, in whatever case it was
    # written, so Time.parse's own zone table never decides.
    abbr = detect_zone(str)
    if abbr&.match?(/\A[A-Za-z]+\z/) && (offset = zone_offset(abbr.upcase))
      str = str.delete_suffix(abbr) + offset
      abbr = offset
    end
    zone = aliased_zone(abbr&.upcase)

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

  # The IANA zone a generic abbreviation (ET, PT) names; fixed ones have none.
  def aliased_zone(abbr)
    return if abbr.nil? || zone_offset(abbr)

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

  # A UTF-8 copy safe to match against, byte for byte the same length. Each
  # invalid byte becomes "?", which like the Rust port's reading of a stray
  # byte is neither letter nor digit; letters around it stay letters.
  def scannable(line)
    text = line.dup.force_encoding(Encoding::UTF_8)
    text.valid_encoding? ? text : text.scrub { |bytes| '?' * bytes.bytesize }
  end

  def real_date?(str)
    m = str.match(/\A(\d{4})-(\d{2})-(\d{2})/)
    m.nil? || Date.valid_date?(m[1].to_i, m[2].to_i, m[3].to_i)
  end

  def time_only?(str)
    str.match?(/\A\d{1,2}:/)
  end

  # A dated timestamp's clock to the precision it was written with.
  def written_clock(original)
    case original
    when /\A\S+[T ]\d{1,2}:\d{2}:\d{2}\.\d/ then '%H:%M:%S.%L'
    when /\A\S+[T ]\d{1,2}:\d{2}:\d{2}/ then '%H:%M:%S'
    else '%H:%M'
    end
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
      time.strftime("%Y-%m-%dT#{written_clock(original)}") + tz
    when /^\d{4}-\d{2}-\d{2} /
      time.strftime("%Y-%m-%d #{written_clock(original)}") + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    when UNIX_DATE
      time.strftime('%a %b %e %H:%M:%S ') + (time.utc? ? 'UTC' : time.strftime('%Z')) + time.strftime(' %Y')
    when RFC_DATE
      m = $~
      zone = m[:zone].start_with?('+', '-') ? time.strftime('%z') : (time.utc? ? 'UTC' : time.strftime('%Z'))
      time.strftime("#{'%a, ' if m[:weekday]}#{m[:day].length == 2 ? '%d' : '%-d'} %b %Y %H:%M#{':%S' if m[:time].count(':') == 2} ") + zone
    when /^\d{1,2}:\d{2}(?::\d{2})/
      time.strftime('%H:%M:%S') + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    when /^\d{1,2}:\d{2}/, /\A\d{1,2}(?:\z| ?[AaPp])/
      time.strftime('%H:%M') + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    else
      time.strftime('%Y-%m-%d %H:%M:%S') + " " + (time.utc? ? 'UTC' : time.strftime('%Z'))
    end
  end
end

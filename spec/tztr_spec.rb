require "spec_helper"
require "open3"
require "json"

RSpec.describe Tztr do
  describe ".translate" do
    it "passes through ISO Z to UTC" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "UTC"))
        .to eq("2026-04-03T12:00:00Z")
    end

    it "converts ISO Z to timezone" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles"))
        .to eq("2026-04-03T05:00:00-07:00")
    end

    it "converts ISO offset to UTC" do
      expect(Tztr.translate("2026-04-03T05:00:00-07:00", to: "UTC"))
        .to eq("2026-04-03T12:00:00Z")
    end

    it "converts ISO offset to timezone" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/New_York"))
        .to eq("2026-04-03T08:00:00-04:00")
    end

    it "preserves fractional seconds" do
      expect(Tztr.translate("2026-04-03T12:00:00.123Z", to: "America/Los_Angeles"))
        .to eq("2026-04-03T05:00:00.123-07:00")
    end

    it "converts space format with tz" do
      expect(Tztr.translate("2026-04-03 12:00:00 UTC", to: "America/Los_Angeles"))
        .to eq("2026-04-03 05:00:00 PDT")
    end

    it "passes through space format to UTC" do
      expect(Tztr.translate("2026-04-03 12:00:00 UTC", to: "UTC"))
        .to eq("2026-04-03 12:00:00 UTC")
    end

    it "converts time with tz" do
      expect(Tztr.translate("15:30 UTC", to: "America/Los_Angeles"))
        .to eq("08:30 PDT")
    end

    it "converts time with seconds" do
      expect(Tztr.translate("15:30:45 UTC", to: "America/Los_Angeles"))
        .to eq("08:30:45 PDT")
    end

    it "converts time to UTC" do
      result = Tztr.translate("08:30 PDT", to: "UTC")
      expect(result).to match(/15:30 UTC/)
    end

    it "preserves surrounding text" do
      expect(Tztr.translate("log 2026-04-03T12:00:00Z something happened", to: "America/New_York"))
        .to eq("log 2026-04-03T08:00:00-04:00 something happened")
    end

    it "replaces multiple timestamps on same line" do
      result = Tztr.translate("from 15:30 UTC to 16:45 UTC", to: "America/Los_Angeles")
      expect(result).to eq("from 08:30 PDT to 09:45 PDT")
    end

    it "converts every timestamp on a line, whatever their formats" do
      line = '{"ts":"2026-04-03T12:00:00Z","created":"2026-04-03 13:00:00 UTC","msg":"at 15:30 UTC"}'
      expect(Tztr.translate(line, to: "America/Los_Angeles", date: "2026-04-03"))
        .to eq('{"ts":"2026-04-03T05:00:00-07:00","created":"2026-04-03 06:00:00 PDT","msg":"at 08:30 PDT"}')
    end

    it "reports every timestamp on a line, in order" do
      result = Tztr.matches("15:30 UTC then 2026-04-03T12:00:00Z", detect: true)
      expect(result.map { |m| m[:detected_format] }).to eq(%w[time iso])
    end

it "leaves a bare time alone beside a timestamp that carries a date or zone" do
      # Most likely a duration: the zone belongs to the timestamp that names it.
      expect(Tztr.translate("2026-04-03T12:00:00Z took 0:05", to: "America/Los_Angeles"))
        .to eq("2026-04-03T05:00:00-07:00 took 0:05")
      expect(Tztr.translate("15:30 UTC, retry in 0:30", to: "America/Los_Angeles", date: "2026-04-03"))
        .to eq("08:30 PDT, retry in 0:30")
      expect(Tztr.matches("2026-04-03T12:00:00Z took 0:05", detect: true).map { |m| m[:original] })
        .to eq(["2026-04-03T12:00:00Z"])
    end
    
    it "still converts bare times when nothing on the line is more specific" do
      expect(Tztr.translate("from 15:30 to 16:45", to: "UTC", from: "America/Los_Angeles", date: "2026-04-03"))
        .to eq("from 22:30 UTC to 23:45 UTC")
    end

    it "treats a meridiem as specific enough to leave a bare time alone" do
      expect(Tztr.translate("meeting 3:30 PM, took 0:05", to: "UTC")).to eq("meeting 15:30 UTC, took 0:05")
    end

    describe "ranges" do
      def tr(line) = Tztr.translate(line, to: "UTC", from: "America/Los_Angeles", date: "2026-04-03")

      it "gives the start of a range the zone written after its end" do
        expect(tr("from 15:30 to 16:45 PST")).to eq("from 23:30 UTC to 00:45 UTC")
        expect(tr("15:30-16:45 PST")).to eq("23:30 UTC-00:45 UTC")
        expect(tr("15:30 – 16:45 JST")).to eq("06:30 UTC – 07:45 UTC")
      end

      it "gives the start the meridiem, unless that would run the range backwards" do
        expect(tr("from 3:30 to 4:45 PM")).to eq("from 22:30 UTC to 23:45 UTC")
        expect(tr("from 3:30 to 4:45 PM PST")).to eq("from 23:30 UTC to 00:45 UTC")
        expect(tr("11:30 to 1:00 PM PST")).to eq("19:30 UTC to 21:00 UTC")
        expect(tr("10:00 until 2:00 AM PST")).to eq("06:00 UTC until 10:00 UTC")
      end

      it "leaves a 24-hour start's clock alone" do
        expect(tr("from 15:30 to 4:45 PM PST")).to eq("from 23:30 UTC to 00:45 UTC")
      end

      it "reports the shared zone for the start of a range" do
        result = Tztr.matches("from 3:30 to 4:45 PM PST", detect: true)
        expect(result.map { |m| [m[:original], m[:detected_tz]] }).to eq([["3:30", "PST"], ["4:45 PM PST", "PST"]])
      end

      it "only shares across a range or list, not across other words" do
        expect(tr("15:30, then 16:45 PST")).to eq("15:30, then 00:45 UTC")
      end

      it "shares the trailing zone and meridiem across a list" do
        expect(tr("Options at 3:00, 4:00 or 5:00 PM PST")).to eq("Options at 23:00 UTC, 00:00 UTC or 01:00 UTC")
        expect(tr("at 3:00 and 4:00 PM")).to eq("at 22:00 UTC and 23:00 UTC")
        expect(tr("11:00, 12:00, or 1:00 PM PST")).to eq("19:00 UTC, 20:00 UTC, or 21:00 UTC")
      end

      it "does not group a time with a dated timestamp" do
        expect(Tztr.matches("2026-04-03T12:00:00Z,15:30", detect: true).map { |m| m.key?(:group) }).to eq([false])
      end

      it "lists every member of a range or list under -j" do
        range = { type: "range", members: ["3:30", "4:45 PM PST"] }
        expect(Tztr.matches("from 3:30 to 4:45 PM PST", detect: true).map { |m| m[:group] }).to eq([range, range])

        list = { type: "list", members: ["3:00", "4:00", "5:00 PM"] }
        expect(Tztr.matches("3:00, 4:00 or 5:00 PM", detect: true).map { |m| m[:group] }).to eq([list] * 3)

        expect(Tztr.matches("15:30 UTC", detect: true).first).not_to have_key(:group)
      end
    end

    describe "dates with named months" do
      def tr(line, to: "America/New_York") = Tztr.translate(line, to:, from: "America/Los_Angeles")

      it "converts date(1) output as one timestamp, weekday and day included" do
        expect(tr("Fri Sep 25 22:14:42 PDT 2026")).to eq("Sat Sep 26 01:14:42 EDT 2026")
        expect(tr("Fri Sep 25 22:14:42 2026")).to eq("Sat Sep 26 01:14:42 EDT 2026")
        expect(tr("Sat Sep  5 22:14:42 UTC 2026", to: "America/Los_Angeles")).to eq("Sat Sep  5 15:14:42 PDT 2026")
      end

      it "converts an RFC 2822 date, keeping its shape" do
        expect(tr("Fri, 25 Sep 2026 22:14:42 -0700")).to eq("Sat, 26 Sep 2026 01:14:42 -0400")
        expect(tr("25 Sep 2026 22:14 PDT")).to eq("26 Sep 2026 01:14 EDT")
        expect(tr("Date: Sat, 5 Sep 2026 12:00:00 GMT", to: "UTC")).to eq("Date: Sat, 5 Sep 2026 12:00:00 UTC")
        # "15 Apr" and "01 Aug" are not "15 am".
        expect(tr("Date: 15 Apr 2026 22:14:42 -0700", to: "Asia/Tokyo")).to eq("Date: 16 Apr 2026 14:14:42 +0900")
        expect(tr("01 Aug 2026 10:00 GMT", to: "Asia/Tokyo")).to eq("01 Aug 2026 19:00 JST")
      end

      it "reports one dated match" do
        expect(Tztr.matches("Fri Sep 25 22:14:42 PDT 2026", detect: true))
          .to eq([{ original: "Fri Sep 25 22:14:42 PDT 2026", detected_format: "datetime", detected_tz: "PDT" }])
      end

      it "leaves a date that never happened alone" do
        expect(tr("Mon Feb 30 12:00:00 UTC 2026")).to eq("Mon Feb 30 12:00:00 UTC 2026")
      end
    end

    describe "hour-only times" do
      def tr(line) = Tztr.translate(line, to: "UTC", from: "America/Los_Angeles", date: "2026-04-03")

      it "converts an hour with a meridiem" do
        expect(tr("Meeting at 9am PST")).to eq("Meeting at 17:00 UTC")
        expect(tr("at 9 PM")).to eq("at 04:00 UTC")
        expect(tr("at 9 p.m. sharp")).to eq("at 04:00 UTC sharp")
      end

      it "reads a bare hour as the start of a range whose end has a meridiem" do
        expect(tr("Standup 9-9:15am PST")).to eq("Standup 17:00 UTC-17:15 UTC")
        expect(tr("9 to 10am PST")).to eq("17:00 UTC to 18:00 UTC")
        expect(tr("11-1pm PST")).to eq("19:00 UTC-21:00 UTC")
        expect(Tztr.matches("9-9:15am PST", detect: true).map { |m| m[:original] }).to eq(["9", "9:15am PST"])
      end

      it "does not take a date or label for the start of a range" do
        expect(tr("Deadline: Friday, Apr 3 - 5pm PST")).to eq("Deadline: Friday, Apr 3 - 01:00 UTC")
        expect(tr("Due 4/3 - 5pm PST")).to eq("Due 4/3 - 01:00 UTC")
        expect(tr("2026-04-03 - 10am")).to eq("2026-04-03 - 17:00 UTC")
        expect(tr("Oct 1 thru 5pm")).to eq("Oct 1 thru 00:00 UTC")
        expect(tr("Ticket #12 - 9:30am")).to eq("Ticket #12 - 16:30 UTC")
        expect(tr("Room 7 - 3pm")).to eq("Room 7 - 22:00 UTC")
      end

      it "leaves other bare numbers alone" do
        expect(tr("won 3-10")).to eq("won 3-10")
        expect(tr("page 9, 10am standup")).to eq("page 9, 17:00 UTC standup")
        expect(tr("v1.9-10am")).to eq("v1.9-17:00 UTC")
        expect(tr("item 42-10am")).to eq("item 42-17:00 UTC")
      end
    end

    it "reads an offset only after seconds, so a hyphenated range stays a range" do
      expect(Tztr.translate("12:34:56-05:00", to: "UTC", date: "2026-04-03")).to eq("17:34:56 UTC")
      expect(Tztr.translate("15:30-16:45", to: "UTC", date: "2026-04-03")).to eq("15:30 UTC-16:45 UTC")
      expect(Tztr.translate("12:34:56-16:45", to: "UTC", date: "2026-04-03")).to eq("12:34:56 UTC-16:45 UTC")
    end

    it "does not also match a shorter format inside a longer one" do
      expect(Tztr.matches("2026-04-03 12:00:00 UTC", detect: true).map { |m| m[:original] })
        .to eq(["2026-04-03 12:00:00 UTC"])
    end

    it "formats as short with abbreviation" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :short))
        .to eq("2026-04-03 05:00 PDT")
    end

    it "formats as short with UTC label when target is UTC" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "UTC", format: :short))
        .to eq("2026-04-03 12:00 UTC")
    end

    it "formats as time" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :time))
        .to eq("05:00:00")
    end

    it "formats as iso with offset" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :iso))
        .to eq("2026-04-03 05:00:00-07:00")
    end

    it "formats as iso with Z for UTC" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "UTC", format: :iso))
        .to eq("2026-04-03 12:00:00Z")
    end

    it "applies from timezone to naive timestamps" do
      expect(Tztr.translate("2026-04-03T12:00:00", from: "America/Los_Angeles", to: "UTC"))
        .to eq("2026-04-03T19:00:00Z")
    end

    it "passes through lines without timestamps" do
      expect(Tztr.translate("no timestamps here")).to eq("no timestamps here")
    end
  end

  describe "zone abbreviation allowlist" do
    it "leaves a log level after a bare time untouched" do
      expect(Tztr.translate("15:30 INFO server started", to: "UTC"))
        .to eq("15:30 UTC INFO server started")
    end

    it "leaves a log level after a full timestamp untouched" do
      expect(Tztr.translate("2026-04-03 12:00:00 ERROR db failed", to: "UTC"))
        .to eq("2026-04-03 12:00:00 UTC ERROR db failed")
    end

    it "converts from an abbreviation Time.parse cannot resolve" do
      expect(Tztr.translate("15:30 JST", to: "UTC")).to eq("06:30 UTC")
    end

    it "reads a standard or daylight abbreviation as the offset it names" do
      tr = ->(line, date) { Tztr.translate(line, to: "UTC", date:) }
      expect(tr.("15:30 CET", "2026-07-15")).to eq("14:30 UTC")
      expect(tr.("15:30 CEST", "2026-01-15")).to eq("13:30 UTC")
      expect(tr.("15:30 BST", "2026-01-15")).to eq("14:30 UTC")
      expect(tr.("15:30 AEST", "2026-01-15")).to eq("05:30 UTC")
      expect(tr.("15:30 NZDT", "2026-07-15")).to eq("02:30 UTC")
      expect(tr.("15:30 IST", "2026-01-15")).to eq("10:00 UTC")
      expect(Tztr.translate("2026-07-15 15:30 CET", to: "UTC")).to eq("2026-07-15 14:30 UTC")
    end

    it "follows DST for a generic abbreviation" do
      expect(Tztr.translate("15:30 PT", to: "UTC", date: "2026-01-15")).to eq("23:30 UTC")
      expect(Tztr.translate("15:30 PT", to: "UTC", date: "2026-07-15")).to eq("22:30 UTC")
    end

    it "does not report a log level as a timezone" do
      expect(Tztr.matches("2026-04-03 12:00:00 ERROR db failed", detect: true))
        .to eq([{ original: "2026-04-03 12:00:00", detected_format: "datetime", detected_tz: nil }])
    end

    it "detects a lowercase abbreviation, resolving it as the uppercase one" do
      expect(Tztr.translate("15:30 utc", to: "America/New_York", date: "2026-01-15")).to eq("10:30 EST")
      expect(Tztr.translate("3:45 pm pst", to: "UTC")).to eq("23:45 UTC")
      expect(Tztr.translate("15:30 jst", to: "UTC")).to eq("06:30 UTC")
      # pdt is a fixed -07:00 like PDT, even in January.
      expect(Tztr.translate("12:00 pdt", to: "UTC", date: "2026-01-15")).to eq("19:00 UTC")
      expect(Tztr.matches("12:00 pst", detect: true).first[:detected_tz]).to eq("pst")
    end

    it "leaves a lowercase abbreviation that is also a word alone" do
      # French "is" / "this", German "is": reading them as zones is a silent wrong answer.
      expect(Tztr.translate("à 15:30 est annulée", to: "UTC")).to eq("à 15:30 UTC est annulée")
      expect(Tztr.translate("à 15:30 cet après-midi", to: "UTC")).to eq("à 15:30 UTC cet après-midi")
      expect(Tztr.translate("um 15:30 ist es", to: "UTC")).to eq("um 15:30 UTC ist es")
    end

    it "does not detect a mixed-case abbreviation" do
      expect(Tztr.translate("15:30 Pst", to: "UTC")).to eq("15:30 UTC Pst")
    end
  end

  describe "12-hour times" do
    it "maps the meridiem, including the midnight and noon boundaries" do
      expect(Tztr.translate("11:30:00 PM", to: "UTC")).to eq("23:30:00 UTC")
      expect(Tztr.translate("12:30:00 AM", to: "UTC")).to eq("00:30:00 UTC")
      expect(Tztr.translate("12:30:00 PM", to: "UTC")).to eq("12:30:00 UTC")
      expect(Tztr.translate("1:00:00 PM", to: "UTC")).to eq("13:00:00 UTC")
    end

    it "converts a 12-hour time carrying a date" do
      expect(Tztr.translate("2026-04-03 03:45:00 PM", to: "UTC"))
        .to eq("2026-04-03 15:45:00 UTC")
      expect(Tztr.translate("2026-04-03 3:45 PM", to: "UTC")).to eq("2026-04-03 15:45 UTC")
    end
  end

  describe "dated timestamps without seconds" do
    it "keeps the written date instead of assuming today" do
      expect(Tztr.translate("2026-01-15 23:30", from: "UTC", to: "America/Los_Angeles"))
        .to eq("2026-01-15 15:30 PST")
      expect(Tztr.translate("2026-01-15 23:30 UTC", to: "America/Los_Angeles")).to eq("2026-01-15 15:30 PST")
    end

    it "converts minute-precision ISO 8601, offset included" do
      expect(Tztr.translate("2026-12-31T23:30+05:30", to: "Pacific/Auckland")).to eq("2027-01-01T07:00+13:00")
      expect(Tztr.translate("2026-12-31T23:30Z", to: "Pacific/Auckland")).to eq("2027-01-01T12:30+13:00")
      expect(Tztr.translate("2026-12-31T23:30", from: "UTC", to: "UTC")).to eq("2026-12-31T23:30Z")
    end
  end

  describe "ranges with a dated start" do
    it "shares the end's zone with the start and the start's date with the end" do
      expect(Tztr.translate("2026-04-03 9:00 AM - 10:00 AM PST", to: "UTC"))
        .to eq("2026-04-03 17:00 UTC - 18:00 UTC")
      expect(Tztr.translate("2026-04-03 15:30 - 16:30", from: "UTC", to: "America/Los_Angeles"))
        .to eq("2026-04-03 08:30 PDT - 09:30 PDT")
      expect(Tztr.translate("2026-04-03 23:00 - 01:00", from: "UTC", to: "UTC", format: :iso))
        .to eq("2026-04-03 23:00:00Z - 2026-04-04 01:00:00Z")
    end
  end

  describe "dated times with a single-digit hour" do
    it "keeps the date" do
      expect(Tztr.translate("2026-01-15 9:00 UTC", to: "Etc/GMT+12")).to eq("2026-01-14 21:00 -12")
      expect(Tztr.translate("2026-01-15T9:00:00Z", to: "Etc/GMT+12")).to eq("2026-01-14T21:00:00-12:00")
    end
  end

  describe "UTC offsets" do
    it "reads a glued offset only after seconds, and only within range" do
      expect(Tztr.translate("15:30-1645", to: "UTC", date: "2026-04-03")).to eq("15:30 UTC-1645")
      expect(Tztr.translate("12:00+0530", to: "UTC", date: "2026-04-03")).to eq("12:00 UTC+0530")
      expect(Tztr.translate("12:00 +0530", to: "UTC", date: "2026-04-03")).to eq("06:30 UTC")
      expect(Tztr.translate("12:00:00-14:59", to: "UTC", date: "2026-04-03")).to eq("12:00:00 UTC-14:59 UTC")
      expect(Tztr.translate("2026-04-03 12:00:00 -1645", to: "UTC")).to eq("2026-04-03 12:00:00 UTC -1645")
      expect(Tztr.translate("12:00:00+14:00", to: "UTC", date: "2026-04-03")).to eq("22:00:00 UTC")
    end

    it "converts a dateless offset time without -d" do
      expect(Tztr.translate("12:34:56-05:00", to: "UTC")).to eq("17:34:56 UTC")
      expect(Tztr.translate("12:00 +0530", to: "UTC")).to eq("06:30 UTC")
    end
  end

  describe "impossible clocks" do
    it "leaves them alone" do
      expect(Tztr.translate("2026-09-25 99:14:42 PDT", to: "UTC")).to eq("2026-09-25 99:14:42 PDT")
      expect(Tztr.translate("99am", to: "UTC", date: "2026-04-03")).to eq("99am")
      expect(Tztr.translate("Fri Sep 25 25:14:42 PDT 2026", to: "UTC")).to eq("Fri Sep 25 25:14:42 PDT 2026")
    end
  end

  describe "ranges crossing midnight" do
    it "keeps a range an hour long whatever today is where" do
      # The start and the rolled end share one base date, whichever zone's
      # "today" it came from.
      %w[Etc/GMT-14 Etc/GMT+12 Asia/Tokyo].each do |to|
        out = Tztr.translate("11:30 PM to 12:30 AM PST", to:, format: :iso)
        start, finish = out.split(" to ").map { |t| Time.parse(t) }
        expect(finish - start).to eq(3600), "#{to}: #{out}"
      end
    end

    def iso(line) = Tztr.translate(line, to: "UTC", format: :iso, date: "2026-04-03")

    it "moves a later member that is earlier on the clock to the next day" do
      expect(iso("11:30 PM to 12:30 AM PST")).to eq("2026-04-04 07:30:00Z to 2026-04-04 08:30:00Z")
      expect(iso("11:30 PM, 12:15 AM or 1:00 AM PST"))
        .to eq("2026-04-04 07:30:00Z, 2026-04-04 08:15:00Z or 2026-04-04 09:00:00Z")
      expect(iso("22:00-02:00 UTC")).to eq("2026-04-03 22:00:00Z-2026-04-04 02:00:00Z")
    end

    it "leaves a range that stays within one day alone" do
      expect(iso("9:00 to 17:00 UTC")).to eq("2026-04-03 09:00:00Z to 2026-04-03 17:00:00Z")
    end

    it "converts a 12-hour time carrying a zone" do
      expect(Tztr.translate("3:45 PM PST", to: "UTC")).to eq("23:45 UTC")
    end

    it "consumes a dotted meridiem whole" do
      expect(Tztr.translate("11:30 p.m.", to: "UTC")).to eq("23:30 UTC")
      expect(Tztr.translate("at 11:30 A.M. sharp", to: "UTC")).to eq("at 11:30 UTC sharp")
      expect(Tztr.translate("3:45 P.M. PST", to: "UTC")).to eq("23:45 UTC")
      expect(Tztr.translate("11:30 a.m", to: "UTC")).to eq("11:30 UTC")
    end

    it "leaves a sentence's full stop after an undotted meridiem" do
      expect(Tztr.translate("It starts at 11:30 PM.", to: "UTC")).to eq("It starts at 23:30 UTC.")
    end
  end

  describe "out-of-range fields" do
    it "normalizes a time that overflows into the next day" do
      expect(Tztr.translate("24:00 UTC", to: "UTC")).to eq("00:00 UTC")
      expect(Tztr.translate("23:59:60 UTC", to: "UTC")).to eq("00:00:00 UTC")
    end

    it "leaves an impossible calendar date alone" do
      expect(Tztr.translate("2026-02-30T12:00:00Z", to: "UTC")).to eq("2026-02-30T12:00:00Z")
      expect(Tztr.translate("2026-02-29T12:00:00Z", to: "UTC")).to eq("2026-02-29T12:00:00Z")
      expect(Tztr.translate("2026-13-03T12:00:00Z", to: "UTC")).to eq("2026-13-03T12:00:00Z")
    end
  end

  describe ".matches" do
    it "returns structured info per match" do
      expect(Tztr.matches("2026-04-03T12:00:00Z", to: "America/Los_Angeles"))
        .to eq([{
          original: "2026-04-03T12:00:00Z",
          detected_format: "iso",
          detected_tz: "Z",
          translated: "2026-04-03T05:00:00-07:00",
        }])
    end

    it "returns one entry per timestamp on a line" do
      result = Tztr.matches("from 15:30 UTC to 16:45 UTC", to: "America/Los_Angeles")
      expect(result.map { |m| m[:translated] }).to eq(["08:30 PDT", "09:45 PDT"])
    end

    it "reports a null zone for naive timestamps" do
      result = Tztr.matches("2026-04-03T12:00:00", from: "America/Los_Angeles", to: "UTC")
      expect(result.first[:detected_tz]).to be_nil
      expect(result.first[:translated]).to eq("2026-04-03T19:00:00Z")
    end

    it "labels the datetime format" do
      expect(Tztr.matches("2026-04-03 12:00:00 UTC").first[:detected_format])
        .to eq("datetime")
    end

    it "omits translated when detecting only" do
      result = Tztr.matches("15:30 PST", detect: true)
      expect(result).to eq([{
        original: "15:30 PST",
        detected_format: "time",
        detected_tz: "PST",
      }])
    end

    it "returns nothing for lines without timestamps" do
      expect(Tztr.matches("no timestamps here")).to eq([])
    end
  end

  describe "reference date (DST)" do
    it "resolves a time-only input against the given date" do
      # 15:30 in LA on Jan 15 is PST (-08:00) -> 23:30 UTC
      expect(Tztr.translate("15:30", from: "America/Los_Angeles", to: "UTC", date: "2026-01-15"))
        .to eq("23:30 UTC")
    end

    it "picks daylight time when the date falls in summer" do
      # 15:30 in LA on Jul 15 is PDT (-07:00) -> 22:30 UTC
      expect(Tztr.translate("15:30", from: "America/Los_Angeles", to: "UTC", date: "2026-07-15"))
        .to eq("22:30 UTC")
    end

    it "picks the earlier occurrence of a repeated fall-back hour" do
      expect(Tztr.translate("2026-11-01 01:30:00", from: "America/New_York", to: "UTC"))
        .to eq("2026-11-01 05:30:00 UTC")
      expect(Tztr.translate("2026-10-25 02:30:00", from: "Europe/Berlin", to: "UTC"))
        .to eq("2026-10-25 00:30:00 UTC")
    end

    it "picks the earlier occurrence for a time-only input too" do
      expect(Tztr.translate("01:30", from: "America/New_York", to: "UTC", date: "2026-11-01"))
        .to eq("05:30 UTC")
    end

    it "leaves the nonexistent spring-forward hour alone" do
      expect(Tztr.translate("2026-03-08 02:30:00", from: "America/New_York", to: "UTC"))
        .to eq("2026-03-08 07:30:00 UTC")
    end

    it "ignores the date for inputs that already carry one" do
      expect(Tztr.translate("2026-07-15T12:00:00Z", to: "UTC", date: "2026-01-15"))
        .to eq("2026-07-15T12:00:00Z")
    end
  end

  describe ".resolve_tz" do
    it "resolves abbreviations" do
      expect(Tztr.resolve_tz("pst")).to eq("America/Los_Angeles")
      expect(Tztr.resolve_tz("PST")).to eq("America/Los_Angeles")
      expect(Tztr.resolve_tz("est")).to eq("America/New_York")
      expect(Tztr.resolve_tz("utc")).to eq("UTC")
    end

    it "resolves gmt to UTC, not to British Summer Time" do
      expect(Tztr.resolve_tz("gmt")).to eq("UTC")
      expect(Tztr.translate("2026-07-15T12:00:00Z", to: "gmt")).to eq("2026-07-15T12:00:00Z")
    end

    it "keeps bst as UK civil time" do
      expect(Tztr.resolve_tz("bst")).to eq("Europe/London")
    end

    it "resolves city names" do
      expect(Tztr.resolve_tz("sf")).to eq("America/Los_Angeles")
      expect(Tztr.resolve_tz("nyc")).to eq("America/New_York")
      expect(Tztr.resolve_tz("london")).to eq("Europe/London")
      expect(Tztr.resolve_tz("tokyo")).to eq("Asia/Tokyo")
    end

    it "resolves numeric offsets" do
      expect(Tztr.resolve_tz("-7")).to eq("Etc/GMT+7")
      expect(Tztr.resolve_tz("+9")).to eq("Etc/GMT-9")
      expect(Tztr.resolve_tz("0")).to eq("UTC")
      expect(Tztr.resolve_tz("-12")).to eq("Etc/GMT+12")
    end

    it "passes through IANA names" do
      expect(Tztr.resolve_tz("America/Chicago")).to eq("America/Chicago")
    end

    it "rejects a name no timezone database knows" do
      expect { Tztr.resolve_tz("Bogus/Zone") }.to raise_error(Tztr::Error, "unknown timezone: Bogus/Zone")
      expect { Tztr.resolve_tz("America/New York") }.to raise_error(Tztr::Error)
      expect { Tztr.resolve_tz("") }.to raise_error(Tztr::Error)
    end

    it "bounds numeric offsets to the zones that exist" do
      expect(Tztr.resolve_tz("14")).to eq("Etc/GMT-14")
      expect { Tztr.resolve_tz("15") }.to raise_error(Tztr::Error, "offset out of range: 15 (expected -12..14)")
      expect { Tztr.resolve_tz("-13") }.to raise_error(Tztr::Error)
    end

    it "rejects a sub-hour numeric offset rather than mis-signing it" do
      expect { Tztr.resolve_tz("+5:30") }.to raise_error(Tztr::Error, "unknown timezone: +5:30")
    end

    it "handles nil" do
      expect(Tztr.resolve_tz(nil)).to be_nil
    end
  end

  describe ".normalize_date" do
    it "accepts the documented forms" do
      expect(Tztr.normalize_date("2026-01-15")).to eq("2026-01-15")
      expect(Tztr.normalize_date("2026/01/15")).to eq("2026-01-15")
      expect(Tztr.normalize_date("20260115")).to eq("2026-01-15")
      expect(Tztr.normalize_date("January 15, 2026")).to eq("2026-01-15")
      expect(Tztr.normalize_date("Jan 15 2026")).to eq("2026-01-15")
      expect(Tztr.normalize_date("15 January 2026")).to eq("2026-01-15")
    end

    it "rejects a date that never happened" do
      expect { Tztr.normalize_date("2026-02-30") }.to raise_error(Tztr::Error, "invalid date: 2026-02-30")
    end

    it "rejects forms outside the documented set" do
      expect { Tztr.normalize_date("15/01/2026") }.to raise_error(Tztr::Error)
      expect { Tztr.normalize_date("Mar 3") }.to raise_error(Tztr::Error)
      expect { Tztr.normalize_date("not-a-date") }.to raise_error(Tztr::Error)
    end
  end

  describe "aliases in translate" do
    it "accepts city name as to" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "sf"))
        .to eq("2026-04-03T05:00:00-07:00")
    end

    it "accepts abbreviation as to" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "et"))
        .to eq("2026-04-03T08:00:00-04:00")
    end

    it "accepts numeric offset as to" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "-7"))
        .to eq("2026-04-03T05:00:00-07:00")
    end

    it "accepts city name as from" do
      expect(Tztr.translate("2026-04-03T12:00:00", from: "sf", to: "UTC"))
        .to eq("2026-04-03T19:00:00Z")
    end

    it "converts bare time with from timezone" do
      expect(Tztr.translate("12:27:40", from: "sf", to: "UTC"))
        .to eq("19:27:40 UTC")
    end
  end

  describe "CLI" do
    TZTR = File.expand_path("../bin/tztr", __dir__)

    def run(input, *args, env: {})
      out, status = Open3.capture2(
        { "TZ" => nil }.merge(env),
        TZTR, *args,
        stdin_data: input
      )
      expect(status).to be_success
      out.chomp
    end

    def run_fail(*args, input: "")
      out, err, status = Open3.capture3({ "TZ" => nil }, TZTR, *args, stdin_data: input)
      expect(status).not_to be_success
      expect(out).to be_empty
      err.chomp
    end

    it "converts via stdin" do
      expect(run("2026-04-03T12:00:00Z", "-t", "America/Los_Angeles"))
        .to eq("2026-04-03T05:00:00-07:00")
    end

    it "handles multiline input" do
      input = "first 2026-04-03T12:00:00Z\nsecond 2026-04-03T13:00:00Z\n"
      lines = run(input, "-t", "America/Los_Angeles").split("\n")
      expect(lines[0]).to eq("first 2026-04-03T05:00:00-07:00")
      expect(lines[1]).to eq("second 2026-04-03T06:00:00-07:00")
    end

    it "uses TZ env var as default output" do
      expect(run("2026-04-03T12:00:00Z", env: { "TZ" => "America/New_York" }))
        .to eq("2026-04-03T08:00:00-04:00")
    end

    it "treats an empty TZ as unset" do
      expect(run("2026-04-03T12:00:00Z", env: { "TZ" => "" })).to eq("2026-04-03T12:00:00Z")
    end

    it "honors the POSIX leading colon in TZ" do
      expect(run("2026-04-03T12:00:00Z", env: { "TZ" => ":America/New_York" }))
        .to eq("2026-04-03T08:00:00-04:00")
    end

    it "overrides TZ env with -t flag" do
      expect(run("2026-04-03T12:00:00Z", "-t", "America/Los_Angeles", env: { "TZ" => "America/New_York" }))
        .to eq("2026-04-03T05:00:00-07:00")
    end

    it "shows help with -h" do
      out, status = Open3.capture2(TZTR, "-h")
      expect(status).to be_success
      expect(out).to match(/Usage: tztr/)
      expect(out).to match(/Timezone Translator/)
    end

    it "shows help as JSON with -h -j" do
      out, status = Open3.capture2(TZTR, "-h", "-j")
      expect(status).to be_success

      doc = JSON.parse(out)
      expect(doc["name"]).to eq("tztr")
      expect(doc["version"]).to eq(Tztr::VERSION)
      expect(doc["options"].map { |o| o["long"] }).to include("--from", "--detect")

      # Every documented option's long flag also appears in the text help.
      text, = Open3.capture2(TZTR, "-h")
      doc["options"].each { |o| expect(text).to include(o["long"]) }

      # -h -J emits the same document as a single NDJSON line.
      nd, = Open3.capture2(TZTR, "-h", "-J")
      expect(nd.lines.size).to eq(1)
      expect(JSON.parse(nd)).to eq(doc)
    end

    it "shows version with -V" do
      out, status = Open3.capture2(TZTR, "-V")
      expect(status).to be_success
      expect(out.chomp).to eq(Tztr::VERSION)
    end

    it "accepts alias as -t flag" do
      expect(run("2026-04-03T12:00:00Z", "-t", "sf"))
        .to eq("2026-04-03T05:00:00-07:00")
    end

    it "accepts numeric offset as -t flag" do
      expect(run("2026-04-03T12:00:00Z", "-t", "-7"))
        .to eq("2026-04-03T05:00:00-07:00")
    end

    it "uses TZ as implicit from for bare timestamps" do
      expect(run("12:27:40", "-t", "utc", env: { "TZ" => "America/Los_Angeles" }))
        .to eq("19:27:40 UTC")
    end

    it "leaves bare timestamp alone when TZ matches target" do
      expect(run("12:27:40", env: { "TZ" => "America/Los_Angeles" }))
        .to eq("12:27:40 PDT")
    end

    it "edits file in place with -i" do
      tmpfile = "/tmp/tztr-inplace-test.txt"
      File.write(tmpfile, "log 2026-04-03T12:00:00Z start\nlog 2026-04-03T13:00:00Z end\n")
      system({ "TZ" => nil }, TZTR, "-i", "-t", "America/Los_Angeles", tmpfile)
      result = File.read(tmpfile)
      expect(result).to eq("log 2026-04-03T05:00:00-07:00 start\nlog 2026-04-03T06:00:00-07:00 end\n")
    ensure
      File.delete(tmpfile) if File.exist?(tmpfile)
    end

    it "skips write when no changes with -i" do
      tmpfile = "/tmp/tztr-inplace-noop.txt"
      File.write(tmpfile, "no timestamps here\n")
      mtime = File.mtime(tmpfile)
      sleep 0.01
      system({ "TZ" => nil }, TZTR, "-i", "-t", "UTC", tmpfile)
      expect(File.mtime(tmpfile)).to eq(mtime)
    ensure
      File.delete(tmpfile) if File.exist?(tmpfile)
    end

    it "lists aliases with -l" do
      out, status = Open3.capture2(TZTR, "-l")
      expect(status).to be_success
      expect(out).to include("sf")
      expect(out).to include("America/Los_Angeles")
    end

    it "emits a JSON array with -j" do
      out = run("from 15:30 UTC to 16:45 UTC", "-t", "pst", "-j")
      group = { "type" => "range", "members" => ["15:30 UTC", "16:45 UTC"] }
      expect(JSON.parse(out)).to eq([
        { "original" => "15:30 UTC", "detected_format" => "time", "detected_tz" => "UTC", "translated" => "08:30 PDT", "group" => group },
        { "original" => "16:45 UTC", "detected_format" => "time", "detected_tz" => "UTC", "translated" => "09:45 PDT", "group" => group },
      ])
    end

    it "emits one JSON object per line with -J" do
      input = "2026-04-03T12:00:00Z\n2026-04-03T13:00:00Z\n"
      lines = run(input, "-t", "pst", "-J").split("\n").map { |l| JSON.parse(l) }
      expect(lines.map { |m| m["translated"] })
        .to eq(["2026-04-03T05:00:00-07:00", "2026-04-03T06:00:00-07:00"])
    end

    it "honors -F format inside JSON output" do
      out = run("2026-04-03T12:00:00Z", "-t", "pst", "-j", "-F", "time")
      expect(JSON.parse(out).first["translated"]).to eq("05:00:00")
    end

    it "reports detection without converting in JSON" do
      out = run("2026-04-03T12:00:00Z", "--detect", "-j")
      expect(JSON.parse(out)).to eq([
        { "original" => "2026-04-03T12:00:00Z", "detected_format" => "iso", "detected_tz" => "Z" },
      ])
    end

    it "reports detection as plain text with --detect" do
      out = run("2026-04-03T12:00:00Z", "--detect")
      expect(out).to eq("2026-04-03T12:00:00Z\tiso\tZ")
    end

    it "rejects combining -i with -j" do
      out, status = Open3.capture2(TZTR, "-i", "-j", "/tmp/whatever.txt")
      expect(status).not_to be_success
      expect(out).to be_empty
    end

    it "applies a reference date to time-only inputs with -d" do
      expect(run("15:30 PST", "-t", "utc", "-d", "2026-01-15"))
        .to eq("23:30 UTC")
    end

    it "accepts flexible date formats" do
      expect(run("15:30 PST", "-t", "utc", "-d", "January 15, 2026"))
        .to eq("23:30 UTC")
      expect(run("15:30 PST", "-t", "utc", "-d", "15 Jan 2026"))
        .to eq("23:30 UTC")
      expect(run("15:30 PST", "-t", "utc", "-d", "2026/01/15"))
        .to eq("23:30 UTC")
    end

    it "aborts on an unparseable date" do
      expect(run_fail("-d", "not-a-date", input: "15:30 PST")).to eq("tztr: invalid date: not-a-date")
    end

    it "aborts on a date that never happened" do
      expect(run_fail("-d", "2026-02-30", input: "15:30 PST")).to eq("tztr: invalid date: 2026-02-30")
    end

    it "labels the zone in short format even when it is the default one" do
      expect(run("2026-04-03T12:00:00Z", "-F", "short")).to eq("2026-04-03 12:00 UTC")
      expect(run("2026-04-03T12:00:00Z", "-F", "short", env: { "TZ" => "America/Los_Angeles" }))
        .to eq("2026-04-03 05:00 PDT")
    end

    it "accepts the -- terminator" do
      expect(run("2026-04-03T12:00:00Z", "-t", "utc", "--")).to eq("2026-04-03T12:00:00Z")
    end

    it "discloses with -v what a bare timestamp assumed" do
      _, err, = Open3.capture3(
        { "TZ" => "America/Los_Angeles" }, TZTR, "-t", "nyc", "-v", stdin_data: "15:30\n"
      )
      expect(err).to include("tztr: from=America/Los_Angeles (implicit, from $TZ) to=America/New_York")
      expect(err).to match(/^tztr: no -d given, assuming \d{4}-\d{2}-\d{2} for DST resolution$/)
    end

    it "discloses the date assumption for a zoned but dateless timestamp" do
      _, err, = Open3.capture3({ "TZ" => nil }, TZTR, "-t", "nyc", "-v", stdin_data: "15:30 UTC\n")
      expect(err).to match(/^tztr: no -d given, assuming \d{4}-\d{2}-\d{2} for DST resolution$/)
    end

    it "says nothing about a date the timestamp already carries" do
      _, err, = Open3.capture3({ "TZ" => nil }, TZTR, "-t", "nyc", "-v", stdin_data: "2026-04-03T12:00:00Z\n")
      expect(err).not_to include("assuming")
    end

    it "discloses nothing for a bare time left alone beside a dated one" do
      _, err, = Open3.capture3(
        { "TZ" => "America/Los_Angeles" }, TZTR, "-t", "nyc", "-v", stdin_data: "2026-04-03T12:00:00Z took 0:05\n"
      )
      expect(err).not_to include("implicit")
      expect(err).not_to include("assuming")
    end

    it "says under -v when it ignored a mixed-case zone, once per spelling" do
      _, err, = Open3.capture3({ "TZ" => nil }, TZTR, "-v", "-d", "2026-04-03", stdin_data: "15:30 Pst\n16:00 Pst\n")
      expect(err.scan('ignored "Pst"').size).to eq(1)
      expect(err).to include('tztr: ignored "Pst": a zone abbreviation is matched in all uppercase or all lowercase')
    end

    it "says nothing about a lowercase word it deliberately does not read as a zone" do
      _, err, = Open3.capture3({ "TZ" => nil }, TZTR, "-v", "-d", "2026-04-03", stdin_data: "à 15:30 est annulée\n")
      expect(err).not_to include("ignored")
    end

    it "discloses a dated timestamp that borrows its zone from $TZ" do
      _, err, = Open3.capture3(
        { "TZ" => "America/New_York" }, TZTR, "-v", "-t", "utc", stdin_data: "2026-04-03 12:00:00\n"
      )
      expect(err).to include("tztr: from=America/New_York (implicit, from $TZ) to=UTC")
      expect(err).not_to include("no -d given")
    end

    it "names the date it actually assumed" do
      _, err, = Open3.capture3({ "TZ" => nil }, TZTR, "-v", "-t", "Etc/GMT-14", stdin_data: "15:30 PST\n")
      expect(err).to include("no -d given, assuming #{(Time.now.utc - 8 * 3600).strftime('%F')}")
    end

    it "says nothing under --detect, which assumes nothing" do
      _, err, = Open3.capture3({ "TZ" => "America/New_York" }, TZTR, "--detect", "-v", stdin_data: "15:30 Pst\n")
      expect(err).to be_empty
    end

    it "says nothing about assumptions it did not make" do
      _, err, = Open3.capture3(
        { "TZ" => "America/Los_Angeles" }, TZTR, "-t", "nyc", "-v", "-f", "utc", "-d", "2026-01-15",
        stdin_data: "15:30\n"
      )
      expect(err).not_to include("implicit")
      expect(err).not_to include("assuming")
    end

    it "keeps stdout clean while disclosing under -j" do
      out, err, = Open3.capture3(
        { "TZ" => "America/Los_Angeles" }, TZTR, "-t", "nyc", "-v", "-j", stdin_data: "15:30\n"
      )
      expect(JSON.parse(out).first["original"]).to eq("15:30")
      expect(err).to include("implicit, from $TZ")
    end

    it "names the file in a missing-file error" do
      expect(run_fail("/tmp/tztr-does-not-exist.txt"))
        .to eq("tztr: /tmp/tztr-does-not-exist.txt: No such file or directory (os error 2)")
    end

    it "names the file in a directory-argument error" do
      expect(run_fail("/tmp")).to eq("tztr: /tmp: Is a directory (os error 21)")
    end

    it "reports a bad file and carries on with the rest, like cat" do
      good = "/tmp/tztr-lazy-good.txt"
      File.write(good, "2026-04-03T12:00:00Z\n")
      out, err, status = Open3.capture3({ "TZ" => nil }, TZTR, "-t", "pst", good, "/tmp/tztr-nope.txt", good)
      expect(status.exitstatus).to eq(1)
      expect(out).to eq("2026-04-03T05:00:00-07:00\n" * 2)
      expect(err.chomp).to eq("tztr: /tmp/tztr-nope.txt: No such file or directory (os error 2)")
    ensure
      File.delete(good) if File.exist?(good)
    end

    it "names the file in an -i error too" do
      expect(run_fail("-i", "/tmp/tztr-does-not-exist.txt"))
        .to eq("tztr: /tmp/tztr-does-not-exist.txt: No such file or directory (os error 2)")
    end

    it "edits every good file in place when one argument is bad, like sed -i" do
      a, b = "/tmp/tztr-inplace-a.txt", "/tmp/tztr-inplace-b.txt"
      [a, b].each { |f| File.write(f, "2026-04-03T12:00:00Z\n") }
      _, err, status = Open3.capture3({ "TZ" => nil }, TZTR, "-i", "-t", "pst", a, "/tmp", b)
      expect(status.exitstatus).to eq(1)
      expect(err.chomp).to eq("tztr: /tmp: Is a directory (os error 21)")
      expect([a, b].map { |f| File.read(f) }).to all(eq("2026-04-03T05:00:00-07:00\n"))
    ensure
      [a, b].each { |f| File.delete(f) if File.exist?(f) }
    end

    it "rejects an abbreviated long option" do
      expect(run_fail("--jso", input: "15:30 UTC")).to eq("tztr: invalid option: --jso")
      expect(run_fail("--verb", "-t", "utc", input: "15:30 UTC")).to eq("tztr: invalid option: --verb")
    end

    it "rejects an unknown option without suggesting an alternative" do
      expect(run_fail("--tox", "utc", input: "15:30 UTC")).to eq("tztr: invalid option: --tox")
      expect(run_fail("--f", "utc", input: "15:30 UTC")).to eq("tztr: invalid option: --f")
    end

    it "rejects an argument on a flag that takes none" do
      expect(run_fail("--json=foo", input: "15:30 UTC")).to eq("tztr: --json takes no argument")
      expect(run_fail("--verbose=1", "-t", "utc", input: "15:30 UTC")).to eq("tztr: --verbose takes no argument")
      expect(run_fail("--list=x", input: "15:30 UTC")).to eq("tztr: --list takes no argument")
    end

    it "reports a missing flag argument" do
      expect(run_fail("-t", input: "15:30 UTC")).to eq("tztr: missing argument for -t")
      expect(run_fail("--to", input: "15:30 UTC")).to eq("tztr: missing argument for --to")
    end

    it "accepts only the exact format spellings" do
      expect(run_fail("-F", "bogus", input: "15:30 UTC"))
        .to eq("tztr: invalid format: bogus (expected iso, short, time)")
      expect(run_fail("-F", "i", input: "15:30 UTC"))
        .to eq("tztr: invalid format: i (expected iso, short, time)")
      expect(run_fail("-F", "", input: "15:30 UTC"))
        .to eq("tztr: invalid format:  (expected iso, short, time)")
    end

    it "reports an unknown flag without a backtrace" do
      expect(run_fail("--bogus")).to eq("tztr: invalid option: --bogus")
    end

    it "preserves bytes that are not valid UTF-8" do
      input = "2026-04-03T12:00:00Z \xff\xfe junk\n2026-04-03T13:00:00Z ok\n".b
      out, _, status = Open3.capture3({ "TZ" => nil }, TZTR, "-t", "pst", stdin_data: input)
      expect(status).to be_success
      expect(out.b)
        .to eq("2026-04-03T05:00:00-07:00 \xff\xfe junk\n2026-04-03T06:00:00-07:00 ok\n".b)
    end

    it "matches a line with invalid bytes as it would a valid one" do
      # A stray byte counts as neither letter nor digit, and letters beside it
      # still count as letters, as in the Rust port.
      tr = ->(line) { Tztr.translate(line.b, to: "UTC", from: "America/Los_Angeles", date: "2026-04-03").b }
      expect(tr.("caf\xE9 standup 9-10am PST")).to eq("caf\xE9 standup 17:00 UTC-18:00 UTC".b)
      expect(tr.("\xFF é15:30 UTC")).to eq("\xFF é15:30 UTC".b)
      expect(tr.("\xFF 15:30 UTCé")).to eq("\xFF 22:30 UTC UTCé".b)
    end

    it "aborts on an unresolvable timezone" do
      expect(run_fail("-t", "Bogus/Zone", input: "2026-04-03T12:00:00Z"))
        .to eq("tztr: unknown timezone: Bogus/Zone")
    end

    it "aborts on an out-of-range numeric offset" do
      expect(run_fail("-t", "15")).to eq("tztr: offset out of range: 15 (expected -12..14)")
    end

    it "aborts on a sub-hour numeric offset" do
      expect(run_fail("-t", "+5:30")).to eq("tztr: unknown timezone: +5:30")
    end

    it "aborts on an unresolvable TZ" do
      out, err, status = Open3.capture3({ "TZ" => "Bogus/Zone" }, TZTR, stdin_data: "15:30")
      expect(status).not_to be_success
      expect(out).to be_empty
      expect(err.chomp).to eq("tztr: unknown timezone: Bogus/Zone")
    end

    it "applies the reference date inside JSON output" do
      out = run("15:30 PST", "-t", "utc", "-d", "2026-01-15", "-j")
      expect(JSON.parse(out).first["translated"]).to eq("23:30 UTC")
    end
  end
end

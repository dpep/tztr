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

    it "formats as short with abbreviation when not local" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :short))
        .to eq("2026-04-03 05:00 PDT")
    end

    it "formats as short without zone when local" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :short, local: true))
        .to eq("2026-04-03 05:00")
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

    it "handles nil" do
      expect(Tztr.resolve_tz(nil)).to be_nil
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
      expect(JSON.parse(out)).to eq([
        { "original" => "15:30 UTC", "detected_format" => "time", "detected_tz" => "UTC", "translated" => "08:30 PDT" },
        { "original" => "16:45 UTC", "detected_format" => "time", "detected_tz" => "UTC", "translated" => "09:45 PDT" },
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
      out, status = Open3.capture2(TZTR, "-d", "not-a-date", stdin_data: "15:30 PST")
      expect(status).not_to be_success
    end

    it "applies the reference date inside JSON output" do
      out = run("15:30 PST", "-t", "utc", "-d", "2026-01-15", "-j")
      expect(JSON.parse(out).first["translated"]).to eq("23:30 UTC")
    end
  end
end

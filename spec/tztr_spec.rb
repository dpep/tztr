require "spec_helper"
require "open3"

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

    it "formats as short" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :short))
        .to eq("2026-04-03 05:00")
    end

    it "formats as time" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :time))
        .to eq("05:00:00")
    end

    it "formats as iso" do
      expect(Tztr.translate("2026-04-03T12:00:00Z", to: "America/Los_Angeles", format: :iso))
        .to eq("2026-04-03 05:00:00")
    end

    it "applies from timezone to naive timestamps" do
      expect(Tztr.translate("2026-04-03T12:00:00", from: "America/Los_Angeles", to: "UTC"))
        .to eq("2026-04-03T19:00:00Z")
    end

    it "passes through lines without timestamps" do
      expect(Tztr.translate("no timestamps here")).to eq("no timestamps here")
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

    it "shows version with -v" do
      out, status = Open3.capture2(TZTR, "-v")
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
  end
end

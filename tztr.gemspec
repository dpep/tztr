require_relative "lib/tztr"

Gem::Specification.new do |s|
  s.authors     = ["Daniel Pepper"]
  s.description = "Convert timestamps between timezones. Reads from stdin or file, auto-detects timestamp formats, and preserves the original format by default."
  s.executables = ["tztr"]
  s.files       = `git ls-files * ':!:spec'`.split("\n")
  s.homepage    = "https://github.com/dpep/tztr"
  s.license     = "MIT"
  s.name        = File.basename(__FILE__, ".gemspec")
  s.summary     = "Timezone Translator"
  s.version     = Tztr::VERSION

  s.required_ruby_version = ">= 3.2"

  s.add_development_dependency 'debug', '>= 1'
  s.add_development_dependency 'rspec', '>= 3.10'
  s.add_development_dependency 'rspec-debugging'
  s.add_development_dependency 'simplecov', '>= 0.22'
end

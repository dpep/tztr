# tztr — dual implementation (Ruby reference + Rust port).
#
#   make build       - release build of the Rust CLI into rust/target/release
#   make install     - cargo install the Rust CLI into ~/.cargo/bin
#   make test        - run both test suites (rspec + cargo test)
#   make check       - Rust gate: cargo fmt --check + clippy + test
#   make fmt         - cargo fmt the Rust crate
#   make parity      - Ruby <-> Rust CLI parity harness (builds Rust first)
#   make hooks       - enable the committed git hooks (pre-push runs parity+check)
#
# Ruby is the reference implementation; Rust mirrors it. See CLAUDE.md for the
# parity contract.

CARGO    ?= cargo
RUST_DIR := rust

.DEFAULT_GOAL := help
.PHONY: help build install test check fmt parity hooks

help:
	@echo "tztr targets:"
	@echo "  make build       release build of the Rust CLI"
	@echo "  make install     cargo install the Rust CLI"
	@echo "  make test        rspec + cargo test"
	@echo "  make check       Rust gate: fmt --check + clippy + test"
	@echo "  make fmt         cargo fmt the Rust crate"
	@echo "  make parity      Ruby <-> Rust CLI parity harness"
	@echo "  make hooks       enable committed git hooks (.githooks)"

build:
	$(CARGO) build --release --manifest-path $(RUST_DIR)/Cargo.toml

install:
	$(CARGO) install --path $(RUST_DIR)/tztr

test:
	bundle exec rspec
	$(CARGO) test --manifest-path $(RUST_DIR)/Cargo.toml

# The Rust gate — mirrors CI. Run before merging/pushing.
check:
	cd $(RUST_DIR) && $(CARGO) fmt --check
	cd $(RUST_DIR) && $(CARGO) clippy --workspace --all-targets -- -D warnings
	cd $(RUST_DIR) && $(CARGO) test --workspace

fmt:
	cd $(RUST_DIR) && $(CARGO) fmt

# A bare `ruby` here is macOS system Ruby 2.6 whenever rbenv's shims are off
# PATH and `rbenv global` is unset, and the harness then reports every case as
# failing for reasons unrelated to parity. Pick the first interpreter that
# satisfies the gemspec instead; override with `make parity RUBY=/path/to/ruby`.
RUBY_CANDIDATES := ruby $(shell rbenv root 2>/dev/null)/versions/*/bin/ruby /opt/homebrew/opt/ruby/bin/ruby
RUBY ?= $(shell for r in $(RUBY_CANDIDATES); do \
	  "$$r" -e 'exit Gem::Version.new(RUBY_VERSION) >= Gem::Version.new("3.2")' 2>/dev/null \
	    && echo "$$r" && break; \
	done)

parity: build
	@test -n "$(RUBY)" || { echo "make parity: no Ruby >= 3.2 found (tried: $(RUBY_CANDIDATES))"; exit 1; }
	$(RUBY) script/parity.rb

hooks:
	git config core.hooksPath .githooks
	@echo "git hooks enabled (.githooks) — pre-push now runs parity + check"

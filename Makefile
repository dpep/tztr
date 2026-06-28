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

parity: build
	ruby script/parity.rb

hooks:
	git config core.hooksPath .githooks
	@echo "git hooks enabled (.githooks) — pre-push now runs parity + check"

SHELL := /bin/sh

BINARY := symeraseme
CARGO ?= cargo
GORELEASER ?= goreleaser
SWIFT ?= swift
PYTHON ?= python3
RUST_TARGET_DIR ?= $(if $(CARGO_TARGET_DIR),$(CARGO_TARGET_DIR),build/rust)
COVERAGE_FILE ?= $(RUST_TARGET_DIR)/coverage.json
NIGHTLY := nightly-2026-09-20

# Quote a make-expanded value as one literal POSIX shell word.
shell_quote = '$(subst ','"'"',$(1))'

RUST_BINARY := $(RUST_TARGET_DIR)/debug/symeraseme-rust
PARITY_BINARY := $(RUST_TARGET_DIR)/debug/parity

.PHONY: build test test-race lint fmt-check vet coverage clean \
	build-rust rust-gate parity app-test release-dry-run miri

build: build-rust
	cp -- $(call shell_quote,$(RUST_BINARY)) $(call shell_quote,$(BINARY))

build-rust:
	@if ! command -v $(call shell_quote,$(CARGO)) >/dev/null 2>&1; then \
		printf '%s\n' 'build-rust requires cargo on PATH (or set CARGO=...).' >&2; \
		exit 127; \
	fi
	@rm -f -- $(call shell_quote,$(RUST_BINARY))
	@mkdir -p $(call shell_quote,$(RUST_TARGET_DIR))
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) build -p symeraseme-cli --bin symeraseme-rust --locked
	@test -x $(call shell_quote,$(RUST_BINARY))

rust-gate: build-rust fmt-check vet lint test

parity: build-rust
	@if ! command -v $(call shell_quote,$(CARGO)) >/dev/null 2>&1; then \
		printf '%s\n' 'parity requires cargo on PATH (or set CARGO=...).' >&2; \
		exit 127; \
	fi
	@rm -f -- $(call shell_quote,$(PARITY_BINARY))
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) build -p parity --bin parity --locked
	@test -x $(call shell_quote,$(PARITY_BINARY))
	SYMERASEME_PARITY_LIVE_GO=0 CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) test -p parity --all-targets --all-features --locked

app-test:
	@if ! command -v $(call shell_quote,$(SWIFT)) >/dev/null 2>&1; then \
		printf '%s\n' 'app-test requires swift on PATH (or set SWIFT=...).' >&2; \
		exit 127; \
	fi
	@if ! xcodebuild -version >/dev/null 2>&1; then \
		printf '%s\n' 'app-test requires a full Xcode installation selected by xcode-select.' >&2; \
		exit 2; \
	fi
	cd app/SymairaEraseMe && $(call shell_quote,$(SWIFT)) test

release-dry-run:
	@if ! command -v $(call shell_quote,$(GORELEASER)) >/dev/null 2>&1; then \
		printf '%s\n' 'release-dry-run requires goreleaser on PATH (or set GORELEASER=...).' >&2; \
		exit 127; \
	fi
	$(call shell_quote,$(GORELEASER)) release --snapshot --clean
	$(call shell_quote,$(PYTHON)) tests/test_release_archive_verifier.py
	$(call shell_quote,$(PYTHON)) tests/verify_release_archives.py dist

test:
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) test --workspace --all-targets --all-features --locked
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) test --workspace --doc --all-features --locked

coverage:
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) CARGO=$(call shell_quote,$(CARGO)) COVERAGE_FILE=$(call shell_quote,$(COVERAGE_FILE)) bash scripts/check-rust-coverage.sh

# Do not falsely label Miri or ordinary Rust tests as the retired Go race detector.
test-race:
	@printf '%s\n' 'Go -race is retired. Use make miri for the pinned Rust interpreter contract; native concurrency tests remain in make test.' >&2
	@exit 2

miri:
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) +$(NIGHTLY) miri test -p symeraseme-core --test miri_contract --locked

vet:
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) check --workspace --all-targets --all-features --locked

lint:
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) clippy --workspace --all-targets --all-features --locked -- -D warnings

fmt-check:
	CARGO_TARGET_DIR=$(call shell_quote,$(RUST_TARGET_DIR)) $(call shell_quote,$(CARGO)) fmt --all --check

clean:
	rm -f -- $(call shell_quote,$(BINARY)) $(call shell_quote,$(COVERAGE_FILE))
	rm -rf -- $(call shell_quote,$(RUST_TARGET_DIR)) dist

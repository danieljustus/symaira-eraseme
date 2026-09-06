SHELL := /bin/sh

BINARY := symeraseme
RAW_VERSION := $(shell git describe --exact-match --tags 2>/dev/null || true)
VERSION ?= $(if $(filter v%,$(RAW_VERSION)),$(patsubst v%,%,$(RAW_VERSION)),dev)
GO ?= go
CARGO ?= cargo
GORELEASER ?= goreleaser
SWIFT ?= swift
CGO_ENABLED ?= 0
GOFLAGS ?=
COVERAGE_FILE ?= coverage.out
COVERAGE_THRESHOLD ?= 75
GO_BUILD_DIR ?= build/go
RUST_TARGET_DIR ?= build/rust
LDFLAGS ?= -s -w -X main.versionValue=$(VERSION)

GO_BINARY := $(GO_BUILD_DIR)/$(BINARY)
RUST_BINARY := $(RUST_TARGET_DIR)/debug/symeraseme-rust
PARITY_BINARY := $(RUST_TARGET_DIR)/debug/parity

.PHONY: build test test-race lint fmt-check vet coverage clean \
	build-go go-gate build-rust rust-gate parity app-test release-dry-run

build:
	CGO_ENABLED=$(CGO_ENABLED) $(GO) build $(GOFLAGS) -trimpath -ldflags "$(LDFLAGS)" -o $(BINARY) ./cmd/symeraseme

build-go:
	@rm -f "$(GO_BINARY)"
	@mkdir -p "$(GO_BUILD_DIR)"
	CGO_ENABLED=$(CGO_ENABLED) $(GO) build $(GOFLAGS) -trimpath -ldflags "$(LDFLAGS)" -o "$(GO_BINARY)" ./cmd/symeraseme
	@test -x "$(GO_BINARY)"

go-gate: fmt-check test lint vet coverage build-go

build-rust:
	@if ! command -v "$(CARGO)" >/dev/null 2>&1; then \
		printf '%s\n' 'build-rust requires cargo on PATH (or set CARGO=...).' >&2; \
		exit 127; \
	fi
	@rm -f "$(RUST_BINARY)"
	@mkdir -p "$(RUST_TARGET_DIR)"
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" build -p symeraseme-cli --bin symeraseme-rust
	@test -x "$(RUST_BINARY)"

rust-gate: build-rust
	@if ! command -v "$(CARGO)" >/dev/null 2>&1; then \
		printf '%s\n' 'rust-gate requires cargo on PATH (or set CARGO=...).' >&2; \
		exit 127; \
	fi
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" fmt --all --check
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" check --workspace --all-targets
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" clippy --workspace --all-targets -- -D warnings
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" test --workspace --all-targets
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" test --workspace --doc

parity: build-go build-rust
	@if ! command -v "$(CARGO)" >/dev/null 2>&1; then \
		printf '%s\n' 'parity requires cargo on PATH (or set CARGO=...).' >&2; \
		exit 127; \
	fi
	@rm -f "$(PARITY_BINARY)"
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" build -p parity --bin parity
	@test -x "$(PARITY_BINARY)"
	CARGO_TARGET_DIR="$(RUST_TARGET_DIR)" "$(CARGO)" test -p parity --all-targets

app-test:
	@if ! command -v "$(SWIFT)" >/dev/null 2>&1; then \
		printf '%s\n' 'app-test requires swift on PATH (or set SWIFT=...).' >&2; \
		exit 127; \
	fi
	@if ! xcodebuild -version >/dev/null 2>&1; then \
		printf '%s\n' 'app-test requires a full Xcode installation selected by xcode-select.' >&2; \
		exit 2; \
	fi
	cd app/SymairaEraseMe && "$(SWIFT)" test

release-dry-run:
	@if ! command -v "$(GORELEASER)" >/dev/null 2>&1; then \
		printf '%s\n' 'release-dry-run requires goreleaser on PATH (or set GORELEASER=...).' >&2; \
		exit 127; \
	fi
	"$(GORELEASER)" release --snapshot --clean

test:
	CGO_ENABLED=$(CGO_ENABLED) $(GO) test $(GOFLAGS) -count=1 ./...

coverage:
	rm -f $(COVERAGE_FILE)
	CGO_ENABLED=$(CGO_ENABLED) $(GO) test $(GOFLAGS) -count=1 -covermode=atomic -coverprofile=$(COVERAGE_FILE) ./...
	@awk -v threshold="$(COVERAGE_THRESHOLD)" 'NR > 1 { total += $$2; if ($$3 > 0) covered += $$2 } END { if (total == 0) { print "coverage: no statements found"; exit 1 } printf "Go coverage: %.2f%% (%d/%d statements), gate: %s%%\n", covered * 100 / total, covered, total, threshold; if (covered * 100 < total * threshold) exit 1 }' $(COVERAGE_FILE)

test-race:
	CGO_ENABLED=1 $(GO) test $(GOFLAGS) -race -count=1 ./...

vet:
	CGO_ENABLED=$(CGO_ENABLED) $(GO) vet $(GOFLAGS) ./...

lint:
	@if command -v golangci-lint >/dev/null 2>&1; then \
		golangci-lint run ./...; \
	else \
		printf '%s\n' 'golangci-lint not found; falling back to go vet'; \
		$(MAKE) vet; \
	fi

fmt-check:
	@files="$$(find . -type f -name '*.go' -not -path './vendor/*' -print)"; \
	if [ -n "$$files" ]; then \
		out="$$(gofmt -l $$files)"; \
		if [ -n "$$out" ]; then \
			printf '%s\n' 'Unformatted Go files:'; \
			printf '%s\n' $$out; \
			exit 1; \
		fi; \
	fi

clean:
	rm -f $(BINARY) $(COVERAGE_FILE)
	rm -rf "$(GO_BUILD_DIR)" "$(RUST_TARGET_DIR)" dist

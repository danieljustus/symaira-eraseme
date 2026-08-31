SHELL := /bin/sh

BINARY := symeraseme
VERSION ?= $(shell git describe --tags --exact-match 2>/dev/null || echo dev)
GO ?= go
CGO_ENABLED ?= 0
GOFLAGS ?=
COVERAGE_FILE ?= coverage.out
COVERAGE_THRESHOLD ?= 75
LDFLAGS ?= -s -w -X main.versionValue=$(VERSION)

.PHONY: build test test-race lint fmt-check vet coverage clean

build:
	CGO_ENABLED=$(CGO_ENABLED) $(GO) build $(GOFLAGS) -trimpath -ldflags "$(LDFLAGS)" -o $(BINARY) ./cmd/symeraseme

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
	rm -f $(BINARY)
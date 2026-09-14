//go:build storage_oracle

package main

import (
	"strings"
	"testing"
)

func TestValidatePinnedSHA256RejectsWrongSourceHash(t *testing.T) {
	t.Parallel()

	err := validatePinnedSHA256(
		"worktree Go storage source",
		[]byte("package eventstore\n"),
		strings.Repeat("0", 64),
	)
	if err == nil {
		t.Fatal("a wrong pinned source SHA-256 must be rejected")
	}
	if !strings.Contains(err.Error(), "worktree Go storage source SHA-256 mismatch") {
		t.Fatalf("unexpected rejection: %v", err)
	}
}

func TestValidatePinnedSHA256AcceptsMatchingSourceHash(t *testing.T) {
	t.Parallel()

	const source = "package eventstore\n"
	const sourceSHA256 = "b516d288bc0fba7755216497568ea307103fdd884768ef4548ece3b23dd34019"
	if err := validatePinnedSHA256("worktree Go storage source", []byte(source), sourceSHA256); err != nil {
		t.Fatalf("matching pinned source SHA-256 rejected: %v", err)
	}
}

func TestCanonicalSQLPreservesQuotedLiteralWhitespace(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name  string
		left  string
		right string
	}{
		{
			name:  "repeated spaces",
			left:  "CREATE TABLE request_state (current_status TEXT DEFAULT 'two  spaces')",
			right: "CREATE TABLE request_state (current_status TEXT DEFAULT 'two spaces')",
		},
		{
			name:  "tabs",
			left:  "CREATE TABLE request_state (current_status TEXT DEFAULT 'two	spaces')",
			right: "CREATE TABLE request_state (current_status TEXT DEFAULT 'two spaces')",
		},
		{
			name:  "newlines",
			left:  "CREATE TABLE request_state (current_status TEXT DEFAULT 'two\nspaces')",
			right: "CREATE TABLE request_state (current_status TEXT DEFAULT 'two spaces')",
		},
		{
			name:  "escaped quotes",
			left:  "CREATE TABLE request_state (current_status TEXT DEFAULT 'two ''spaces''')",
			right: "CREATE TABLE request_state (current_status TEXT DEFAULT 'two spaces')",
		},
	}

	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			if canonicalSQL(testCase.left) == canonicalSQL(testCase.right) {
				t.Fatalf("canonicalSQL collapsed semantic literal difference: %q vs %q", testCase.left, testCase.right)
			}
		})
	}
}

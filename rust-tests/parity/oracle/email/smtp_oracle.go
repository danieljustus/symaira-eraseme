package main

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"os"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/email"
)

const smtpSourceRevision = "5e88de93cde1adda75b4f803924b27450fc37a9a"

type smtpFixtureDocument struct {
	SourceRevision string            `json:"source_revision"`
	SourceSHA256   map[string]string `json:"source_sha256"`
	Cases          []smtpFixtureCase `json:"cases"`
	SendError      string            `json:"send_error"`
}

type smtpFixtureInput struct {
	Name      string             `json:"name"`
	From      string             `json:"from"`
	Message   email.EmailMessage `json:"message"`
	Date      string             `json:"date"`
	MessageID string             `json:"message_id"`
}

type smtpFixtureCase struct {
	Name       string           `json:"name"`
	Input      smtpFixtureInput `json:"input"`
	Message    string           `json:"message,omitempty"`
	MessageID  string           `json:"message_id,omitempty"`
	Recipients []string         `json:"recipients"`
	Error      string           `json:"error,omitempty"`
}

func smtpCases() []smtpFixtureInput {
	return []smtpFixtureInput{
		{
			Name: "multipart_with_cc_bcc_and_mixed_newlines",
			From: "sender@example.test",
			Message: email.EmailMessage{
				To:      " first@example.test, second@example.test ",
				CC:      "copy@example.test",
				BCC:     "hidden@example.test",
				Subject: "Data request ✓",
				Body:    "First\r\nSecond\rThird\n",
			},
			Date:      "2026-08-31T12:05:06Z",
			MessageID: "<fixed-message@example.test>",
		},
		{
			Name: "sanitized_headers",
			From: "sender@example.test\r\nBcc: injected@example.test",
			Message: email.EmailMessage{
				To:      "recipient@example.test\r\nBcc: leaked@example.test",
				Subject: "subject\r\nInjected: yes",
				Body:    "body",
			},
			Date:      "2026-08-31T12:00:00Z",
			MessageID: "<sanitized@example.test>",
		},
		{
			Name: "empty_from",
			Message: email.EmailMessage{
				To: "recipient@example.test",
			},
			Date:      "2026-08-31T12:00:00Z",
			MessageID: "<error@example.test>",
		},
		{
			Name: "empty_recipient",
			From: "sender@example.test",
			Message: email.EmailMessage{
				To: " \t ",
			},
			Date:      "2026-08-31T12:00:00Z",
			MessageID: "<error@example.test>",
		},
	}
}

func collectSMTPFixture() (smtpFixtureDocument, error) {
	document := smtpFixtureDocument{
		SourceRevision: smtpSourceRevision,
		SourceSHA256:   make(map[string]string, 2),
	}
	for _, path := range []string{
		"internal/email/smtp.go",
		"internal/email/types.go",
		"rust-tests/parity/oracle/email/main.go",
		"rust-tests/parity/oracle/email/smtp_oracle.go",
	} {
		contents, err := os.ReadFile(path)
		if err != nil {
			return smtpFixtureDocument{}, err
		}
		digest := sha256.Sum256(contents)
		document.SourceSHA256[path] = hex.EncodeToString(digest[:])
	}
	for _, input := range smtpCases() {
		entry := smtpFixtureCase{
			Name:       input.Name,
			Input:      input,
			Recipients: email.Recipients(input.Message),
		}
		now, err := time.Parse(time.RFC3339, input.Date)
		if err != nil {
			return smtpFixtureDocument{}, fmt.Errorf("%s date: %w", input.Name, err)
		}
		raw, messageID, err := email.BuildMIMEAt(input.Message, input.From, now, input.MessageID)
		if err != nil {
			entry.Error = err.Error()
		} else {
			entry.Message = string(raw)
			entry.MessageID = messageID
		}
		document.Cases = append(document.Cases, entry)
	}
	transport := smtpOracleTransport{err: errors.New("offline synthetic transport")}
	_, err := email.SendMessage(context.Background(), email.SMTPConfig{From: "sender@example.test"}, email.EmailMessage{To: "recipient@example.test"}, transport)
	if err == nil {
		return smtpFixtureDocument{}, errors.New("synthetic transport unexpectedly succeeded")
	}
	document.SendError = err.Error()
	return document, nil
}

type smtpOracleTransport struct{ err error }

func (t smtpOracleTransport) Send(context.Context, email.SMTPConfig, []string, []byte) error {
	return t.err
}

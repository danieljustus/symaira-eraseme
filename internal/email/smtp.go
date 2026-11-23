package email

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/tls"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net"
	"net/smtp"
	"strconv"
	"strings"
	"time"
)

var (
	ErrSMTP            = errors.New("email: smtp error")
	ErrSMTPFromMissing = errors.New("email: SMTP sender is not configured")
)

// SMTPTransport is the side-effect boundary for SMTP. Tests can inject a
// recorder; production uses NetSMTPTransport.
type SMTPTransport interface {
	Send(ctx context.Context, cfg SMTPConfig, recipients []string, message []byte) error
}

// BuildMIME creates a multipart/plain RFC 5322 message. BuildMIMEAt exists so
// tests can pin the clock and Message-ID without contacting a mailbox.
func BuildMIME(message EmailMessage, from string) ([]byte, string, error) {
	return BuildMIMEAt(message, from, time.Now(), "")
}

func BuildMIMEAt(message EmailMessage, from string, now time.Time, messageID string) ([]byte, string, error) {
	if strings.TrimSpace(from) == "" {
		return nil, "", ErrSMTPFromMissing
	}
	if strings.TrimSpace(message.To) == "" {
		return nil, "", fmt.Errorf("%w: recipient is empty", ErrSMTP)
	}
	if messageID == "" {
		var random [12]byte
		if _, err := io.ReadFull(rand.Reader, random[:]); err != nil {
			return nil, "", fmt.Errorf("%w: generate message id: %v", ErrSMTP, err)
		}
		messageID = "<" + hex.EncodeToString(random[:]) + "@symeraseme>"
	}
	boundaryHash := sha256.Sum256([]byte(messageID))
	boundary := "=_symeraseme_" + hex.EncodeToString(boundaryHash[:8])

	var b strings.Builder
	writeHeader := func(name, value string) {
		b.WriteString(name)
		b.WriteString(": ")
		b.WriteString(strings.ReplaceAll(strings.ReplaceAll(value, "\r", ""), "\n", ""))
		b.WriteString("\r\n")
	}
	writeHeader("From", from)
	writeHeader("To", message.To)
	if message.CC != "" {
		writeHeader("Cc", message.CC)
	}
	writeHeader("Subject", message.Subject)
	writeHeader("Date", now.Local().Format(time.RFC1123Z))
	writeHeader("Message-ID", messageID)
	writeHeader("MIME-Version", "1.0")
	writeHeader("Content-Type", `multipart/mixed; boundary="`+boundary+`"`)
	b.WriteString("\r\n--" + boundary + "\r\n")
	b.WriteString("Content-Type: text/plain; charset=utf-8\r\n")
	b.WriteString("Content-Transfer-Encoding: 8bit\r\n\r\n")
	b.WriteString(strings.ReplaceAll(strings.ReplaceAll(message.Body, "\r\n", "\n"), "\r", "\n"))
	b.WriteString("\r\n--" + boundary + "--\r\n")
	return []byte(b.String()), messageID, nil
}

// Recipients returns the SMTP envelope recipients. BCC is deliberately not
// added to the message headers.
func Recipients(message EmailMessage) []string {
	var out []string
	for _, field := range []string{message.To, message.CC, message.BCC} {
		for _, item := range strings.Split(field, ",") {
			if item = strings.TrimSpace(item); item != "" {
				out = append(out, item)
			}
		}
	}
	return out
}

// SendMessage builds and sends one message through the supplied transport.
func SendMessage(ctx context.Context, cfg SMTPConfig, message EmailMessage, transport SMTPTransport) (string, error) {
	if transport == nil {
		transport = NetSMTPTransport{}
	}
	raw, messageID, err := BuildMIME(message, cfg.From)
	if err != nil {
		return "", err
	}
	if err := transport.Send(ctx, cfg, Recipients(message), raw); err != nil {
		return "", fmt.Errorf("%w: %v", ErrSMTP, err)
	}
	return messageID, nil
}

// SendMessagesBatch reuses the transport boundary and preserves one result per
// message, matching the Python batch contract without exposing credentials.
func SendMessagesBatch(ctx context.Context, cfg SMTPConfig, messages []EmailMessage, transport SMTPTransport) []SendResult {
	results := make([]SendResult, 0, len(messages))
	for _, message := range messages {
		id, err := SendMessage(ctx, cfg, message, transport)
		result := SendResult{To: message.To, Subject: message.Subject, MessageID: id}
		if err != nil {
			result.Error = err.Error()
		} else {
			result.Success = true
		}
		results = append(results, result)
	}
	return results
}

// SendResult is safe to serialize: it contains no SMTP password or token.
type SendResult struct {
	Success   bool
	To        string
	Subject   string
	MessageID string
	Error     string
}

// NetSMTPTransport is the production SMTP implementation. It supports
// STARTTLS plus password or OAuth2 SASL authentication. It never logs or
// includes authentication material in errors.
type NetSMTPTransport struct{}

func (NetSMTPTransport) Send(ctx context.Context, cfg SMTPConfig, recipients []string, message []byte) error {
	if len(recipients) == 0 {
		return errors.New("no recipients")
	}
	if cfg.Host == "" {
		return errors.New("smtp host is empty")
	}
	if cfg.Port <= 0 {
		return errors.New("smtp port must be positive")
	}
	timeout := cfg.Timeout
	if timeout <= 0 {
		timeout = 30 * time.Second
	}
	dialer := net.Dialer{Timeout: timeout}
	address := net.JoinHostPort(cfg.Host, strconv.Itoa(cfg.Port))
	conn, err := dialer.DialContext(ctx, "tcp", address)
	if err != nil {
		return err
	}
	defer conn.Close()
	client, err := smtp.NewClient(conn, cfg.Host)
	if err != nil {
		return err
	}
	defer client.Close()
	if cfg.UseTLS {
		if ok, _ := client.Extension("STARTTLS"); !ok {
			return errors.New("SMTP server does not support STARTTLS")
		}
		if err := client.StartTLS(&tls.Config{ServerName: cfg.Host, MinVersion: tls.VersionTLS12}); err != nil {
			return err
		}
	}
	if cfg.OAuth2 != nil {
		if err := client.Auth(xoauth2Auth{token: *cfg.OAuth2}); err != nil {
			return err
		}
	} else if cfg.Username != "" && cfg.Password != "" {
		if err := client.Auth(smtp.PlainAuth("", cfg.Username, cfg.Password, cfg.Host)); err != nil {
			return err
		}
	}
	if err := client.Mail(cfg.From); err != nil {
		return err
	}
	for _, recipient := range recipients {
		if err := client.Rcpt(recipient); err != nil {
			return err
		}
	}
	writer, err := client.Data()
	if err != nil {
		return err
	}
	if _, err := writer.Write(message); err != nil {
		_ = writer.Close()
		return err
	}
	if err := writer.Close(); err != nil {
		return err
	}
	return client.Quit()
}

type xoauth2Auth struct{ token OAuth2Token }

func (a xoauth2Auth) Start(*smtp.ServerInfo) (string, []byte, error) {
	if a.token.Username == "" || a.token.AccessToken == "" {
		return "", nil, errors.New("oauth2 username/token is empty")
	}
	initial := "user=" + a.token.Username + "\x01auth=Bearer " + a.token.AccessToken + "\x01\x01"
	return "XOAUTH2", []byte(initial), nil
}

func (xoauth2Auth) Next(_ []byte, more bool) ([]byte, error) {
	if more {
		return nil, errors.New("SMTP XOAUTH2 authentication rejected")
	}
	return nil, nil
}

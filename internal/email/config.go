package email

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

// LoadSMTPConfig reads the Python-compatible SYMERASEME_SMTP_* variables.
func LoadSMTPConfig() (SMTPConfig, error) {
	port, err := envPort("SYMERASEME_SMTP_PORT", 587)
	if err != nil {
		return SMTPConfig{}, err
	}
	useTLS := envBool("SYMERASEME_SMTP_TLS", true)
	return SMTPConfig{
		Host:     envDefault("SYMERASEME_SMTP_HOST", "localhost"),
		Port:     port,
		Username: os.Getenv("SYMERASEME_SMTP_USER"),
		Password: os.Getenv("SYMERASEME_SMTP_PASSWORD"),
		UseTLS:   useTLS,
		From:     os.Getenv("SYMERASEME_SMTP_FROM"),
	}, nil
}

// LoadIMAPConfig reads IMAP_* variables and resolves only vault URI values.
// Literal passwords are passed through; unresolved vault URIs fail closed.
func LoadIMAPConfig() (IMAPConfig, error) {
	port, err := envPort("IMAP_PORT", 993)
	if err != nil {
		return IMAPConfig{}, err
	}
	password := os.Getenv("IMAP_PASSWORD")
	if password != "" {
		password, err = identity.ResolveSecret(password, identity.SecretResolver{
			EnvFallback:     "IMAP_PASSWORD",
			KeyringService:  "symeraseme-imap",
			KeyringUsername: "IMAP_PASSWORD",
		})
		if err != nil {
			return IMAPConfig{}, fmt.Errorf("email: cannot resolve IMAP password: %w", err)
		}
	}
	return IMAPConfig{
		Host:        envDefault("IMAP_HOST", "imap.gmail.com"),
		Port:        port,
		Username:    os.Getenv("IMAP_USERNAME"),
		Password:    password,
		UseTLS:      envBool("IMAP_SSL", true),
		Folder:      envDefault("IMAP_FOLDER", "INBOX"),
		SinceDays:   envInt("IMAP_SINCE_DAYS", 1),
		MaxMessages: envInt("IMAP_MAX_MESSAGES", 50),
	}, nil
}

func envDefault(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}

func envBool(name string, fallback bool) bool {
	value, ok := os.LookupEnv(name)
	if !ok || strings.TrimSpace(value) == "" {
		return fallback
	}
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "1", "true", "yes", "on":
		return true
	case "0", "false", "no", "off":
		return false
	default:
		return fallback
	}
}

func envInt(name string, fallback int) int {
	value, err := strconv.Atoi(os.Getenv(name))
	if err != nil || value < 0 {
		return fallback
	}
	return value
}

func envPort(name string, fallback int) (int, error) {
	value := os.Getenv(name)
	if value == "" {
		return fallback, nil
	}
	port, err := strconv.Atoi(value)
	if err != nil || port < 1 || port > 65535 {
		return 0, fmt.Errorf("email: %s must be a valid TCP port", name)
	}
	return port, nil
}

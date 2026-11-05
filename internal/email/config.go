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

// IMAPConfigOptions carries explicit transport overrides. OAuth2 overrides are
// resolved before the password so a usable token always takes precedence.
type IMAPConfigOptions struct {
	OAuth2AccessToken string
	OAuth2Username    string
}

// OAuth2ResolutionSource controls which fallback context is allowed while
// resolving an OAuth2 reference.
type OAuth2ResolutionSource uint8

const (
	OAuth2FromEnvironment OAuth2ResolutionSource = iota
	OAuth2FromExplicitArgument
)

type OAuth2ResolutionOptions struct {
	Source OAuth2ResolutionSource
}

// LoadIMAPConfig reads IMAP_* variables and resolves only vault URI values.
// Literal passwords are passed through; unresolved vault URIs fail closed.
func LoadIMAPConfig() (IMAPConfig, error) {
	return LoadIMAPConfigWithOptions(IMAPConfigOptions{})
}

// LoadIMAPConfigWithOptions applies explicit OAuth2 overrides before resolving
// any password. An explicit OAuth2 reference never falls back to its
// environment token; environment references retain their normal fallback and
// keyring context.
func LoadIMAPConfigWithOptions(options IMAPConfigOptions) (IMAPConfig, error) {
	port, err := envPort("IMAP_PORT", 993)
	if err != nil {
		return IMAPConfig{}, err
	}
	username := os.Getenv("IMAP_USERNAME")
	oauthUsername := os.Getenv("IMAP_OAUTH2_USERNAME")
	if oauthUsername == "" {
		oauthUsername = username
	}
	if options.OAuth2Username != "" {
		oauthUsername = options.OAuth2Username
	}

	accessToken := os.Getenv("IMAP_OAUTH2_ACCESS_TOKEN")
	resolutionSource := OAuth2FromEnvironment
	if options.OAuth2AccessToken != "" {
		accessToken = options.OAuth2AccessToken
		resolutionSource = OAuth2FromExplicitArgument
	}
	oauth2, err := ResolveIMAPOAuth2(accessToken, oauthUsername, OAuth2ResolutionOptions{Source: resolutionSource})
	if err != nil {
		return IMAPConfig{}, err
	}

	password := os.Getenv("IMAP_PASSWORD")
	if oauth2 == nil && password != "" {
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
		Username:    username,
		Password:    password,
		UseTLS:      envBool("IMAP_SSL", true),
		Folder:      envDefault("IMAP_FOLDER", "INBOX"),
		SinceDays:   envInt("IMAP_SINCE_DAYS", 14),
		MaxMessages: envInt("IMAP_MAX_MESSAGES", 50),
		OAuth2:      oauth2,
	}, nil
}

// ResolveIMAPOAuth2 resolves an optional IMAP OAuth2 access token. Secret
// references use the same resolver and Python-compatible keyring names as the
// legacy implementation; literal tokens are accepted for compatibility but
// are never included in returned errors.
func ResolveIMAPOAuth2(accessToken, username string, options ...OAuth2ResolutionOptions) (*OAuth2Token, error) {
	if accessToken == "" {
		return nil, nil
	}
	resolution := OAuth2ResolutionOptions{Source: OAuth2FromEnvironment}
	if len(options) > 0 {
		resolution = options[0]
	}
	resolver := identity.SecretResolver{
		KeyringService:  "symeraseme-oauth2",
		KeyringUsername: "oauth2:" + username + ":access_token",
	}
	if resolution.Source != OAuth2FromExplicitArgument {
		resolver.EnvFallback = "IMAP_OAUTH2_ACCESS_TOKEN"
	}
	resolved, err := identity.ResolveSecret(accessToken, resolver)
	if err != nil {
		return nil, fmt.Errorf("email: cannot resolve IMAP OAuth2 access token: %w", err)
	}
	if resolved == "" {
		return nil, fmt.Errorf("email: cannot resolve IMAP OAuth2 access token: empty value")
	}
	return &OAuth2Token{Username: username, AccessToken: resolved}, nil
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

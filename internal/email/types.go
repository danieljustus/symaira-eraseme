package email

import "time"

// EmailMessage is the content and envelope for one outgoing message.
type EmailMessage struct {
	To      string
	Subject string
	Body    string
	CC      string
	BCC     string
}

// Envelope contains message metadata returned by an IMAP listing.
type Envelope struct {
	ID      string
	Subject string
	From    string
	To      string
	Date    *time.Time
	Flags   []string
}

// Message contains a fetched email and its decoded plain-text body.
type Message struct {
	ID        string
	Subject   string
	From      string
	To        string
	Date      *time.Time
	Body      string
	Flags     []string
	MessageID string
	ThreadID  string
	IMAPUID   uint32
}

// SMTPConfig controls an SMTP connection. Password and OAuth2 token are
// write-only operational inputs: no package logger or error includes them.
type SMTPConfig struct {
	Host     string
	Port     int
	Username string
	Password string
	UseTLS   bool
	From     string
	Timeout  time.Duration
	OAuth2   *OAuth2Token
}

// IMAPConfig controls an IMAP connection and bounded inbox fetch.
type IMAPConfig struct {
	Host        string
	Port        int
	Username    string
	Password    string
	UseTLS      bool
	Folder      string
	SinceDays   int
	MaxMessages int
	Timeout     time.Duration
	OAuth2      *OAuth2Token
}

// OAuth2Token is sufficient for SASL XOAUTH2/OAUTHBEARER authentication.
// Token values must be obtained through a configured secret boundary.
type OAuth2Token struct {
	Username    string
	AccessToken string
}

// InboundMessage is the transport-neutral representation used by inbox
// polling and matching. HeaderBytes/BodyBytes are intentionally not exposed:
// callers receive parsed, bounded content only.
type InboundMessage = Message

// RemovalRequest is the subset needed to associate a reply with a request.
type RemovalRequest struct {
	ID       int64
	BrokerID string
}

// MatchedMessage is a message annotated with the matching decision.
type MatchedMessage struct {
	Message     Message
	RequestID   *int64
	MatchMethod string
}

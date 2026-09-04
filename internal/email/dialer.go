package email

import (
	"context"
	"crypto/tls"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/emersion/go-imap"
	"github.com/emersion/go-imap/client"
	"github.com/emersion/go-sasl"
)

// NetIMAPDialer connects to a live IMAP server. Implicit TLS is used when
// cfg.UseTLS is true; otherwise STARTTLS is mandatory before authentication.
type NetIMAPDialer struct {
	// TLSConfig overrides the TLS configuration if non-nil (useful in tests).
	TLSConfig *tls.Config
}

func NewNetIMAPDialer() *NetIMAPDialer { return &NetIMAPDialer{} }

func (d *NetIMAPDialer) Dial(ctx context.Context, cfg IMAPConfig) (IMAPSession, error) {
	if cfg.Host == "" {
		return nil, fmt.Errorf("%w: host is empty", ErrIMAP)
	}
	port := cfg.Port
	if port <= 0 {
		if cfg.UseTLS {
			port = 993
		} else {
			port = 143
		}
	}
	addr := net.JoinHostPort(cfg.Host, strconv.Itoa(port))
	timeout := cfg.Timeout
	if timeout <= 0 {
		timeout = 30 * time.Second
	}
	timeout = boundedTimeout(ctx, timeout)

	conn, err := (&net.Dialer{Timeout: timeout}).DialContext(ctx, "tcp", addr)
	if err != nil {
		return nil, imapError("connect/login failed", err, cfg.Password, secretFromOAuth(cfg.OAuth2))
	}
	deadlineCleanup := setContextDeadline(ctx, conn, timeout)
	defer func() { deadlineCleanup() }()

	if cfg.UseTLS {
		tlsConfig := imapTLSConfig(d.TLSConfig, cfg.TLSConfig, cfg.Host)
		tlsConn := tls.Client(conn, tlsConfig)
		if err := tlsConn.HandshakeContext(ctx); err != nil {
			_ = conn.Close()
			return nil, imapError("connect/login failed", err, cfg.Password, secretFromOAuth(cfg.OAuth2))
		}
		conn = tlsConn
		deadlineCleanup()
		deadlineCleanup = setContextDeadline(ctx, conn, timeout)
	}

	c, err := client.New(conn)
	if err != nil {
		_ = conn.Close()
		return nil, imapError("connect/login failed", err, cfg.Password, secretFromOAuth(cfg.OAuth2))
	}
	c.Timeout = timeout

	if !cfg.UseTLS {
		supported, supportErr := c.SupportStartTLS()
		if supportErr != nil {
			_ = c.Close()
			return nil, imapError("STARTTLS capability check failed", supportErr, cfg.Password, secretFromOAuth(cfg.OAuth2))
		}
		if supported {
			tlsConfig := imapTLSConfig(d.TLSConfig, cfg.TLSConfig, cfg.Host)
			if err := c.StartTLS(tlsConfig); err != nil {
				_ = c.Close()
				return nil, imapError("STARTTLS failed", err, cfg.Password, secretFromOAuth(cfg.OAuth2))
			}
		} else if !cfg.AllowInsecureCleartextAuth {
			_ = c.Close()
			return nil, fmt.Errorf("%w: cleartext authentication prohibited: server does not support STARTTLS", ErrIMAP)
		}
	}

	session := &netIMAPSession{client: c, conn: conn, cfg: cfg}
	if cfg.OAuth2 != nil && cfg.OAuth2.AccessToken != "" {
		user := cfg.OAuth2.Username
		if user == "" {
			user = cfg.Username
		}
		auth := &xoauth2Client{username: user, token: cfg.OAuth2.AccessToken}
		if err := c.Authenticate(auth); err != nil {
			_ = session.Close()
			return nil, imapError("connect/login failed", err, cfg.OAuth2.AccessToken, xoauth2Payload(user, cfg.OAuth2.AccessToken))
		}
	} else if cfg.Password != "" {
		if err := c.Login(cfg.Username, cfg.Password); err != nil {
			_ = session.Close()
			return nil, imapError("connect/login failed", err, cfg.Password)
		}
	}
	return session, nil
}

func imapTLSConfig(dialerConfig, config *tls.Config, host string) *tls.Config {
	if config != nil {
		return config
	}
	if dialerConfig != nil {
		return dialerConfig
	}
	return &tls.Config{MinVersion: tls.VersionTLS12, ServerName: host}
}

func setContextDeadline(ctx context.Context, conn net.Conn, timeout time.Duration) func() {
	timeout = boundedTimeout(ctx, timeout)
	_ = conn.SetDeadline(time.Now().Add(timeout))
	cancelDone := make(chan struct{})
	stopCancel := context.AfterFunc(ctx, func() {
		_ = conn.SetDeadline(time.Now())
		close(cancelDone)
	})
	return func() {
		if !stopCancel() {
			<-cancelDone
		}
		_ = conn.SetDeadline(time.Time{})
	}
}

func boundedTimeout(ctx context.Context, timeout time.Duration) time.Duration {
	if deadline, ok := ctx.Deadline(); ok {
		remaining := time.Until(deadline)
		if remaining <= 0 {
			return time.Nanosecond
		}
		if remaining < timeout {
			return remaining
		}
	}
	return timeout
}

type xoauth2Client struct {
	username string
	token    string
}

func (x *xoauth2Client) Start() (string, []byte, error) {
	if x.username == "" || x.token == "" {
		return "", nil, errors.New("oauth2 username or access token is empty")
	}
	return "XOAUTH2", []byte(xoauth2Payload(x.username, x.token)), nil
}

func (x *xoauth2Client) Next(_ []byte) ([]byte, error) {
	return nil, errors.New("XOAUTH2 challenge rejected")
}

func xoauth2Payload(username, token string) string {
	return "user=" + username + "\x01auth=Bearer " + token + "\x01\x01"
}

var _ sasl.Client = (*xoauth2Client)(nil)

type netIMAPSession struct {
	client    *client.Client
	conn      net.Conn
	cfg       IMAPConfig
	closeOnce sync.Once
}

func (s *netIMAPSession) begin(ctx context.Context) func() {
	originalTimeout := s.client.Timeout
	timeout := boundedTimeout(ctx, sessionTimeout(s.cfg.Timeout))
	// go-imap applies Client.Timeout inside execute, so set the bounded value on
	// the client as well as the connection. Context cancellation without a
	// deadline is converted into an immediate I/O deadline.
	s.client.Timeout = timeout
	_ = s.conn.SetDeadline(time.Now().Add(timeout))
	cancelDone := make(chan struct{})
	stopCancel := context.AfterFunc(ctx, func() {
		_ = s.conn.SetDeadline(time.Now())
		close(cancelDone)
	})
	return func() {
		if !stopCancel() {
			<-cancelDone
		}
		s.client.Timeout = originalTimeout
		_ = s.conn.SetDeadline(time.Time{})
	}
}

func sessionTimeout(timeout time.Duration) time.Duration {
	if timeout <= 0 {
		return 30 * time.Second
	}
	return timeout
}

func (s *netIMAPSession) Select(ctx context.Context, folder string) (uint32, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	clearDeadline := s.begin(ctx)
	defer clearDeadline()
	mbox, err := s.client.Select(folder, true)
	if err != nil {
		return 0, imapError("folder select failed", err, s.cfg.Password, secretFromOAuth(s.cfg.OAuth2))
	}
	return mbox.UidValidity, nil
}

func (s *netIMAPSession) SearchUID(ctx context.Context, uidRange string, since ...time.Time) ([]uint32, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	clearDeadline := s.begin(ctx)
	defer clearDeadline()
	seqSet, err := imap.ParseSeqSet(uidRange)
	if err != nil {
		return nil, imapError("invalid UID range", err)
	}
	criteria := imap.NewSearchCriteria()
	criteria.Uid = seqSet
	if len(since) > 0 && !since[0].IsZero() {
		criteria.Since = since[0].UTC()
	}
	uids, err := s.client.UidSearch(criteria)
	if err != nil {
		return nil, imapError("UID search failed", err, s.cfg.Password, secretFromOAuth(s.cfg.OAuth2))
	}
	return uids, nil
}

func (s *netIMAPSession) Fetch(ctx context.Context, uids []uint32) ([]FetchedMessage, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(uids) == 0 {
		return nil, nil
	}
	clearDeadline := s.begin(ctx)
	defer clearDeadline()

	seqSet := new(imap.SeqSet)
	seqSet.AddNum(uids...)
	headerSection, _ := imap.ParseBodySectionName("BODY.PEEK[HEADER]")
	textSection, _ := imap.ParseBodySectionName("BODY.PEEK[TEXT]")
	items := []imap.FetchItem{imap.FetchUid, imap.FetchFlags, imap.FetchInternalDate, headerSection.FetchItem(), textSection.FetchItem()}

	// go-imap v1 owns the producer and closes the channel. Keep the producer
	// cancellable by consuming concurrently and closing the socket on context
	// cancellation; the producer is always joined before returning.
	messagesChan := make(chan *imap.Message, len(uids))
	errCh := make(chan error, 1)
	go func() { errCh <- s.client.UidFetch(seqSet, items, messagesChan) }()
	var fetched []FetchedMessage
	for {
		select {
		case <-ctx.Done():
			_ = s.conn.Close()
			<-errCh
			return nil, ctx.Err()
		case msg, ok := <-messagesChan:
			if !ok {
				if err := <-errCh; err != nil {
					return nil, imapError("UID fetch failed", err, s.cfg.Password, secretFromOAuth(s.cfg.OAuth2))
				}
				return fetched, nil
			}
			if msg == nil {
				continue
			}
			headerBytes, err := readFetchBody(msg, headerSection)
			if err != nil {
				return nil, imapError("read fetched header failed", err, s.cfg.Password, secretFromOAuth(s.cfg.OAuth2))
			}
			bodyBytes, err := readFetchBody(msg, textSection)
			if err != nil {
				return nil, imapError("read fetched body failed", err, s.cfg.Password, secretFromOAuth(s.cfg.OAuth2))
			}
			fetched = append(fetched, FetchedMessage{UID: msg.Uid, Flags: append([]string(nil), msg.Flags...), Header: headerBytes, Body: bodyBytes, InternalDate: msg.InternalDate})
		}
	}
}

func readFetchBody(msg *imap.Message, section *imap.BodySectionName) ([]byte, error) {
	if reader := msg.GetBody(section); reader != nil {
		return readBoundedBody(reader)
	}
	for sec, reader := range msg.Body {
		if sec != nil && sec.Specifier == section.Specifier {
			return readBoundedBody(reader)
		}
	}
	return nil, nil
}

func readBoundedBody(reader io.Reader) ([]byte, error) {
	const maxBodyBytes = 64 * 1024
	body, err := io.ReadAll(io.LimitReader(reader, maxBodyBytes+1))
	if err != nil {
		return nil, err
	}
	if len(body) > maxBodyBytes {
		return nil, fmt.Errorf("IMAP body section exceeds %d bytes", maxBodyBytes)
	}
	return body, nil
}

func (s *netIMAPSession) Close() error {
	s.closeOnce.Do(func() {
		_ = s.conn.SetDeadline(time.Now().Add(time.Second))
		_ = s.client.Logout()
		_ = s.client.Close()
		_ = s.conn.Close()
	})
	return nil
}

// RedactError replaces raw and base64-encoded forms of each secret while
// preserving errors.Is/errors.As through the original cause.
func RedactError(err error, secrets ...string) error {
	if err == nil {
		return nil
	}
	msg := err.Error()
	for _, secret := range secrets {
		if secret == "" {
			continue
		}
		for _, encoded := range []string{
			secret,
			base64.StdEncoding.EncodeToString([]byte(secret)),
			base64.RawStdEncoding.EncodeToString([]byte(secret)),
			base64.URLEncoding.EncodeToString([]byte(secret)),
			base64.RawURLEncoding.EncodeToString([]byte(secret)),
		} {
			msg = strings.ReplaceAll(msg, encoded, "[REDACTED]")
		}
	}
	return &redactedError{message: msg, cause: err}
}

type redactedError struct {
	message string
	cause   error
}

func (e *redactedError) Error() string { return e.message }
func (e *redactedError) Unwrap() error { return e.cause }

func imapError(stage string, err error, secrets ...string) error {
	if err == nil {
		return fmt.Errorf("%w: %s", ErrIMAP, stage)
	}
	return fmt.Errorf("%w: %s: %w", ErrIMAP, stage, RedactError(err, secrets...))
}

func secretFromOAuth(tok *OAuth2Token) string {
	if tok != nil {
		return tok.AccessToken
	}
	return ""
}

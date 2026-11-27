package email_test

import (
	"bufio"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"crypto/x509/pkix"
	"encoding/base64"
	"fmt"
	"io"
	"math/big"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"
)

type fakeMessage struct {
	UID     uint32
	Flags   []string
	Header  string
	Body    string
	Invalid bool
}

type fakeFolder struct {
	UIDValidity uint32
	Messages    []fakeMessage
}

type fakeServer struct {
	listener  net.Listener
	tlsConfig *tls.Config
	clientTLS *tls.Config
	startTLS  bool
	addr      string
	port      int

	mu          sync.Mutex
	transcript  []string
	folders     map[string]*fakeFolder
	authErr     error
	selectErr   error
	searchErr   error
	fetchErr    error
	delay       time.Duration
	requireUser string
	requirePass string
	requireTok  string

	closed chan struct{}
}

func newTestCertificate() (tls.Certificate, *x509.CertPool, error) {
	priv, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return tls.Certificate{}, nil, err
	}
	template := x509.Certificate{
		SerialNumber: big.NewInt(1),
		Subject:      pkix.Name{CommonName: "127.0.0.1"},
		NotBefore:    time.Now().Add(-1 * time.Hour),
		NotAfter:     time.Now().Add(24 * time.Hour),
		IPAddresses:  []net.IP{net.ParseIP("127.0.0.1")},
		DNSNames:     []string{"localhost", "imap.example.test"},
		KeyUsage:     x509.KeyUsageKeyEncipherment | x509.KeyUsageDigitalSignature,
		ExtKeyUsage:  []x509.ExtKeyUsage{x509.ExtKeyUsageServerAuth},
	}
	certDER, err := x509.CreateCertificate(rand.Reader, &template, &template, &priv.PublicKey, priv)
	if err != nil {
		return tls.Certificate{}, nil, err
	}
	cert := tls.Certificate{Certificate: [][]byte{certDER}, PrivateKey: priv}
	pool := x509.NewCertPool()
	parsed, err := x509.ParseCertificate(certDER)
	if err != nil {
		return tls.Certificate{}, nil, err
	}
	pool.AddCert(parsed)
	return cert, pool, nil
}

func startFakeIMAPServer(useTLS bool) (*fakeServer, error) {
	return startFakeIMAPServerMode(useTLS, false)
}

func startFakeIMAPStartTLSServer() (*fakeServer, error) {
	return startFakeIMAPServerMode(false, true)
}

func startFakeIMAPServerMode(useTLS, supportStartTLS bool) (*fakeServer, error) {
	var l net.Listener
	var serverCert tls.Certificate
	var certPool *x509.CertPool
	var err error

	if useTLS || supportStartTLS {
		serverCert, certPool, err = newTestCertificate()
		if err != nil {
			return nil, err
		}
	}
	serverTLS := &tls.Config{
		Certificates: []tls.Certificate{serverCert},
		MinVersion:   tls.VersionTLS12,
	}
	if useTLS {
		l, err = tls.Listen("tcp", "127.0.0.1:0", serverTLS)
		if err != nil {
			return nil, err
		}
	} else {
		l, err = net.Listen("tcp", "127.0.0.1:0")
		if err != nil {
			return nil, err
		}
	}

	tcpAddr := l.Addr().(*net.TCPAddr)
	s := &fakeServer{
		listener:    l,
		tlsConfig:   serverTLS,
		startTLS:    supportStartTLS,
		addr:        tcpAddr.IP.String(),
		port:        tcpAddr.Port,
		folders:     make(map[string]*fakeFolder),
		closed:      make(chan struct{}),
		requireUser: "testuser",
		requirePass: "testpass",
		requireTok:  "testtoken",
	}

	if useTLS || supportStartTLS {
		s.clientTLS = &tls.Config{
			RootCAs:    certPool,
			ServerName: "127.0.0.1",
			MinVersion: tls.VersionTLS12,
		}
	}

	// Default INBOX
	s.folders["INBOX"] = &fakeFolder{
		UIDValidity: 100,
		Messages:    nil,
	}

	go s.serve()
	return s, nil
}

func (s *fakeServer) close() {
	s.mu.Lock()
	select {
	case <-s.closed:
		s.mu.Unlock()
		return
	default:
		close(s.closed)
	}
	s.mu.Unlock()
	_ = s.listener.Close()
}

func (s *fakeServer) getTranscript() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]string(nil), s.transcript...)
}

func (s *fakeServer) serve() {
	for {
		conn, err := s.listener.Accept()
		if err != nil {
			return
		}
		go s.handleConn(conn)
	}
}

func (s *fakeServer) handleConn(conn net.Conn) {
	defer func() { _ = conn.Close() }()
	reader := bufio.NewReader(conn)
	tlsActive := !s.startTLS

	// Greeting
	_, _ = fmt.Fprintf(conn, "* OK IMAP4rev1 Service Ready\r\n")

	var selectedFolder *fakeFolder

	for {
		line, err := reader.ReadString('\n')
		if err != nil {
			return
		}
		line = strings.TrimRight(line, "\r\n")
		s.mu.Lock()
		s.transcript = append(s.transcript, line)
		delay := s.delay
		authErr := s.authErr
		selectErr := s.selectErr
		searchErr := s.searchErr
		fetchErr := s.fetchErr
		reqUser := s.requireUser
		reqPass := s.requirePass
		reqTok := s.requireTok
		s.mu.Unlock()

		if delay > 0 {
			time.Sleep(delay)
		}

		parts := strings.SplitN(line, " ", 3)
		if len(parts) < 2 {
			continue
		}
		tag := parts[0]
		cmd := strings.ToUpper(parts[1])
		rest := ""
		if len(parts) > 2 {
			rest = parts[2]
		}

		switch cmd {
		case "CAPABILITY":
			startTLSCapability := ""
			if s.startTLS && !tlsActive {
				startTLSCapability = " STARTTLS"
			}
			_, _ = fmt.Fprintf(conn, "* CAPABILITY IMAP4rev1%s AUTH=PLAIN AUTH=XOAUTH2 SASL-IR\r\n%s OK CAPABILITY completed\r\n", startTLSCapability, tag)

		case "STARTTLS":
			if !s.startTLS || tlsActive {
				_, _ = fmt.Fprintf(conn, "%s BAD STARTTLS unavailable\r\n", tag)
				continue
			}
			_, _ = fmt.Fprintf(conn, "%s OK Begin TLS negotiation now\r\n", tag)
			tlsConn := tls.Server(conn, s.tlsConfig)
			if err := tlsConn.Handshake(); err != nil {
				return
			}
			conn = tlsConn
			reader = bufio.NewReader(conn)
			tlsActive = true

		case "LOGIN":
			if authErr != nil {
				_, _ = fmt.Fprintf(conn, "%s NO [AUTHENTICATIONFAILED] %s\r\n", tag, authErr.Error())
				continue
			}
			// Parse username and password
			loginParts := strings.Fields(rest)
			user, pass := "", ""
			if len(loginParts) >= 2 {
				user = strings.Trim(loginParts[0], "\"")
				pass = strings.Trim(loginParts[1], "\"")
			}
			if reqUser != "" && (user != reqUser || pass != reqPass) {
				_, _ = fmt.Fprintf(conn, "%s NO [AUTHENTICATIONFAILED] Invalid credentials\r\n", tag)
			} else {
				_, _ = fmt.Fprintf(conn, "%s OK [CAPABILITY IMAP4rev1] Logged in\r\n", tag)
			}

		case "AUTHENTICATE":
			authArgs := strings.Fields(rest)
			mech := ""
			if len(authArgs) > 0 {
				mech = strings.ToUpper(authArgs[0])
			}
			if mech == "XOAUTH2" {
				if authErr != nil {
					_, _ = fmt.Fprintf(conn, "%s NO [AUTHENTICATIONFAILED] %s\r\n", tag, authErr.Error())
					continue
				}
				authPayload := ""
				if len(authArgs) > 1 {
					authPayload = authArgs[1]
				} else {
					// Challenge continuation
					_, _ = fmt.Fprintf(conn, "+\r\n")
					respLine, readErr := reader.ReadString('\n')
					if readErr != nil {
						return
					}
					authPayload = strings.TrimRight(respLine, "\r\n")
				}
				tokValid := false
				if authPayload != "" {
					decoded, decErr := base64.StdEncoding.DecodeString(authPayload)
					if decErr == nil {
						tokValid = strings.Contains(string(decoded), reqTok) || reqTok == ""
					}
				}
				if tokValid {
					_, _ = fmt.Fprintf(conn, "%s OK Authenticated\r\n", tag)
				} else {
					_, _ = fmt.Fprintf(conn, "%s NO [AUTHENTICATIONFAILED] Invalid token\r\n", tag)
				}
			} else {
				_, _ = fmt.Fprintf(conn, "%s NO Unsupported auth\r\n", tag)
			}

		case "SELECT", "EXAMINE":
			if selectErr != nil {
				_, _ = fmt.Fprintf(conn, "%s NO %s\r\n", tag, selectErr.Error())
				continue
			}
			folderName := strings.Trim(rest, "\"")
			s.mu.Lock()
			f, ok := s.folders[folderName]
			s.mu.Unlock()
			if !ok {
				f = &fakeFolder{UIDValidity: 1, Messages: nil}
				s.mu.Lock()
				s.folders[folderName] = f
				s.mu.Unlock()
			}
			selectedFolder = f
			count := len(f.Messages)
			_, _ = fmt.Fprintf(conn, "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n")
			_, _ = fmt.Fprintf(conn, "* %d EXISTS\r\n", count)
			_, _ = fmt.Fprintf(conn, "* 0 RECENT\r\n")
			_, _ = fmt.Fprintf(conn, "* OK [UIDVALIDITY %d] UIDs valid\r\n", f.UIDValidity)
			_, _ = fmt.Fprintf(conn, "%s OK [READ-ONLY] Select completed\r\n", tag)

		case "UID":
			uidSubArgs := strings.SplitN(rest, " ", 2)
			if len(uidSubArgs) == 0 {
				_, _ = fmt.Fprintf(conn, "%s BAD Missing UID subcommand\r\n", tag)
				continue
			}
			uidCmd := strings.ToUpper(uidSubArgs[0])
			uidRest := ""
			if len(uidSubArgs) > 1 {
				uidRest = uidSubArgs[1]
			}

			if uidCmd == "SEARCH" {
				if searchErr != nil {
					_, _ = fmt.Fprintf(conn, "%s NO %s\r\n", tag, searchErr.Error())
					continue
				}
				if selectedFolder == nil {
					_, _ = fmt.Fprintf(conn, "%s NO No folder selected\r\n", tag)
					continue
				}
				// Parse start UID from UID range, e.g. "UID 1:*" or "1:*"
				startUID := uint32(1)
				searchTokens := strings.Fields(uidRest)
				for i, tok := range searchTokens {
					if strings.ToUpper(tok) == "UID" && i+1 < len(searchTokens) {
						rangeToken := searchTokens[i+1]
						parts := strings.Split(rangeToken, ":")
						if n, parseErr := strconv.ParseUint(parts[0], 10, 32); parseErr == nil {
							startUID = uint32(n)
						}
					}
				}

				var matchingUIDs []string
				for _, msg := range selectedFolder.Messages {
					if msg.UID >= startUID {
						matchingUIDs = append(matchingUIDs, strconv.FormatUint(uint64(msg.UID), 10))
					}
				}
				if len(matchingUIDs) > 0 {
					_, _ = fmt.Fprintf(conn, "* SEARCH %s\r\n", strings.Join(matchingUIDs, " "))
				} else {
					_, _ = fmt.Fprintf(conn, "* SEARCH\r\n")
				}
				_, _ = fmt.Fprintf(conn, "%s OK UID SEARCH completed\r\n", tag)

			} else if uidCmd == "FETCH" {
				if fetchErr != nil {
					_, _ = fmt.Fprintf(conn, "%s NO %s\r\n", tag, fetchErr.Error())
					continue
				}
				if selectedFolder == nil {
					_, _ = fmt.Fprintf(conn, "%s NO No folder selected\r\n", tag)
					continue
				}

				// Find requested UIDs from fetch arg
				fetchTokens := strings.Fields(uidRest)
				if len(fetchTokens) == 0 {
					_, _ = fmt.Fprintf(conn, "%s BAD Invalid FETCH syntax\r\n", tag)
					continue
				}
				uidsSpec := fetchTokens[0]
				uidMap := make(map[uint32]bool)
				for _, part := range strings.Split(uidsSpec, ",") {
					part = strings.TrimSpace(part)
					if strings.Contains(part, ":") {
						rangeParts := strings.Split(part, ":")
						start, err1 := strconv.ParseUint(strings.TrimSpace(rangeParts[0]), 10, 32)
						end, err2 := strconv.ParseUint(strings.TrimSpace(rangeParts[1]), 10, 32)
						if err1 == nil && err2 == nil {
							for k := uint32(start); k <= uint32(end); k++ {
								uidMap[k] = true
							}
						}
					} else {
						if n, err := strconv.ParseUint(part, 10, 32); err == nil {
							uidMap[uint32(n)] = true
						}
					}
				}

				seq := 1
				for _, msg := range selectedFolder.Messages {
					if !uidMap[msg.UID] {
						seq++
						continue
					}
					if msg.Invalid {
						// Malformed header without proper RFC 5322 CRLF separator
						badHeader := "Not-A-Header"
						_, _ = fmt.Fprintf(conn, "* %d FETCH (UID %d FLAGS (\\Seen) BODY[HEADER] {%d}\r\n%s BODY[TEXT] {5}\r\nhello)\r\n",
							seq, msg.UID, len(badHeader), badHeader)
					} else {
						headerBytes := []byte(msg.Header)
						bodyBytes := []byte(msg.Body)
						flagsStr := "\\Seen"
						if len(msg.Flags) > 0 {
							flagsStr = strings.Join(msg.Flags, " ")
						}
						_, _ = fmt.Fprintf(conn, "* %d FETCH (UID %d FLAGS (%s) BODY[HEADER] {%d}\r\n%s BODY[TEXT] {%d}\r\n%s)\r\n",
							seq, msg.UID, flagsStr, len(headerBytes), headerBytes, len(bodyBytes), bodyBytes)
					}
					seq++
				}
				_, _ = fmt.Fprintf(conn, "%s OK UID FETCH completed\r\n", tag)

			} else {
				_, _ = fmt.Fprintf(conn, "%s BAD Unsupported UID command\r\n", tag)
			}

		case "LOGOUT":
			_, _ = fmt.Fprintf(conn, "* BYE IMAP4rev1 Server logging out\r\n%s OK LOGOUT completed\r\n", tag)
			return

		case "NOOP":
			_, _ = fmt.Fprintf(conn, "%s OK NOOP completed\r\n", tag)

		default:
			_, _ = fmt.Fprintf(conn, "%s BAD Command unrecognized\r\n", tag)
		}
	}
}

// Silence unused linter if needed
var _ io.Closer = (*fakeServer)(nil)

func (s *fakeServer) Close() error {
	s.close()
	return nil
}

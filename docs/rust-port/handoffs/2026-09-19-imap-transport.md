# Cut 2026-09-19 (3): IMAP-Transport (Plain-TCP) portiert — DOM-006 weiter PARTIAL

Vierter Cut des Tages. Interne Dokumentation, nicht veröffentlichen.

## Basis

- Branch `migration/rust-imap-client-20260919` in
  `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/tmp/wt-imap`; Basis `main` = `5cd0cb60`.
- Neu: `crates/symeraseme-core/src/email/imap/{mod,client}.rs`,
  `crates/symeraseme-core/tests/imap_transport_parity.rs`,
  `crates/symeraseme-core/tests/support/imap_server.rs` (scripted server, gehört
  in die Tests, nicht in die Bibliothek).

## Was jetzt belegt ist

`rust-tests/parity/oracle/imap-transport/transcript_cases.json` — 11 Fälle, vom
echten `NetIMAPDialer` gegen den Fake-Server der Go-Tests erzeugt; der Generator
ist gleichzeitig Wächter (`go test ./internal/email -run TestDialerOracle`),
ohne `-update` schlägt er bei Drift fehl. Neun Fälle replayt
`tests/imap_transport_parity.rs` byte-genau, inklusive Kommando-Transkript:

- `<tag> CAPABILITY`, `<tag> LOGIN "user" "pass"`, `<tag> AUTHENTICATE XOAUTH2
  <base64>` (SASL-IR), `<tag> EXAMINE INBOX` (read-only),
  `<tag> UID SEARCH CHARSET UTF-8 UID 1:*`,
  `<tag> UID FETCH 1:2 (UID FLAGS INTERNALDATE BODY.PEEK[HEADER] BODY.PEEK[TEXT])`,
  `<tag> LOGOUT`.
- Fehlertexte byte-genau: Cleartext-Verweigerung, `connect/login failed: Invalid
  credentials` (nach der NO-Antwort folgt ein Aufräum-LOGOUT), `invalid UID
  range: imap: bad sequence set value "…"`, `IMAP body section exceeds 65536 bytes`.
- Rückgaben: UIDVALIDITY, Suchliste, gefetchte Nachrichten (UID, Flags, Header-
  und Body-Bytes).

## Gemessene Regel statt Annahme

- `imap.FormatMailboxName` (go-imap v1.2.1, `mailbox.go:308`): `INBOX` wird roh
  gesendet, **jeder andere** Mailbox-Name als gequoteter String. Genau deshalb
  steht im Orakel `EXAMINE INBOX`, aber `EXAMINE "Missing"`.
- Ein fehlgeschlagenes LOGIN wird vom Go-Client mit `LOGOUT` abgeräumt; das
  Kommando steht im aufgezeichneten Transkript.
- Die Parser sitzen hinter `(UID` bzw. `FLAGS (`: zwei Off-by-one-Fehler
  (`start + 8` bei sieben Zeichen Präfix, Token-Vergleich auf `UID` statt
  `(UID`) machten UID 0 und Flag `Seen` statt `\Seen` — beide vom Test gefunden.
- Ein FETCH-Abschnitt ist ein Literal, das am Zeilenende angekündigt wird
  (`{n}` + CRLF + n Bytes); zeilenweises Lesen schneidet ihn am ersten CRLF
  *innerhalb* der Nutzdaten ab. Der Client setzt die Antwort daher erst zu
  Bytes zusammen und liest die Abschnitte daraus.

## Negativkontrollen (beide Richtungen belegt)

| Änderung | Ergebnis |
|---|---|
| `EXAMINE` → `SELECT` | rot: „the command transcript differs from the Go oracle" |
| `CHARSET UTF-8` entfernt | rot: dieselbe Transkript-Zusicherung |
| beide zurückgebaut | grün, 9/9 Byte-Fälle |

## Bewusste Grenzen

- **TLS/STARTTLS bleibt Go.** `use_tls=true` und ein angebotenes STARTTLS
  ergeben einen expliziten Fehler („the TLS transport is not ported yet"),
  keinen vorgetäuschten Handshake. Die beiden TLS-Orakelfälle sind als
  `replay: "go-only"` mit Begründung aufgezeichnet: der Go-Server erzeugt sein
  Zertifikat pro Lauf, und ein Private Key im Repository ist keine Option.
- **Nicht-ASCII-Mailboxen**: der Port quotet den Namen in UTF-8, Go wandelt
  vorher in modifiziertes UTF-7. Im Port als `ponytail:`-Grenze markiert; die
  Konfiguration liefert den Ordner aus `IMAP_FOLDER` und ist im gepinnten Korpus
  ASCII.
- `IMAPConfig.Timeout` und `AllowInsecureCleartextAuth` sind Struct-Felder ohne
  Umgebungsvariable (Go liest dafür keine); der Transport setzt bei 0 selbst
  30 s. Der erste Entwurf hatte dafür `IMAP_TIMEOUT_SECONDS` und
  `IMAP_ALLOW_INSECURE_CLEARTEXT_AUTH` erfunden — geprüft und entfernt.

## Delegationslehre (für die nächste Runde)

Zwei Kindprozesse (gpt-5.6-luna) lieferten trotz `status=completed` keinen
Test und keinen Commit; der erste hinterließ zusätzlich einen nicht
kompilierenden Baum. Beide Meldungen waren unbrauchbar, die Arbeit musste
verifiziert bzw. selbst zu Ende geführt werden. Konsequenz: erst
`git log`/`git status`/`cargo check` im Worktree prüfen, dann der Meldung
glauben; Briefings gestaffelt („nach jedem Schritt kompilieren") und mit
„wenn du nicht fertig wirst: nichts committen, aber den Baum kompilierend
hinterlassen".

## Nächster Slice

1. `poll_inbox` im Rust-MCP-Handler verdrahten (MCP-003 14/26 → 15/26) — der
   Transport existiert jetzt dafür.
2. Danach TLS/STARTTLS: eigene Belegstrategie nötig (Zertifikat aus dem
   Fixture-Verzeichnis oder ein TLS-Server im Rust-Test, ohne Private Key im
   Repo).
3. `execute` (MCP-003) bleibt offen.

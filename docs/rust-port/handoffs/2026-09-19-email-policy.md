# Cut 2026-09-19 (2): Inbox-Policy portiert — DOM-006 PARTIAL

Zweiter Cut des Tages. Er schließt den ersten Block der Email-Migration ab:
Regelwerk ja, Transport nein. Interne Dokumentation, nicht veröffentlichen.

## Basis

- Repo `symaira-eraseme`, `main` = `76759b80` (#977).
- Go-Toolchain `go1.26.6`, Rust `cargo +1.98.0`,
  `CARGO_TARGET_DIR=/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/cargo-target`.

## Was jetzt in Rust ist

`crates/symeraseme-core/src/email/`:

| Datei | Go-Herkunft | Inhalt |
|---|---|---|
| `policy.rs` | `internal/email/imap.go`, `inbox.go` | `poll_inbox`, `poll_folders`, `match_reply_to_request`, `normalize_subject`, `subject_matches`, `parse_email_body`, `redact_error` |
| `service.rs` | `service.go` | `InboxService::poll_and_match` — HWM wird erst nach allen Inserts committet |
| `hwm.rs` | `hwm.go` | `HwmStore`-Trait, `MemoryHwmStore`, `StagingHwmStore` |
| `parse.rs` | `net/mail`, `net/textproto`, `mime` | Header-Block, Faltung, RFC-2047-Wörter, Datumslayouts |
| `config.rs` | `config.go` | `IMAP_*`-Vertrag, Literal-Passthrough, OAuth2-Vorrang |
| `wire.rs` | `encoding/json` | Feldreihenfolge, HTML-Escaping, `null` vs `[]` |
| `session.rs` | `imap.go` | `ImapDialer`/`ImapSession` als Trait-Grenze |

Nicht portiert (bleibt Go): `dialer.go` (Netz/TLS/SASL) und `oauth2.go`.

## Beleg

`rust-tests/parity/oracle/email/main.go` erzeugt
`rust-tests/parity/oracle/email/email_cases.json` aus dem **produzierenden**
Go-Paket: 18 Subjekt-Normalisierungen, 8 Body-Kürzungen, 6 Subjekt-Vergleiche,
3 Match-Fälle, 22 Header-Parse-Fälle, 12 Poll-Szenarien, 3 Service-Läufe,
7 Config-Fälle. Das IMAP ist dabei ein **skriptetes Session-Objekt**, kein
Server — wie in Go's eigenen Tests.

Zwei Läufe byte-identisch; das Fixture ist byte-identisch zum Live-Lauf
(`cmp`). `crates/symeraseme-core/tests/email_parity.rs` spielt jeden Fall
gegen den Rust-Port und vergleicht **Zeichen für Zeichen** (Whitespace wird
vorher entfernt, Feldreihenfolge bleibt erhalten).

Negativkontrolle: HTML-Escaping in `wire.rs` entfernt → Test rot; wieder
eingesetzt → grün. Der Test vergleicht also wirklich die Go-Bytes.

## Gepinnte Policy (Auszug, alles aus echten Go-Antworten)

- HWM: `1:*` kalt, `6:*` nach HWM 5, `1:*` bei geänderter UIDVALIDITY,
  `last_uid` 4294967295 → `email: imap error: UID range exhausted: last UID is
  UINT32_MAX`, kein Wrap.
- Fenster: `MaxMessages=2` mit UIDs 1/2/6 → FETCH `[1,2]`, HWM 2 (ältestes
  Fenster zuerst), nicht 6.
- Leere Suche → HWM wird auf die UIDVALIDITY mit `last_uid` 0 geschrieben, der
  Aufrufer bekommt `null` (Go's nil-Slice), nicht `[]`.
- FETCH-Antwort ohne angeforderten UID → Fehler
  `UID fetch omitted requested message: UID 2 missing from FETCH response`,
  HWM bleibt.
- Such-/Fetch-/Select-Fehler → HWM bleibt unverändert (Wiederholung sicher).
- `since_days` filtert undatiert und zu alte Nachrichten; HWM zählt trotzdem
  alle verarbeiteten UIDs. Datumsfälle nutzen absichtlich extreme Jahre
  (1970/2199), damit der Fall nicht mit der Uhr wandert.
- Ordnerübergreifend wird per Message-ID dedupliziert.
- Service: Inserts zuerst, dann Commit; Insert-Fehler →
  `email: persist inbox reply: reply insert failed` und **kein** HWM.

Header/Encoded-Words (aus Go's Quellen abgeleitet und gemessen):

- Faltung: `trim(Zeile) + " " + trim(Fortsetzung)`.
- Adjazente Encoded-Words: Whitespace dazwischen verschwindet.
- `=?UTF-8?Q/B?...?=` und `=?ISO-8859-1?...?=` werden dekodiert;
  `windows-1252` ist ein unhandled charset → **roher** Headerwert.
- Kaputtes Q (`=ZZ`), kaputtes Base64, fehlender Terminator → roher Wert.
- `X`-Encoding wird nicht als Word erkannt und bleibt Text.
- Datum: RFC-5322-Layouts, `Date` zuerst, sonst `InternalDate`.
- Fehlertexte 1:1: `malformed initial line: …`, `malformed header line: …`.

## Fallen für den nächsten Agenten

1. Go's `json.Marshal` escapt `<`, `>`, `&` als `\u003c`/`\u003e`/`\u0026`.
   Jeder seriöse Serializer (auch `serde_json`) macht das nicht — deshalb ist
   `wire.rs` handgeschrieben. Ohne das ist jede Byte-Parität auf Header-Bytes
   unmöglich.
2. `serde_json::Value` ist ohne `preserve_order` alphabetisch sortiert;
   Feldreihenfolge lässt sich damit nicht vergleichen. Fixture-Werte als
   **JSON-Text** ablegen (`string`, nicht `RawMessage`) und Text vergleichen.
3. `Utc::now()` fehlt, weil `chrono` nur mit `std` gebaut wird →
   `SystemTime` verwenden. Go hat keinen Clock-Seam, deshalb wird im Fixture nur
   `since_set` gepinnt, nie der Zeitpunkt.
4. `chrono` parst `-0700`/`MST`/`UT` nicht wie Go's `mail.ParseDate`; die
   Layouts sind in `parse.rs` nachgebaut (2-/4-stelliges Jahr, optionale
   Sekunden, Kommentar in Klammern fällt weg).
5. `len(body) > max` in Go ist **Bytes**, nicht Zeichen; ein Schnitt mitten im
   Zeichen wird beim JSON-Encoding pro Byte zu U+FFFD — beide Seiten tun das
   jetzt identisch (`to_valid_utf8_lossy_per_byte`).
6. Kein `cmd | tail` bei Gates: der Pipe verschluckt den Exit-Code.

## Register

- **DOM-006**: TODO → **PARTIAL** (Policy + Config byte-exakt belegt; Netz-Transport offen).
- **DOM-005**: bleibt TODO, aber mit Hinweis, dass der Header-Teil jetzt gepinnt ist;
  offen sind MIME-Boundaries, CRLF und Empfänger.
- **MCP-003**: unverändert bei 14/26 — `poll_inbox` bleibt Go, solange der
  Transport fehlt. Kein Tool wird als „läuft" gezählt, das nicht pollen kann.

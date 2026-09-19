# Übergabe-Prompt (für den nächsten Agenten, kopierfertig)

Stand 2026-09-19, dritter Cut des Tages.

Du setzt die Symaira-Go→Rust-Migration für **EraseMe** fort. Arbeite
evidenzbasiert: jede Behauptung muss durch echte Ausführung belegt sein.

## Pfade

- Repo: `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-eraseme`
- Übergabe-Doku (zuerst lesen): `docs/rust-port/handoffs/2026-09-19-oauth2.md`
- Davor, als Historie: `2026-09-19-email-policy.md`,
  `docs/rust-port/handoffs/2026-09-19-mcp-store-reads.md`,
  `2026-09-18-mcp-handler-cut.md`
- Vertragsregister (SSOT): `docs/rust-port-contract-matrix.md`
- Docs-Repo-Status: `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/docs/intern/rust-cut-20260917/eraseme/STATUS.md`
- Go-Orakel: `rust-tests/parity/oracle/{mcp-tools-call,email,oauth2}/`
- Fixtures: `tests/fixtures/mcp-contract/mcp-003/cases.json` (dateibasiert),
  `tests/fixtures/mcp-contract/mcp-003-store/{seed.sql,empty-cases.json,seeded-cases.json}` (Store-Leser),
  `rust-tests/parity/oracle/email/email_cases.json` (Inbox-Policy),
  `rust-tests/parity/oracle/oauth2/oauth2_cases.json` (OAuth2)
- Arbeitsdateien/Temporäres: `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/tmp/`

## Umgebung (verpflichtend)

```bash
export CARGO_TARGET_DIR=/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/cargo-target
~/.local/bin/dev-external --status        # muss mounted:true zeigen, sonst NICHT bauen
cargo +1.98.0 …                           # Rust ist auf 1.98.0 gepinnt
GOTOOLCHAIN=go1.26.6 go …                 # Go gepinnt
```

Alle Builds, Caches und Artefakte liegen auf der NVMe. Nichts nach
`~/Downloads` oder `~/Desktop`.

## Harte Regeln

1. **Kein Go-Code löschen, kein Release, kein Production-Cutover.** Go bleibt die
   Produktionsroute; dieser Strang ist Migration.
2. **Seriell mergen**, ein Slice pro PR, Rebase auf aktuellen `main`.
3. Beide Gates vor dem Merge prüfen: Rust-PR-Gate läuft auf dem PR; die
   Native-Matrix (3 OS) läuft **nur auf `main`-Pushes** und ist damit ein
   Post-Merge-Gate. Lauf-IDs **per Workflow-Name und Head-SHA** prüfen, nicht per
   Ereignis — ein `pull_request`-Ereignis kann auch CodeQL sein.
4. **Nichts pinnen, was wandert.** Wanduhr, frisch erzeugte Row-IDs oder
   maschinenabhängige Pfade gehören nicht in einen Vertrag. Wo das Produkt die
   Eingabe-Zeilen selbst mit der Wanduhr stempelt, sind **eingefrorene Zeilen**
   (siehe `mcp-003-store/seed.sql`) das Mittel der Wahl — nicht ein Shape-Test.
   Bei relativen Zeitfenstern (z. B. `since_days`) extreme Datumswerte
   (1970/2199) verwenden, dann ist der Fall stabil und prüft trotzdem die Regel.
5. Coverage-Gate (90 % kritische Dateien) **nicht** aufweichen.
6. Bei rotem Test: erst die tatsächliche Fehlermeldung ausgeben, dann fixen.
   Nicht raten. Lokale Gates **einzeln** fahren: `cmd | tail` verschluckt den
   Exit-Code, rote Läufe bleiben dann unsichtbar.
7. Ein Tool zählt nur als „im Rust-Pfad ausgeführt", wenn es ohne Go produktiv
   arbeiten kann. Policy ohne Transport ist **PARTIAL**, nicht PASS.

## Stand

`main` = `76759b80` (#977). MCP-003: **14 von 26** Katalog-Tools verdrahtet
(lokal 350/350 Tests, clippy clean, kritische Coverage 93,80 %).

Byte-genau gegen Go's echten `ContractHandler`: `redact_file`, `validate`,
`manual_tasks_list/show/complete/cleanup`, `grant` (Dry-Run),
`generate_scheduler` (Dry-Run), `plan_show`, `list_requests`, `get_events`.

Shape-geprüft (bewusst nicht pinnbar): `plan_create`, `list_brokers`,
`schedule_install`.

**DOM-006 = PARTIAL**: Inbox-Policy vollständig portiert
(`crates/symeraseme-core/src/email/`, `crates/symeraseme-core/tests/email_parity.rs`)
und byte-exakt gegen `rust-tests/parity/oracle/email` belegt (22 Parse-, 12 Poll-,
3 Service-, 7 Config-Fälle plus Text-Helfer). Offen: Netz-Transport.

## Nächste Schritte, in dieser Reihenfolge

1. **Transport**: `internal/email/dialer.go` + `oauth2.go` (STARTTLS, SASL
   XOAUTH2, Redaction). Belegklasse DOM-007: Mock-HTTP für OAuth2, Transkript
   für IMAP. Danach `poll_inbox` im MCP-Handler verdrahten (MCP-003 → 15/26).
2. `execute` inklusive Consent-Gate-Pfad.
3. `run_web_form`, `auto_confirm`, `generate_report`, `generate_dashboard` — je
   eigener Handler-Pfad, gleiches Muster. Prüfe vor dem Pinnen, ob die Antwort
   Zeiten oder frische IDs enthält; wenn ja, eingefrorene Zeilen.
4. **Nicht implementierbar ohne Entscheidung:** `get_calendar`/
   `get_dashboard_data` (Go ruft `time.Now()` im Handler), `classify_reply`/
   `generate_rebuttal` (corekit **#288**: kein Rust-`llmkit`), `schedule_status`/
   `schedule_uninstall` (Host-Zustand/-Nebenwirkung). Diese brauchen eine
   Produktentscheidung, keinen weiteren Code.

## Muster für jeden neuen Slice

1. Go-Verhalten messen (Standardbibliothek im Zweifel im `GOROOT` lesen), nicht
   abschreiben. Orakel unter `rust-tests/parity/oracle/<paket>/` ergänzen.
2. Orakel **zweimal** laufen lassen und per `cmp` Byte-Gleichheit fordern; erst
   dann ist der Fall pinnbar. Fixture mit `--fixture` schreiben.
3. Fixture-Werte als JSON-**Text** speichern (`string`, nie `json.RawMessage`),
   sonst geht die Feldreihenfolge verloren.
4. Rust-Test spielt die Fälle zurück und vergleicht Zeichen für Zeichen.
5. **Negativkontrolle**: eine Zusicherung absichtlich brechen, Test muss rot
   werden, dann zurückbauen. Erst das beweist, dass der Test die Bytes prüft.
6. Gates lokal grün; PR; Gates; Merge; Register/Cut-Doku/STATUS nachziehen.

## Bekannte Fallen (belegt, siehe Cut-Dokumente)

- Abgebrochener CI-Lauf ≠ Fehlschlag.
- `Cargo.lock` gehört in denselben Commit wie eine Dependency-Änderung.
- JSON-Testpayloads über einen Serializer bauen (Windows-Backslash!).
- Go escapt HTML in JSON zweistufig (`\\\\u0026`) — `serde_json` tut das nicht;
  für Byte-Parität ist die Serialisierung handgeschrieben (`email/wire.rs`).
- `serde_json::Value` sortiert Keys alphabetisch; Feldreihenfolge nur über den
  Text vergleichen.
- Go löst den MCP-Workspace über das Prozess-cwd auf.
- Go-Map = sortierte Keys, Go-Struct = Feldreihenfolge; `nil`-Slice = `null`.
- Go liest `TIMESTAMP`-Spalten als `time.Time` und rendert sie als RFC 3339.
- `chrono` hat hier kein `clock`-Feature → `Utc::now()` fehlt, `SystemTime` nutzen.
- Fixtures, die `include_bytes!`/`include_str!` byte-vergleichen, brauchen
  `eol=lf` in `.gitattributes` (Windows-Checkout mit `core.autocrlf=true`).

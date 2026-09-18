# Cut 2026-09-18: MCP-Handler-Verdrahtung bis #974

Dieser Cut schließt den laufenden Strang „MCP-Tools wirklich ausführen" ab und
hinterlässt einen aufgeräumten Stand. Interne Dokumentation, nicht veröffentlichen.

## Basis und Integrationsstand

- Aufsetzpunkt: `387a81d1` (Stand vor diesem Strang), Arbeitszweig `main`.
- Integriert bis: `2cf4dd52` (#973). Alle 17 Commits dieses Strangs sind
  Squash-Merges auf `main`; die Historie ist linear, es gibt keine Merge-Commits.
- In Flight: **#974** `feat(rust): wire the schedule_install dry run`
  (Branch `codex/rust-mcp-schedule-install-20260918`, Head `f0cd81dd`, Native-Matrix
  grün, Rust-PR-Gate beim Schreiben laufend).

| PR | Commit | Slice |
|---|---|---|
| #955 | `7f7a6b01` | DOM-004b Triage-Prompts |
| #956 | `1a58d33a` | MCP-001a `initialize` |
| #957 | `57a2a9ea` | Cache-Seed (`rust-tools`) |
| #958 | `ec577231` | MCP-002 `tools/list` |
| #959 | `2f2e6985` | MCP-003 Validierung/Envelope |
| #960 | `4cdbf9be` | MCP-004/005 Envelope + Sanitisierung |
| #961 | `e0264a3d` | MCP-Stream-Framing |
| #962 | `c05d5c7f` | DOM-003 Reporting (Dashboard/Kalender/Report) |
| #964 | `a8dbad4d` | DOM-010 Manual-Task-Queue |
| #965 | `79bf23e8` | DOM-002 Campaign-Planung |
| #967 | `fb7e2537` | DOM-001 Tick-Übergänge (Scan + Apply) |
| #968 | `42614bc2` | **MCP-Handler** + `redact_file`, `validate` |
| #969 | `3c52371b` | `manual_tasks_*` (4 Tools) |
| #970 | `b749eaa5` | `grant` (Dry-Run) |
| #971 | `996d22df` | `generate_scheduler` + **geteiltes Go-HTML-Escaping** |
| #972 | `b0e8fa82` | `plan_create` |
| #973 | `2cf4dd52` | `list_brokers` |

## Architektur, die dieser Cut einführt

`crates/symeraseme-cli/src/mcp/handler.rs` enthält das `ToolHandler`-Trait und den
`ContractHandler`. Der Handler wird an der Transportgrenze injiziert und durch
`initialize` und `serve_stream` gereicht — analog zu Go's `NewServer(handler)`.

- Handler-Ergebnis → Content-Envelope (`envelope::result_response`).
- Handler-Fehler → **sanitisiertes** `-32603` (`envelope::sanitize_error`).
- Nicht verdrahtetes Katalog-Tool → eigener, ausdrücklicher Fehler.
- Name außerhalb des Katalogs (`status`, Legacy-Alias) → Go's Switch-Default
  `tool not found`.
- `ContractHandler` trägt: `workspace_root` (Go: Prozess-cwd), optionale
  Store-Config (`ConfigContext`), optionale injizierte Uhr, optionales Datenverzeichnis.

**Kein Go-Code gelöscht, kein Release, kein Cutover.** Go bleibt Produktionsroute.

## Verdrahtete MCP-Tools (11 von 26)

| Tool | Belegklasse |
|---|---|
| `redact_file` | byte-genau, 3 Fälle aus Go's echtem `ContractHandler` |
| `validate` | byte-genau, Mini-Registry im Workspace |
| `manual_tasks_list` | byte-genau (Not-found-Pfad) + Shape für `created_at`-Zeilen |
| `manual_tasks_show` | byte-genau (Not-found) + Shape für den Detailblock |
| `manual_tasks_complete` | byte-genau |
| `manual_tasks_cleanup` | byte-genau (inkl. „kein Verzeichnis") |
| `grant` | byte-genau, Dry-Run (2 Fälle) |
| `generate_scheduler` | byte-genau, Dry-Run mit festen Pfaden im Request |
| `plan_create` | Shape (Orakel nicht isolierbar) |
| `list_brokers` | Shape (Antwort = Registry, 985 KB) |
| `schedule_install` | Shape (Default-Pfade maschinenabhängig) |

Fixture: `tests/fixtures/mcp-contract/mcp-003/cases.json`, 8 Fälle, erzeugt von
`rust-tests/parity/oracle/mcp-tools-call/` (Go, `mcp.ContractHandler()`).

## Bewusst **nicht** gepinnt — mit Begründung

Wandernde Werte gehören nicht in einen Vertrag. Vier Werkzeuge liefern deshalb
keine byte-genauen Fixtures:

| Tool | Grund |
|---|---|
| `get_calendar`, `get_dashboard_data` | Der Go-Handler ruft `time.Now()` **ohne** Injektionspunkt; die Antwort trägt Wanduhr. |
| `plan_create` | Antwort enthält die gerade erzeugten Request-IDs; das Orakel bekam den Entwickler-Store (IDs ab 12351) und wuchs zwischen Läufen. |
| `list_brokers` | Antwort ist die eingebettete Registry selbst (1274 Broker / 985 KB mit `include_inactive`, 1273 sonst); ein Fixture würde sie duplizieren. Modell + Filter sind über die REG-Goldens byte-genau belegt. |
| `schedule_install` | Das Tool akzeptiert nur Plattform/Tick; die Templates betten den aufgelösten Binärpfad ein — unter `go run` je Lauf ein anderes Temp-Verzeichnis. Stabil sind die Dateinamen (Assertion). |

Zusätzlich bewusst **nicht verdrahtet**: `schedule_uninstall` (entfernt
Scheduler-Einträge des Hosts), `schedule_status` (meldet dessen Ist-Zustand).

## Noch offen

1. `poll_inbox`, `execute` — brauchen den `email`-Slice.
2. `classify_reply`, `generate_rebuttal` — **cross-repo blockiert**: Go delegiert
   alle LLM-Transports an `corekit/llmkit`, für das es kein Rust-Pendant gibt.
   Issues: corekit **#288**, eraseme **#966**.
3. `run_web_form`, `auto_confirm`, `plan_show`, `generate_report`,
   `generate_dashboard`, `get_events`, `list_requests`, `schedule_uninstall`,
   `schedule_status` — weitere Handler-Pfade.
4. Entscheidung nötig: ein Go-seitiger Uhr-Injektionspunkt würde
   `get_calendar`/`get_dashboard_data` pinbar machen. Das ist eine Produkt-/
   Vertragsfrage, keine Implementierungsfrage.

## Befehle (verifiziert in diesem Cut)

```bash
export CARGO_TARGET_DIR=/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/cargo-target
cd /Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-eraseme

~/.local/bin/dev-external --status                      # muss mounted:true zeigen
cargo +1.98.0 fmt --all --check
cargo +1.98.0 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.98.0 nextest run --workspace --all-features --locked
cargo +1.98.0 test -p symeraseme-cli --all-features --bin symeraseme-rust

# Fixture neu messen (Go-Orakel, Go 1.26.6)
GOTOOLCHAIN=go1.26.6 go run ./rust-tests/parity/oracle/mcp-tools-call --fixture

# Coverage-Gate (90 % kritische Dateien)
cargo +1.98.0 llvm-cov --workspace --all-features --json \
  --output-path /Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/tmp/cov/rust-coverage.json
find . -name '*.profraw' -not -path './.git/*' -delete
```

Stand dieses Cuts: **348/348** Tests, clippy clean, kritische Coverage
**94,21 %** (2296/2437), Gate 90 %.

## Fallen, die dieser Strang belegt hat

- Ein **abgebrochener** CI-Lauf trägt dieselbe rote Markierung wie ein
  Fehlschlag. Vor jeder Diagnose den Lauf öffnen.
- `Cargo.lock` gehört in denselben Commit wie eine Dependency-Änderung, sonst
  bricht jedes `--locked`-Kommando.
- JSON-Testpayloads **immer** über einen Serializer bauen: ein Windows-Pfad im
  `format!`-String ist ungültiges JSON (Backslash = Escape).
- Go escapt HTML in JSON **zweistufig** (`json.Marshal` innen, Encoder außen);
  `&` erreicht den Draht als `\\u0026`.
- Orakel-Requests dürfen **keine** maschinenspezifischen Pfade enthalten; Go löst
  den MCP-Workspace über das Prozess-cwd auf.
- Wenn ein Test rot ist: erst die tatsächliche Nachricht ausgeben, dann fixen.
  Dreimal auf dasselbe Symptom zu raten kostet mehr als eine Messung.
# Übergabe-Prompt (für den nächsten Agenten, kopierfertig)

Du setzt die Symaira-Go→Rust-Migration für **EraseMe** fort. Arbeite
evidenzbasiert: jede Behauptung muss durch echte Ausführung belegt sein.

## Pfade

- Repo: `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-eraseme`
- Übergabe-Doku (zuerst lesen): `docs/rust-port/handoffs/2026-09-18-mcp-handler-cut.md`
- Vertragsregister (SSOT): `docs/rust-port-contract-matrix.md`
- Docs-Repo (Korrekturen vom 18.09.): `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/docs/intern/rust-cut-20260917/eraseme/STATUS.md`
- Go-Orakel für MCP: `rust-tests/parity/oracle/mcp-tools-call/`
- Fixture: `tests/fixtures/mcp-contract/mcp-003/cases.json`
- Gesicherte Fremd-WIP (nur lesen): `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/tmp/wip-preserved/`
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
3. Jeder PR braucht **beide** Gates: Rust-PR-Gate **und** Native-Matrix (3 OS).
   Lauf-IDs **per Workflow-Name und Head-SHA** prüfen, nicht per Ereignis — ein
   `pull_request`-Ereignis kann auch CodeQL sein.
4. **Nichts pinnen, was wandert.** Wenn Go eine Wanduhr, eine gerade erzeugte
   Row-ID oder einen maschinenabhängigen Pfad in die Antwort schreibt, dann
   Shape-Assertion statt Fixture — mit Begründung im Orakel.
5. Coverage-Gate (90 % kritische Dateien) **nicht** aufweichen; ungedeckte Pfade
   abdecken.
6. Bei rotem Test: erst die tatsächliche Fehlermeldung ausgeben, dann fixen.
   Nicht raten.

## Stand

`main` = `3c04c927` (nach #974). MCP-003: **11 von 26** Katalog-Tools verdrahtet.
Lokal: 348/348 Tests, clippy clean, kritische Coverage 94,21 %.

Byte-genau gegen Go's echten `ContractHandler`: `redact_file`, `validate`,
`manual_tasks_list/show/complete/cleanup`, `grant` (Dry-Run),
`generate_scheduler` (Dry-Run).
Shape-geprüft (bewusst nicht pinnbar): `plan_create`, `list_brokers`,
`schedule_install`.

## Nächste Schritte, in dieser Reihenfolge

1. **#975 prüfen und mergen** (Doku-PR, falls noch offen).
2. `get_events`, `list_requests`, `plan_show` verdrahten — Store-basiert, aber
   **ohne** Wanduhr im Handler: vermutlich byte-genau pinbar. Prüfe vor dem
   Pinnen, ob die Antwort Zeiten oder frische IDs enthält.
3. `email`-Slice (`internal/email`) portieren, dann `poll_inbox` und `execute`
   verdrahten.
4. `run_web_form`, `auto_confirm`, `generate_report`, `generate_dashboard` — je
   eigener Handler-Pfad, gleiches Muster.
5. **Nicht implementierbar ohne Entscheidung:** `get_calendar`/
   `get_dashboard_data` (Go ruft `time.Now()` im Handler), `classify_reply`/
   `generate_rebuttal` (corekit **#288**: kein Rust-`llmkit`), `schedule_status`/
   `schedule_uninstall` (Host-Zustand/-Nebenwirkung). Diese brauchen eine
   Produktentscheidung, keinen weiteren Code.

## Muster für jeden neuen Tool-Slice

1. Go-Handler-Fall lesen (`internal/mcp/contract_handler.go`) und prüfen, ob die
   Antwort deterministisch ist (Wanduhr? Store-IDs? Pfade?).
2. Orakel-Fall ergänzen (`rust-tests/parity/oracle/mcp-tools-call/main.go`),
   **zweimal** laufen lassen und die Ausgaben byte-vergleichen — erst dann gilt
   er als pinnbar.
3. Fixture erzeugen (`--fixture`), Handler in
   `crates/symeraseme-cli/src/mcp/handler.rs` verdrahten.
4. Fixture-Test **oder** Shape-Test schreiben; Gates lokal grün; PR; beide CI-
   Gates; Merge.

## Bekannte Fallen (belegt, siehe Handoff-Doku)

- Abgebrochener CI-Lauf ≠ Fehlschlag.
- `Cargo.lock` gehört in denselben Commit wie eine Dependency-Änderung.
- JSON-Testpayloads über einen Serializer bauen (Windows-Backslash!).
- Go escapt HTML in JSON zweistufig (`\\u0026`).
- Orakel-Requests dürfen keine maschinenspezifischen Pfade enthalten.
- Go löst den MCP-Workspace über das Prozess-cwd auf.

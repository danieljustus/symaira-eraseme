# Übergabe-Prompt (für den nächsten Agenten, kopierfertig)

Du setzt die Symaira-Go→Rust-Migration für **EraseMe** fort. Arbeite
evidenzbasiert: jede Behauptung muss durch echte Ausführung belegt sein.

## Pfade

- Repo: `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-eraseme`
- Übergabe-Doku (zuerst lesen): `docs/rust-port/handoffs/2026-09-19-mcp-store-reads.md`
- Davor, als Historie: `docs/rust-port/handoffs/2026-09-18-mcp-handler-cut.md`
- Vertragsregister (SSOT): `docs/rust-port-contract-matrix.md`
- Docs-Repo: `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/docs/intern/rust-cut-20260917/eraseme/STATUS.md`
- Go-Orakel für MCP: `rust-tests/parity/oracle/mcp-tools-call/`
- Fixtures: `tests/fixtures/mcp-contract/mcp-003/cases.json` (dateibasiert),
  `tests/fixtures/mcp-contract/mcp-003-store/{seed.sql,empty-cases.json,seeded-cases.json}` (Store-Leser)
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
3. Beide Gates vor dem Merge prüfen: Rust-PR-Gate läuft auf dem PR; die
   Native-Matrix (3 OS) läuft **nur auf `main`-Pushes** und ist damit ein
   Post-Merge-Gate. Lauf-IDs **per Workflow-Name und Head-SHA** prüfen, nicht per
   Ereignis — ein `pull_request`-Ereignis kann auch CodeQL sein.
4. **Nichts pinnen, was wandert.** Wanduhr, frisch erzeugte Row-IDs oder
   maschinenabhängige Pfade gehören nicht in einen Vertrag. Wo das Produkt die
   Eingabe-Zeilen selbst mit der Wanduhr stempelt, sind **eingefrorene Zeilen**
   (siehe `mcp-003-store/seed.sql`) das Mittel der Wahl — nicht ein Shape-Test.
5. Coverage-Gate (90 % kritische Dateien) **nicht** aufweichen.
6. Bei rotem Test: erst die tatsächliche Fehlermeldung ausgeben, dann fixen.
   Nicht raten. Lokale Gates **einzeln** fahren: `cmd | tail` verschluckt den
   Exit-Code, rote Läufe bleiben dann unsichtbar.

## Stand

`main` = `a01c5cc5` (nach #976). MCP-003: **14 von 26** Katalog-Tools verdrahtet.
Lokal: 350/350 Tests, clippy clean, kritische Coverage 93,80 %.

Byte-genau gegen Go's echten `ContractHandler`: `redact_file`, `validate`,
`manual_tasks_list/show/complete/cleanup`, `grant` (Dry-Run),
`generate_scheduler` (Dry-Run), `plan_show`, `list_requests`, `get_events`.

Shape-geprüft (bewusst nicht pinnbar): `plan_create`, `list_brokers`,
`schedule_install`.

## Nächste Schritte, in dieser Reihenfolge

1. `email`-Slice (`internal/email`) portieren, dann `poll_inbox` und `execute`
   verdrahten (der Consent-Gate-Pfad für `execute` gehört dazu).
2. `run_web_form`, `auto_confirm`, `generate_report`, `generate_dashboard` — je
   eigener Handler-Pfad, gleiches Muster. Prüfe vor dem Pinnen, ob die Antwort
   Zeiten oder frische IDs enthält; wenn ja, eingefrorene Zeilen wie in diesem Cut.
3. **Nicht implementierbar ohne Entscheidung:** `get_calendar`/
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
4. Fixture-Test **oder** Shape-Test schreiben; Gates lokal grün; PR; Gates; Merge.
5. Doku nachziehen: Registerzeile, Cut-Doku, `STATUS.md` im Docs-Repo.

## Bekannte Fallen (belegt, siehe Cut-Dokumente)

- Abgebrochener CI-Lauf ≠ Fehlschlag.
- `Cargo.lock` gehört in denselben Commit wie eine Dependency-Änderung.
- JSON-Testpayloads über einen Serializer bauen (Windows-Backslash!).
- Go escapt HTML in JSON zweistufig (`\\u0026`).
- Orakel-Requests dürfen keine maschinenspezifischen Pfade enthalten.
- Go löst den MCP-Workspace über das Prozess-cwd auf.
- Go-Map = sortierte Keys, Go-Struct = Feldreihenfolge; `nil`-Slice = `null`.
- Go liest `TIMESTAMP`-Spalten als `time.Time` und rendert sie als RFC 3339.
- Fixtures, die `include_bytes!`/`include_str!` byte-vergleichen, brauchen
  `eol=lf` in `.gitattributes` (Windows-Checkout mit `core.autocrlf=true`).
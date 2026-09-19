# Cut 2026-09-19: Store-gelesene MCP-Tools bis #976

Dieser Cut verlängert den Strang „MCP-Tools wirklich ausführen" um die drei
Store-Leser. Interne Dokumentation, nicht veröffentlichen.

## Basis und Integrationsstand

- Aufsetzpunkt: `a4c9a7e0` (Stand nach #975/#974), Arbeitszweig `main`.
- Integriert bis: `a01c5cc5` (#976). Squash-Merge, Historie bleibt linear.
- Keine offenen Strang-PRs; #975 (Doku des vorherigen Cuts) ist enthalten.

| PR | Commit | Slice |
|---|---|---|
| #976 | `a01c5cc5` | `plan_show`, `list_requests`, `get_events` + eingefrorene Store-Zeilen |

## Was dieser Cut verdrahtet (14 von 26)

`plan_show`, `list_requests` und `get_events` lesen den Event-Store. Damit sind
14 der 26 Katalog-Tools verdrahtet; 11 davon byte-genau, 3 per Shape.

Byte-genau: `redact_file`, `validate`, `manual_tasks_list/show/complete/cleanup`,
`grant` (Dry-Run), `generate_scheduler` (Dry-Run), **`plan_show`,
`list_requests`, `get_events`**.
Shape: `plan_create`, `list_brokers`, `schedule_install`.

## Der Kern dieses Cuts: eingefrorene Zeilen

Der Go-Schreibpfad stempelt `created_at`/`recorded_at` über `time.Now()` und
bietet **keinen** Injektionspunkt. Ein Store, den das Produkt geschrieben hat,
trägt deshalb eine Wanduhr — seine Antwort ist nicht pinnbar. Dieser Cut zieht
die Konsequenz nicht als Shape-Test, sondern friert die Zeilen ein:

- `tests/fixtures/mcp-contract/mcp-003-store/seed.sql` — die Eingabe-Zeilen,
  eine Anweisung pro Zeile.
- `mcp-003-store/empty-cases.json` — 7 Fälle ohne Zeilen: leerer Store,
  `page`/`page_size`-Ablehnungen, unbekannte Request-ID.
- `mcp-003-store/seeded-cases.json` — 13 Fälle über `seed.sql`.

Das Orakel legt dieselbe Datei über einen Store an, den der Handler selbst
erzeugt hat (Schema), und die Rust-Tests wenden sie erneut an. Beide Seiten
lesen damit identische Zeilen; verglichen werden die Bytes der **Lesepfades**.
Die Zeilen sind Eingabe im selben Sinn wie die Registry-YAML der
`validate`-Fälle. Die Schreibpfade haben eigene Verträge (Event-Store-Goldens).

Abgedeckt: Campaign-/Status-/Broker-Filter, Pagination (Offset+Limit), Go's
Zähl-Asymmetrie (der Broker-Filter verengt die Zeilen, **nicht** `total`),
`(occurred_at, id)`-Reihenfolge der Events **mit Gleichstand**, ein Event-Typ
außerhalb des Katalogs, `null` statt `[]` für leere Ergebnisse.

Orakel-Isolation: eigenes `SYMERASEME_DATA_DIR`, `SYMERASEME_DB_DIR` und
`XDG_CONFIG_HOME` je Fixture-Familie. Genau diese Isolation hatte beim früheren
`plan_create`-Versuch gefehlt; die IDs kamen damals aus dem Entwickler-Store.

## Zwei Verdrahtungsdetails, die die Bytes aufgedeckt haben

1. **TIMESTAMP-Spalten erreichen den Draht als RFC 3339.** `database/sql` reicht
   dem Scan das `time.Time` des Treibers und rendert es als RFC 3339: aus dem
   gespeicherten `2026-08-06 10:00:00` wird `2026-08-06T10:00:00Z`. Die rohe
   Spaltentext-Fassung der Rust-Row-Typen wäre nicht deckungsgleich.
2. **`[]Event` wird als Struct marshallisiert.** Ein Event behält Go's
   Feldreihenfolge (`ID`, `RequestID`, `OccurredAt`, …), nicht die
   alphabetische einer JSON-Map. Die Liste wird daher als JSON-Text gebaut und
   dem Envelope als String-Ergebnis übergeben — Go's `contentEnvelope` reicht
   Strings ebenfalls wörtlich durch.

## Befehle (verifiziert in diesem Cut)

```bash
export CARGO_TARGET_DIR=/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/cargo-target
cd /Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-eraseme

~/.local/bin/dev-external --status                      # muss mounted:true zeigen
cargo +1.98.0 fmt --all --check
cargo +1.98.0 clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo +1.98.0 nextest run --workspace --all-features --locked
cargo +1.98.0 test -p symeraseme-cli --all-features --bin symeraseme-rust -- store_read

# Fixture neu messen (Go-Orakel, Go 1.26.6). Schreibt beide Store-Fixtures.
GOTOOLCHAIN=go1.26.6 go run ./rust-tests/parity/oracle/mcp-tools-call --fixture

# Coverage-Gate (90 % kritische Dateien)
cargo +1.98.0 llvm-cov --workspace --all-features --json \
  --output-path /Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/tmp/cov/rust-coverage.json
find . -name '*.profraw' -not -path './.git/*' -delete
```

Stand dieses Cuts: **350/350** Tests, clippy clean, kritische Coverage
**93,80 %** (2526/2693), Gate 90 %. `mcp/handler.rs` bei 91,46 %.

## Fallen, die dieser Cut belegt hat

- **Ein `| tail` verschluckt den Exit-Code.** `clippy … | tail -15 && nextest …`
  läuft bei rotem Clippy weiter, und die Fehlerzeilen stehen nicht im
  Ausgabeschwanz. Der PR-Gate-Lauf war der erste, der das meldete
  (`clippy::needless_borrow` in einem neuen Test-Helfer). Lokale Gates einzeln
  und mit sichtbarem Exit-Code fahren.
- **`cargo fmt --all` ohne `--check` schreibt.** Für den Gate-Beleg `--check`
  verwenden, sonst ist der Beweis nur ein Schreibvorgang.
- **Ein Go-Struct und eine Go-Map sind auf dem Draht nicht dasselbe.** Map-Keys
  sortiert Go alphabetisch, Struct-Felder behält es in Deklarationsreihenfolge.
  `serde_json::Value` (BTreeMap) verliert die zweite Information.
- **`null` ≠ `[]`.** Go antwortet auf „nichts gefunden“ mit einem nil-Slice →
  `null`. Eine leere Rust-`Vec` wäre `[]`.
- **Der Go-SQLite-Treiber liest `TIMESTAMP`-Spalten als `time.Time`.** Wer die
  Rohspalte durchreicht, weicht ab, ohne dass es auffällt.
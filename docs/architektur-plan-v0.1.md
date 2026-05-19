# OpenEraseMe — Architekturplan v0.1

## Context

**Problem:** Datenbroker sammeln und verkaufen personenbezogene Daten. DSGVO/CCPA gewähren Löschrechte, aber praktisch undurchsetzbar (hunderte Broker, jeder mit eigenem Prozess). Kommerzielle Dienste wie Incogni schließen die Lücke, schaffen aber selbst einen Daten-Honeypot und scheitern oft an Vollmachts-Hürden.

**Lösung:** OpenEraseMe stellt **Tools, Skills und eine kuratierte Datenbank** bereit, mit denen **bereits vorhandene AI-Agents** des Nutzers (Claude Code, OpenClaw, Hermes, Cursor, eigene MCP-Clients …) Lösch­anfragen vollautomatisch versenden, Antworten triagieren und Status verwalten. Da der Versand über das **lokale Postfach des Nutzers** läuft, gilt jede Anfrage juristisch als **Direct Subject Request** — keine Vollmacht, kein Honeypot, kein Drittanbieter.

**Outcome v0.1:** Ein Python-Paket mit **CLI als einzigem Tool-Interface** (`--output json` überall, damit jeder AI-Agent sie per Shell-Aufruf konsumieren kann) + Skill-Bundle, mit dem DSGVO/CCPA-Löschungen bei 30–50 priorisierten Brokern durchgeführt werden — inkl. E-Mail-Versand, Web-Form-Automatisierung (mit CAPTCHA-Lösung), Inbox-Triage, **vollständiges Lifecycle-Tracking jeder Anfrage** (Sent → Ack → Verification → Confirmed/Rejected/Escalated) und proaktives No-Response-/Deadline-Handling.

**Was OpenEraseMe explizit *nicht* ist:** Kein eigener Agent, kein gehosteter SaaS (zunächst), kein MCP-Server, keine eigene LLM-Orchestrierung. Die Intelligenz lebt im AI-Agent des Nutzers; OpenEraseMe liefert nur die Werkzeuge — und zwar ausschließlich als CLI mit strukturiertem JSON-Output.

---

## Architektur in einem Bild

```
┌────────────────────────────────────────────────────────────────────┐
│  User's AI Agent (Claude Code, OpenClaw, Hermes, Cursor, …)        │
│  └── konsumiert Skills (.md) + ruft CLI via Shell auf              │
└──────────────┬─────────────────────────────────────────────────────┘
               │ shell: `openeraseme <cmd> --output json`
               ▼
┌────────────────────────────────────────────────────────────────────┐
│  OpenEraseMe (Python package: `openeraseme`) — CLI-only            │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │ CLI (typer)            │  Skill Bundle (.md)                 │  │
│  ├──────────────────────────────────────────────────────────────┤  │
│  │ Orchestration: plan · execute · poll-inbox · tick (deadlines)│  │
│  ├──────────────────────────────────────────────────────────────┤  │
│  │ Adapters:                                                    │  │
│  │   • email_adapter  (Himalaya CLI / aiosmtplib + IMAPClient)  │  │
│  │   • web_adapter    (Playwright + CapSolver/2Captcha)         │  │
│  │   • postal_adapter (PDF-Generator → Mail-Anhang oder Print)  │  │
│  │   • triage         (LLM-Calls via Anthropic SDK)             │  │
│  ├──────────────────────────────────────────────────────────────┤  │
│  │ Templating · Identity Vault (Keyring) · Event-Sourced DB     │  │
│  ├──────────────────────────────────────────────────────────────┤  │
│  │ Registry (Broker-YAMLs + Rechts-Templates, im selben Repo)   │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────┬──────────────────────────────────────────┘
                          ▼
              ┌──────────────────────────────┐
              │ Lokales Postfach (IMAP/SMTP) │
              │ Gmail / Outlook / Custom     │
              │ via OAuth2 (XOAUTH2)         │
              └──────────────────────────────┘
```

---

## Repository-Struktur (ein Monorepo)

Alles in **einem** GitHub-Repo (`OpenEraseMe/openeraseme`, dieses Repo). Registry, Templates, Code, Skills, Docs liegen zusammen — einfacher zu kontribuieren (eine PR ändert ggf. Code + Beispiel + Broker-Eintrag atomar), einfacher zu releasen, weniger Versions-Drift zwischen Schema und Daten.

Damit Broker-Daten trotzdem unabhängig vom Code-Release aktuell bleiben können, holt `openeraseme registry sync` die Registry per `git pull --ff-only` (oder bei pip-Install per Re-Download), ohne dass das ganze Paket neu installiert werden muss.

```
openeraseme/
├── pyproject.toml              # uv/hatch, Python 3.11+
├── README.md
├── CONTRIBUTING.md             # Broker-Onboarding-Guide + Code-Contrib-Guide
├── docs/                       # bereits vorhanden, bleibt (Recherche + Architektur)
├── src/openeraseme/
│   ├── __init__.py
│   ├── cli.py                  # Typer-App, einziger Entrypoint
│   ├── core/
│   │   ├── orchestrator.py     # plan / execute / poll-inbox / tick
│   │   ├── identity.py         # Identity Profile (read/write via Keyring)
│   │   ├── templating.py       # Jinja2 + Skill-Files
│   │   ├── events.py           # Append-only Event-Store
│   │   ├── projection.py       # baut request_state aus Events
│   │   ├── deadlines.py        # No-Response-Handling, Eskalations-Trigger
│   │   ├── consent.py          # Grant-Token-Mechanik
│   │   └── db.py               # SQLite (aiosqlite) Connection-Mgmt
│   ├── adapters/
│   │   ├── email/
│   │   │   ├── himalaya.py     # primärer Backend (subprocess → JSON out)
│   │   │   ├── smtp_imap.py    # Fallback (aiosmtplib + aioimaplib)
│   │   │   └── oauth2.py       # XOAUTH2-Flow (Gmail, Outlook)
│   │   ├── web/
│   │   │   ├── playwright_runner.py
│   │   │   ├── captcha.py      # CapSolver/2Captcha-Wrapper
│   │   │   └── form_dsl.py     # interpretiert broker.opt_out.web spec
│   │   └── triage/
│   │       ├── classifier.py   # LLM-Call: Reply → Event-Typ
│   │       └── responder.py    # LLM generiert Rebuttal/Confirmation
│   ├── registry/
│   │   ├── loader.py           # lädt + validiert Registry-Dateien
│   │   ├── sync.py             # `registry sync` Implementierung
│   │   └── schema.py           # pydantic models
│   └── llm/
│       └── anthropic_client.py # mit Prompt-Caching
├── registry/                   # ← Daten, im selben Repo
│   ├── brokers/
│   │   ├── eu/
│   │   │   ├── _example.yaml   # vollständig dokumentiert
│   │   │   ├── acxiom.yaml
│   │   │   └── …
│   │   ├── us/
│   │   │   ├── beenverified.yaml
│   │   │   ├── spokeo.yaml
│   │   │   └── …
│   │   └── global/
│   ├── laws/
│   │   ├── gdpr-art17.de.md.j2 # Jinja2-Templates pro Sprache
│   │   ├── gdpr-art17.en.md.j2
│   │   ├── ccpa-deletion.en.md.j2
│   │   └── ccpa-opt-out.en.md.j2
│   ├── locales/
│   │   └── (string-bundles)
│   └── schemas/
│       └── broker.schema.json  # JSON Schema 2020-12, single source of truth
├── skills/
│   ├── SKILL.md                # Top-level: "using-openeraseme"
│   ├── setup-identity.md
│   ├── plan-removal-campaign.md
│   ├── send-removal-batch.md
│   ├── triage-broker-replies.md
│   ├── handle-action-required.md
│   ├── daily-tick.md
│   └── re-scan-quarterly.md
├── tests/
│   ├── unit/
│   ├── integration/            # gegen Mailpit (lokaler SMTP/IMAP)
│   ├── registry/               # validiert alle YAMLs gegen Schema
│   └── fixtures/
│       └── broker_replies/     # anonymisierte Antwort-Samples
├── examples/
│   ├── claude-code/            # Beispiel-Setup für Claude Code
│   ├── openclaw/
│   └── plain-cron/             # ohne Agent, nur CLI + cron
└── .github/
    ├── workflows/
    │   ├── ci.yml              # lint + tests + schema-validate
    │   └── registry-link-check.yml  # weekly: 404-Check auf Broker-URLs
    └── PULL_REQUEST_TEMPLATE.md
```

---

## Datenmodelle

### Broker (YAML, validiert gegen JSON-Schema)

```yaml
# brokers/eu/example.yaml
id: example-broker-eu
name: Example Data Broker GmbH
website: https://example-broker.com
category: people-search       # people-search | marketing | credit | analytics
jurisdictions: [DE, AT, EU]
laws: [GDPR]
data_sensitivity: 4           # 1–5 (Skala wie Incogni)
priority: high                # high | medium | low (Erstanlauf-Priorität)

opt_out:
  - type: email
    endpoint: privacy@example-broker.com
    template: gdpr-art17       # referenziert laws/gdpr-art17.*.md.j2
    locale: de
    required_fields: [full_name, address, email]
    supports_suppression: true
    expected_response_days: 30
  - type: web_form              # alternativer Kanal
    url: https://example-broker.com/privacy/delete
    form_spec:                  # deklarative DSL für Playwright
      steps:
        - fill: { selector: "#name", from: full_name }
        - fill: { selector: "#email", from: email }
        - solve_captcha: { type: recaptcha-v2, sitekey: "6Lc..." }
        - click: "#submit"
        - wait_for: ".confirmation"

verification:
  ack_keywords: ["received", "wir bestätigen", "request id"]
  rejection_keywords: ["cannot verify", "additional information required"]
  human_required_keywords: ["upload id", "passport", "ausweis"]

notes: |
  Verlangt manchmal alte Adresse — siehe rebuttal-template `gdpr-rebuttal-address.md`.
```

### Identity Profile (lokal verschlüsselt)

```python
# pydantic model
class IdentityProfile(BaseModel):
    full_name: str
    name_variants: list[str]        # Heiratsname, Vor-Schreibvarianten
    date_of_birth: date | None
    addresses: list[Address]        # mit valid_from / valid_to
    email_addresses: list[EmailStr]
    phone_numbers: list[str]
    jurisdictions: list[str]        # ["DE", "EU"] etc.
```

Speicherung: serialisiert als JSON, AES-verschlüsselt, Master-Key im OS-Keyring. Keine Klartext-Datei je.

### Status-DB (SQLite, Event-Sourced)

Eine reine `sent_at + replies`-Tabelle wäre eine Buchhaltung für einen Massenmailer. Damit OpenEraseMe eine **vollwertige Lifecycle-Lösung** ist, ist die DB **event-sourced**: ein immutabler Event-Log pro Anfrage ist die Wahrheit; eine projizierte `request_state`-Tabelle ist der schnell abfragbare aktuelle Stand. Jede Aktion (Versand, Ack, Verifizierungsaufforderung, Bestätigungs-Klick, Rebuttal, Beschwerde an Aufsichtsbehörde, Timeout, …) ist ein Event mit Zeitstempel.

```sql
-- Eine Zeile pro geplanter/laufender Löschanfrage
CREATE TABLE removal_requests (
  id              INTEGER PRIMARY KEY,
  broker_id       TEXT NOT NULL,        -- → registry/brokers/*.yaml
  channel         TEXT NOT NULL,        -- email | web_form | postal
  campaign_id     TEXT NOT NULL,        -- Gruppierung für Plan/Batch
  created_at      TIMESTAMP NOT NULL,
  jurisdiction    TEXT NOT NULL,        -- z.B. "GDPR-DE"
  template_id     TEXT NOT NULL,        -- welches Template benutzt wurde
  identity_snapshot_hash TEXT NOT NULL  -- Hash der zum Zeitpunkt benutzten Identitätsdaten
);

-- Append-only Event-Log: Quelle der Wahrheit
CREATE TABLE request_events (
  id              INTEGER PRIMARY KEY,
  request_id      INTEGER NOT NULL REFERENCES removal_requests(id),
  occurred_at     TIMESTAMP NOT NULL,
  recorded_at     TIMESTAMP NOT NULL,    -- wann WIR es gemerkt haben (für Inbox-Polling)
  event_type      TEXT NOT NULL,         -- siehe Liste unten
  payload_json    TEXT NOT NULL,         -- typ-spezifische Details
  source          TEXT NOT NULL          -- system | inbox | user | scheduler
);
CREATE INDEX idx_events_request ON request_events(request_id, occurred_at);

-- Projektion: aktueller Stand, aus Events berechnet (rebuildbar)
CREATE TABLE request_state (
  request_id      INTEGER PRIMARY KEY REFERENCES removal_requests(id),
  current_status  TEXT NOT NULL,        -- s. Status-Maschine unten
  last_event_id   INTEGER NOT NULL REFERENCES request_events(id),
  last_event_at   TIMESTAMP NOT NULL,
  sent_at         TIMESTAMP,            -- erstes SENT-Event
  acknowledged_at TIMESTAMP,            -- erstes ACK-Event
  resolved_at     TIMESTAMP,            -- CONFIRMED / REJECTED_FINAL
  deadline_at     TIMESTAMP,            -- gesetzliche Frist (GDPR: sent_at + 30d)
  next_action_at  TIMESTAMP,            -- wann der nächste Tick aktiv werden soll
  reminders_sent  INTEGER NOT NULL DEFAULT 0,
  escalation_level INTEGER NOT NULL DEFAULT 0  -- 0=keine, 1=Reminder, 2=DPA-Beschwerde vorbereitet
);

-- Eingegangene Mails (rohe Verweise, vollständige .eml im Mail-Account)
CREATE TABLE inbox_replies (
  id              INTEGER PRIMARY KEY,
  request_id      INTEGER REFERENCES removal_requests(id),  -- nullable: noch nicht zugeordnet
  message_id      TEXT UNIQUE NOT NULL,                     -- IMAP Message-ID
  thread_id       TEXT,                                     -- References-Kette
  received_at     TIMESTAMP NOT NULL,
  from_addr       TEXT,
  subject         TEXT,
  snippet         TEXT,                                     -- erste 500 Zeichen redacted
  classified_as   TEXT,                                     -- via triage: ack|verification|confirmed|rejected|human_required|noise
  classifier_confidence REAL,
  llm_summary     TEXT
);

-- Kampagnen (für Group-Reports & Re-Scan-Logik)
CREATE TABLE campaigns (
  id              TEXT PRIMARY KEY,    -- z.B. "initial-2026-Q2"
  created_at      TIMESTAMP NOT NULL,
  kind            TEXT NOT NULL,       -- initial | re-scan | targeted
  notes           TEXT
);
```

#### Event-Typen (vollständige Liste)

| event_type | Bedeutung | Trigger |
|------------|-----------|---------|
| `PLANNED` | Request angelegt, noch nicht gesendet | `openeraseme plan` |
| `SENT` | E-Mail/Form-Submission raus | `execute` |
| `SEND_FAILED` | Transport-Fehler (SMTP-Reject, Form-Timeout) | adapter |
| `BOUNCE` | Hard-Bounce vom MTA empfangen | inbox-poll |
| `AUTORESPONDER` | Out-of-Office o.ä., ignorieren | inbox-poll (triage) |
| `ACK` | Broker bestätigt Erhalt | inbox-poll (triage) |
| `VERIFICATION_REQUESTED` | Broker will Ausweis/alte Adresse/Captcha | inbox-poll |
| `VERIFICATION_PROVIDED` | Wir haben geantwortet | execute |
| `HUMAN_ACTION_REQUIRED` | Klassifizierer ist unsicher / verlangt User-Entscheidung | inbox-poll |
| `CONFIRMATION_LINK_CLICKED` | Auto-Klick auf Bestätigungslink | web-adapter |
| `REBUTTAL_SENT` | Gegenargumentation auf Ablehnung | execute |
| `REMINDER_SENT` | Erinnerung nach X Tagen Stille | `tick` |
| `DEADLINE_REACHED` | Gesetzliche Frist abgelaufen ohne Antwort | `tick` |
| `DPA_COMPLAINT_DRAFTED` | Entwurf für Beschwerde an Aufsichtsbehörde liegt vor | `tick` |
| `DPA_COMPLAINT_FILED` | User hat Beschwerde abgesendet | user |
| `CONFIRMED` | Löschung schriftlich bestätigt | inbox-poll oder user |
| `REJECTED_FINAL` | Endgültige, nicht anfechtbare Ablehnung | inbox-poll oder user |
| `RE_SCAN_TRIGGERED` | Quartals-Wiedervorlage gestartet | `tick` |
| `NOTE_ADDED` | Manuelle Notiz | user |

#### Status-Maschine (current_status in request_state)

```
PLANNED → SENT → {AWAITING_ACK, ACK} → {AWAITING_RESPONSE, AWAITING_USER_ACTION}
                                      → {CONFIRMED, REJECTED_FINAL,
                                         OVERDUE → ESCALATED → DPA_FILED}
                                      → RE_SCAN_DUE (nach 90d, neuer Request)
SEND_FAILED → RETRY_SCHEDULED → (zurück nach PLANNED) | DEAD
```

### No-Response-/Deadline-Handling

Ohne aktives Nachfass-Verhalten wäre das Tool nur ein Sender. Daher: **`openeraseme tick`** ist ein eigenständiger Command (gedacht für Cron/launchd, täglich), der:

1. Alle `request_state` Zeilen mit `next_action_at <= now` lädt.
2. Pro Zeile entscheidet — Beispiele:
   - Status `AWAITING_ACK`, sent_at > 7 Tage → `REMINDER_SENT` Event + Reminder-Mail.
   - Status `AWAITING_RESPONSE`, sent_at > deadline_at (GDPR: +30d) → `DEADLINE_REACHED` Event, Status → `OVERDUE`, Eskalations-Level 1.
   - `OVERDUE` seit 14 Tagen → `DPA_COMPLAINT_DRAFTED` Event, Beschwerde-PDF an Aufsichtsbehörde (z. B. BfDI, ICO) generieren, User benachrichtigen.
   - Status `CONFIRMED`, resolved_at > 90 Tage → `RE_SCAN_TRIGGERED` Event, neuer Request für denselben Broker (gegen Re-Scraping).
3. Generiert einen strukturierten Output (`--output json`), den ein AI-Agent direkt konsumieren kann: "Was muss heute passieren?".

Damit ist OpenEraseMe explizit kein Massenmailer, sondern ein **Lifecycle-Manager**: jede Anfrage hat eine vollständige Historie, bekannte Fristen, und das System weiß proaktiv, wann es selbst aktiv werden muss.

---

## CLI-Spezifikation (Hauptintegrationspfad für AI-Agents)

**Entscheidung:** CLI statt MCP-Server als v0.1-Hauptinterface. Begründung:
- Jeder AI-Agent kann Shell-Befehle ausführen (kein MCP-Support nötig).
- `--output json` macht jeden Befehl maschinenlesbar — das ist die "Tool-Spec" implizit.
- Kein langlebiger Server-Prozess; jede Invocation ist atomar und einfacher zu auditieren.
- Einfacher zu testen (`subprocess.run` in Tests), einfacher zu installieren, einfacher zu sandboxen.
- Wenn später ein MCP-Wrapper gewünscht ist (für Agents, die MCP bevorzugen): triviale dünne Schicht, die die CLI-Befehle als MCP-Tools anbietet.

Jeder Befehl unterstützt `--output {text|json}`. JSON-Output ist stabil versioniert (`schema_version` im Top-Level).

| Befehl | Beispiel | Zweck |
|--------|----------|-------|
| `openeraseme init` | `init --identity-from-prompt` | Identity-Vault anlegen, OS-Keyring-Setup |
| `openeraseme accounts add` | `accounts add --provider gmail` | OAuth2-Flow, Mail-Account einbinden |
| `openeraseme brokers list` | `brokers list --jurisdiction GDPR --priority high --output json` | Registry filtern |
| `openeraseme brokers show` | `brokers show acxiom` | Broker-Details |
| `openeraseme registry sync` | `registry sync` | Holt aktuelle Registry-Version (git pull / pip update) |
| `openeraseme plan` | `plan --jurisdiction GDPR --max 30 --campaign initial-q2` | Plant Kampagne, schreibt `PLANNED`-Events, sendet noch nicht |
| `openeraseme plan show` | `plan show --campaign initial-q2 --output json` | Zeigt aktuellen Plan zur Review |
| `openeraseme execute` | `execute --campaign initial-q2 --batch-size 15` | Sendet Drip-Tranche, fügt `SENT`-Events hinzu |
| `openeraseme execute` | `execute --dry-run --request-id 42` | Rendert nur, sendet nicht |
| `openeraseme poll-inbox` | `poll-inbox --since 1d` | Pullt IMAP, klassifiziert, schreibt Reply-Events |
| `openeraseme tick` | `tick` | **Lifecycle-Engine**: prüft Deadlines, sendet Reminders, eskaliert, triggert Re-Scans |
| `openeraseme reply` | `reply --request-id 42 --action confirm` | Manuelle/agent-getriebene Reaktion auf eine Antwort |
| `openeraseme reply` | `reply --request-id 42 --action escalate --draft-only` | LLM generiert Rebuttal-Entwurf |
| `openeraseme status` | `status --output json` | Aggregierter Stand: pro Status wieviele, welche Deadlines anstehen, welche Eskalationen drohen |
| `openeraseme requests` | `requests list --status overdue --output json` | Konkrete Anfragen filtern |
| `openeraseme events` | `events show --request-id 42` | Vollständige Historie einer Anfrage |
| `openeraseme schedule install` | `schedule install --daily 10:00` | Generiert launchd-plist / systemd-timer / crontab für `tick` und `poll-inbox` |
| `openeraseme grant` | `grant --command execute --ttl 24h` | Optionales Consent-Token (s. u.) |

**Sicherheits-Constraint (gilt für destruktive/sendende Befehle):**

`execute`, `reply` (nicht draft-only), `init` und `accounts add` verlangen entweder:
- Interaktive Bestätigung (TTY-Prompt) ODER
- ein gültiges Consent-Token (`OPENERASEME_CONSENT=<token>` env-var oder `--consent <token>`), das die CLI vorher ausstellt (`openeraseme grant`).

Das schützt vor versehentlichen oder bösartigen Aufrufen durch beliebige Tools, die im Nutzerkontext laufen — der AI-Agent muss explizit ein User-bestätigtes Token besitzen, um zu senden. Das ist die CLI-Variante des "human in the loop" für gefährliche Aktionen.

**Trennung Plan ↔ Execute:** Konsequent durchgezogen, kein einzelner Befehl verschickt ungeplante Mails. Verhindert "agent goes brrr"-Unfälle.

---

## Skill Bundle (für Skill-fähige Agents)

Skills sind Markdown-Dateien im Superpowers/Anthropic-Stil — selbst wenn ein Agent kein MCP spricht, kann er die Skills lesen und die CLI aufrufen.

`skills/SKILL.md` (top-level):
```markdown
---
name: using-openeraseme
description: Use when the user wants to delete personal data from data brokers
  (GDPR/CCPA "right to erasure"). Provides tools and workflow guidance.
---

# Removing personal data from brokers with OpenEraseMe

## When to use
- User says: "delete me from data brokers", "remove my data", "Incogni alternative"
- User wants quarterly cleanup of online presence

## Workflow
1. Run `setup-identity` skill if no identity vault exists yet
2. Run `plan-removal-campaign` to build a batch plan
3. Show plan to user, get explicit approval
4. Run `send-removal-batch` (default: 15-20 mails/day to avoid rate-limits)
5. Schedule `triage-broker-replies` to run daily for 60 days
6. Schedule `re-scan-quarterly` after 90 days
…
```

Jeder Sub-Skill verweist auf konkrete MCP-Tools oder CLI-Befehle.

---

## Sicherheits-Architektur

| Asset | Speicherort | Schutz |
|-------|-------------|--------|
| Identitäts­profil | Lokale verschlüsselte SQLite/JSON | AES-GCM, Master-Key in OS-Keyring (macOS Keychain / Windows Credential Locker / Freedesktop Secret Service) |
| OAuth2 Refresh-Tokens | OS-Keyring (über Himalaya oder direkt `keyring` lib) | OS-nativ |
| OAuth2 Client-Secret | OS-Keyring | OS-nativ |
| Status-DB | Lokale SQLite | optional verschlüsselt via SQLCipher |
| Broker-Replies (E-Mails) | nur Verweise (Message-ID) in DB; volle E-Mail bleibt im IMAP-Postfach des Nutzers | wie Postfach |
| Logs | `~/.local/share/openeraseme/logs/` mit Redaction (Name/Adresse → `[REDACTED]`) | Filesystem + opt. log-rotation |

**Threat Model Doc** (kommt in v0.1): explizite Auflistung von Vektoren (lokaler Malware-Zugriff, Mail-Provider-Breach, Registry-Supply-Chain-Attack auf Schema-Updates) und Mitigationen.

**Supply Chain:** Registry-Repo nutzt signed commits + ein `MAINTAINERS.toml`; nur Maintainer-Signed-Commits dürfen in `main`. Core-Engine verifiziert beim Pull, dass Updates vom erwarteten Schlüssel signiert sind (opt-in via `--verify-signatures`).

---

## Tech-Stack-Entscheidungen (konkret)

| Schicht | Wahl | Begründung |
|---------|------|-----------|
| Sprache | Python 3.11+ | Vom Nutzer gewählt, beste Bibliotheken |
| Build/Deps | `uv` + `pyproject.toml` | Schnell, modern, reproduzierbar |
| CLI | `typer` | Modernes Click, gute UX, Autocompletion |
| Mail-Versand | **Primary:** Himalaya CLI als Subprozess (`--output json`) — wegen OAuth2-Komfort, Multi-Account; **Fallback:** `aiosmtplib` + `aioimaplib` für Umgebungen ohne Himalaya | Hybrid: Himalaya nimmt OAuth2-Kopfschmerzen, Fallback für Container/CI |
| Web-Forms | `playwright` (Chromium) | Standard für Headless |
| CAPTCHA | `capsolver-python` Adapter, austauschbar gegen 2Captcha/Anti-Captcha | Kosten ~0,001 USD/Solve |
| Templates | `jinja2` | Standard, sandboxbar |
| Validierung | `pydantic v2` + `jsonschema` | Schema einmal, beides validiert |
| Persistenz | `aiosqlite` (+ optional SQLCipher) | Lightweight, embedded |
| Secrets | `keyring` lib (cross-OS) | OS-nativ |
| LLM | `anthropic` SDK, mit Prompt-Caching | Default Claude (Sonnet für Triage, Opus für komplexe Rebuttals); steckbar via Adapter-Interface, sodass Ollama/OpenAI/etc. nachträglich gehen |
| Scheduling | OS-native (cron/launchd/systemd) durch generierte Files; in-Process `apscheduler` als Alternative | Lokale Autonomie wichtiger als Cross-Platform-Magie |
| Tests | `pytest`, `pytest-asyncio`, `mailpit` (Docker) als lokaler SMTP/IMAP-Stub | Echte Mail-Flows testbar |
| Logging | `structlog` mit Redactor | Strukturierte Logs ohne PII-Leaks |

---

## Phasen / Roadmap

Trotz "voller Blueprint sofort" als MVP-Ziel: in **inkrementellen, jederzeit lauffähigen** Stufen, damit jede Phase einen Nutzwert liefert.

### Phase 0 — Foundation (1 Woche)
- Repo-Setup beider Repos, `pyproject.toml`, CI (lint+test+schema-validate)
- Broker-JSON-Schema (Single Source of Truth)
- 5 Beispiel-Broker (3× EU, 2× US) vollständig modelliert
- Pydantic-Modelle + Registry-Loader
- Identity Vault (Keyring + AES) + CLI-Befehle `init-profile`, `show-profile`

### Phase 1 — Email Removal MVP + Event-Store (2 Wochen)
- Himalaya-Adapter (subprocess + JSON-Parsing)
- OAuth2-Flow für Gmail + Outlook (Setup-Wizard im CLI)
- Templating-Engine (Jinja2 → fertige E-Mail)
- **Event-Store + Projektion** (Kernstück; wird ab hier von allen weiteren Phasen erweitert)
- CLI-Befehle: `plan`, `plan show`, `execute` (dry-run + echt), `events show`, `requests list`
- Consent-Token-Mechanik (`grant`)
- Erste 20 EU-Broker per E-Mail anschreibbar

### Phase 2 — Inbox-Triage + Lifecycle-Engine (2 Wochen)
- IMAP-Polling, Thread-Matching via Message-ID + References-Header
- LLM-Klassifizierer (Anthropic SDK + Prompt-Caching) → Event-Typ-Output (ACK, VERIFICATION_REQUESTED, CONFIRMED, REJECTED_FINAL, HUMAN_ACTION_REQUIRED, AUTORESPONDER)
- `poll-inbox`, `reply` als CLI-Befehle
- **`tick`-Engine** mit Deadline-/No-Response-Logik (Reminders, OVERDUE, DPA-Beschwerde-Entwurf)
- Auto-Confirmation für simple Klick-Bestätigungslinks (Playwright `GET`)
- Rebuttal-Template-Generator (LLM + Templates aus Registry)

### Phase 3 — Web-Form-Runner (2 Wochen)
- Playwright-Runner mit deklarativer Form-DSL aus broker YAML
- CapSolver-Integration (austauschbar via Adapter)
- 10 zusätzliche Broker mit Web-Formular
- Manuelles Fallback: "I cannot solve this, please complete in browser" → `HUMAN_ACTION_REQUIRED` Event + öffnet URL

### Phase 4 — Skill-Bundle + Agent-Integration (1 Woche)
- Skill-Bundle (`skills/*.md`) mit Setup, Plan, Send, Triage, Daily-Tick, Rescan, Handle-Action-Required
- Beispiel-Integration: Claude Code, OpenClaw, Cron-only Setup-Doku unter `examples/`
- Smoke-Test-Suite, die jeden Skill durch einen Agent fahren lässt

### Phase 5 — Scheduling + Reporting (1 Woche)
- Cron/launchd/systemd-Generator (`openeraseme schedule install`) für `tick` (täglich) und `poll-inbox` (4×/Tag)
- `status --output html` als statisches lokales Dashboard
- Aggregierter Kampagnen-Report (gelöscht/offen/eskaliert pro Broker)

### Phase 6 — Registry-Wachstum + Community-Onboarding (laufend)
- `CONTRIBUTING.md` mit Broker-Onboarding-Template
- "How to research a broker" Guide (welche Felder, woher Endpoints)
- Seed-Crawler: scraped Incogni-Public-Liste + BADBOOL, generiert YAML-Stubs für PR-Review
- Bot, der broken Links / 404 in Registry findet (weekly CI)

### Phase 7 (später, optional) — Web-SaaS-Variante
- Erst wenn v1.0 stabil und Community wächst
- Klar separater Tech-Stack-Entscheid (FastAPI? bestehende Engine wiederverwenden?)
- Pflicht: Threat-Modell-Update (Server wird zum Honeypot)

---

## Kritische Files (Übersicht für Implementierung)

Bei Phase 0/1 anzulegen / zu ändern:

- `pyproject.toml` — Paket-Definition, deps, scripts (entrypoint für `openeraseme` CLI)
- `src/openeraseme/registry/schema.py` — Pydantic-Modelle für Broker / Identity
- `src/openeraseme/core/identity.py` — Vault read/write
- `src/openeraseme/core/events.py` — Append-only Event-Store
- `src/openeraseme/core/projection.py` — `request_state` aus Events bauen
- `src/openeraseme/core/orchestrator.py` — Plan/Execute-Logik, schreibt Events
- `src/openeraseme/core/deadlines.py` — Tick-Engine, No-Response-/Eskalations-Logik
- `src/openeraseme/adapters/email/himalaya.py` — primärer Mail-Pfad
- `src/openeraseme/cli.py` — Typer-App mit Subcommands, `--output json` durchgängig
- `registry/schemas/broker.schema.json` — Schema, single source of truth
- `registry/brokers/eu/_example.yaml` — Anker für Contributoren
- `registry/laws/gdpr-art17.de.md.j2` — erstes Template
- `skills/SKILL.md` — Top-Level-Skill für AI-Agents

---

## Wiederverwendete / nicht neu zu schreibende Komponenten

| Funktion | Quelle | Nutzungsart |
|---------|--------|-------------|
| OAuth2-Mail-Auth | **Himalaya CLI** | Subprocess, kein eigener XOAUTH2-Code |
| OS-Keyring-Zugriff | **`keyring`** Python-Lib | direkter Import |
| Broker-Seed-Daten | **BADBOOL** (yaelwrites), **Incogni-Public-Liste**, **JustVanish organization-schema.json** | Einmalig scrapen → YAML-Stubs generieren, manuell reviewen |
| Web-Form-Runner-Pattern | **auto-identity-remove** (stephenlthorn) | Konzept übernehmen (90-day re-check, CapSolver), eigenständig in Python neu implementieren (jenes Projekt ist Shell-Skript-zentriert) |
| Template-Idee | **JustVanish** | rechtliche Textbausteine als Vorlage |

---

## Verifikation

End-to-End-Tests pro Phase, Reihenfolge bewusst von "fastest signal" zu "real world":

**Phase 0:**
- `pytest tests/unit/test_schema.py` — Beispiel-YAMLs validieren gegen Schema, ungültige Fixtures werden abgelehnt
- `openeraseme init-profile` → `show-profile` Round-Trip; Master-Key sichtbar in OS-Keyring, JSON-Datei verschlüsselt auf Disk

**Phase 1:**
- Lokaler Mailpit-Container als SMTP/IMAP-Stub: `docker run -p 1025:1025 -p 8025:8025 axllent/mailpit`
- Integrationstest: `openeraseme execute --dry-run` rendert Mail korrekt, ohne Token blockiert; mit Token landet sie in Mailpit
- Event-Store-Test: nach `plan` + `execute` exakt die erwarteten Events (`PLANNED`, `SENT`) in DB; Projektion zeigt `current_status=AWAITING_ACK`
- Rebuild-Test: Projektion löschen, aus Events neu bauen → identischer Zustand
- Consent-Token-Erzwingung: `execute` ohne Token → klare Fehlermeldung, Exit-Code != 0
- Echter Smoke-Test mit Gmail-Test-Account: 1 Mail an eigene Adresse, OAuth2-Flow erfolgreich

**Phase 2:**
- Reply-Fixtures (`tests/fixtures/broker_replies/`) durch Triage laufen lassen, Klassifikation gegen erwartete Event-Typen prüfen
- Live-Test: Selbst-Absender (eigene Adresse als "Broker"), Reply → `poll-inbox` → richtiges Event geschrieben, Status springt korrekt
- **Tick-Test** mit Zeit-Stub: Request-Datum 31 Tage in der Vergangenheit, kein ACK → `tick` schreibt `DEADLINE_REACHED` + `OVERDUE`-Status; 14 Tage später → `DPA_COMPLAINT_DRAFTED` mit korrektem PDF-Entwurf
- Reminder-Test: Status `AWAITING_ACK`, 8 Tage ohne Antwort → `tick` schickt Reminder, `reminders_sent` zählt hoch

**Phase 3:**
- Playwright-Tests gegen lokale Test-Form (statisches HTML mit reCAPTCHA-Stub)
- Manueller Live-Lauf gegen einen echten EU-Broker (mit eigener Test-Identität), Erfolg = Bestätigungs-Mail in Inbox + `CONFIRMED`-Event nach Triage

**Phase 4:**
- Skill-Bundle in echtem Claude-Code-Setup einbinden, Agent durch kompletten Workflow fahren (init → plan → execute → tick → reply)
- `openeraseme status --output json` durch Agent parsen lassen → richtige Empfehlung für nächste Aktion

**Phase 5:**
- `openeraseme schedule install --daily 10:00` legt korrekten launchd-plist/cron-line an, `tick` läuft am nächsten Tag, schreibt Audit-Log-Eintrag

**Acceptance v0.1:** Ein Tester kann mit einer echten Identität in <30 min Setup machen und in den folgenden 60 Tagen messbar ≥10 erfolgreiche Löschungen bei realen EU-Brokern erzielen, dokumentiert in `examples/case-study.md`.

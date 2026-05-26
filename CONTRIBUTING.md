# Contributing to Symaira EraseMe

## Quick Start

1. Fork the repository and create a feature branch:
   ```bash
   git checkout -b feature/my-feature
   ```
2. Install dependencies: `uv sync`
3. Run tests: `pytest`
4. Run lints: `ruff check . && mypy src/symeraseme/`
5. Open a pull request — use the PR template checklist.

---

## Adding a Data Broker (most impactful contribution)

Adding a broker lets Symaira EraseMe automatically file opt-out requests
on behalf of users. No Python knowledge required — just a YAML file.

### Step 1 — Open an issue first (optional but helpful)

Use the **Add / Update Data Broker** issue template on GitHub. This lets
maintainers flag duplicates and share research before you write the YAML.
If you have all the details already, you can skip straight to a PR.

### Step 2 — Create the YAML file

Pick the right directory based on the broker's primary jurisdiction:

| Directory | Laws covered | Examples |
|-----------|-------------|---------|
| `registry/brokers/us/` | CCPA, CPRA | Spokeo, BeenVerified, ZoomInfo |
| `registry/brokers/eu/` | GDPR | SCHUFA, Creditreform, Criteo |
| `registry/brokers/uk/` | UK GDPR | Experian UK, TransUnion UK |

Name the file `<broker-slug>-<region>.yaml` (e.g. `radaris-us.yaml`).
The `id` field in the YAML must match: `radaris-us`.

Use `registry/brokers/eu/_example.yaml` as the reference template.
The JSON schema is at `registry/schemas/broker.schema.json`.

### Step 3 — Fill in the key fields

**Minimum required fields:**

```yaml
id: example-broker-us          # unique, lowercase, hyphenated
name: Example Broker Inc.
website: https://example.com
category: people-search         # see allowed values below
jurisdictions: [US]
laws: [CCPA]
priority: high                  # high / medium / low
opt_out:
  - type: email                 # or web_form
    endpoint: privacy@example.com
    template: ccpa-deletion
    required_fields: [full_name, email, address]
    expected_response_days: 30
verification:
  ack_keywords: ["request received", "opt-out"]
```

**Category values:** `people-search`, `background-check`, `marketing`,
`credit`, `analytics`, `social-media`, `other`

### Step 4 — Finding the CAPTCHA site key

If the opt-out form has a reCAPTCHA or hCaptcha, you need the live
`data-sitekey` value — not a placeholder. Here's how to find it:

1. Open the opt-out page in Chrome/Firefox.
2. Right-click → **Inspect** → search the HTML for `data-sitekey`.
3. Copy the full key (looks like `6Lc...XXXX`).
4. Paste it into the `site_key` field of the `solve_captcha` step.

If you cannot capture a live site key, set `disabled: true` on that
`opt_out` entry and add a `notes:` block explaining what's missing.
A partial entry is better than no entry — maintainers can complete it later.

### Step 5 — Identity placeholders in form fields

Web-form steps can reference identity fields with `${field_name}` syntax
(e.g. `${full_name}`, `${email}`). If a referenced field is missing from the
user's profile, the form runner aborts with an error listing the unresolved
placeholders. Only reference fields that are guaranteed to exist for the
broker's target jurisdiction.

### Step 6 — Find the acknowledgement text

After submitting a real opt-out (use a throwaway email), note the exact
wording of the confirmation message. Add key phrases to `ack_keywords`.
These are used by the automated triage engine to mark requests as resolved.

### Step 7 — Validate

```bash
symeraseme registry validate registry/brokers/us/example-broker-us.yaml
```

Or validate the whole registry:

```bash
symeraseme registry validate
```

### Step 8 — Open a pull request

Use the PR template. Complete the **Registry Additions** table at the
bottom. A maintainer will review the opt-out URL/email and merge.

---

## Research-Only Contributions

Not sure about the form fields or CAPTCHA key? Open an issue using the
**Add / Update Data Broker** template and fill in what you know. Mark
unknown fields with `NEEDS_RESEARCH`. Someone else can complete the rest.

---

## Code Contributions

### Code Style

- Python 3.11+ with type annotations.
- Format with `ruff format`.
- All CLI output must support `--output {text,json}`.
- All commands return a non-zero exit code on failure for both text and JSON output.
  `_render` is the single place that raises `typer.Exit(1)` for failed `CliResult` instances.
- All models must use pydantic v2.
- Default to no comments — only add one when the *why* is non-obvious.
- Pin all GitHub Actions to a release tag or commit SHA (e.g. `uses: owner/action@v1.2.3`).
  Dependabot is configured to propose upgrades automatically.

### Testing

| Suite | Location | Notes |
|-------|----------|-------|
| Unit tests | `tests/unit/` | No external dependencies |
| Integration tests | `tests/integration/` | Not yet implemented |
| Registry validation | `tests/smoke/test_broker_validation.py` | Schema + lint checks |

Run everything: `pytest`

Run only registry validation: `pytest tests/smoke/test_broker_validation.py`

---

## Broker Registry at a Glance

```
registry/
├── brokers/
│   ├── us/          # CCPA/CPRA brokers
│   ├── eu/          # GDPR brokers
│   │   └── _example.yaml   # reference template
│   └── uk/          # UK GDPR brokers
├── laws/            # Law definitions
├── schemas/
│   └── broker.schema.json  # JSON Schema for YAML validation
└── templates/       # Email opt-out templates (gdpr-art17, ccpa-deletion, …)
```

---

## Questions?

Open a [Discussion](https://github.com/danieljustus/Symaira EraseMe/discussions)
or comment on an existing issue. Security issues go to the
[Security Policy](https://github.com/danieljustus/Symaira EraseMe/security/policy).

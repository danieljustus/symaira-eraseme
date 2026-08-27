# Registry Data Contract

**Status:** pinned · **Schema version:** `1` · **Manifest:** `registry/manifest.json`

This document is the language-neutral contract for the data broker registry.
It exists so a second implementation (the Go port, milestone v1.0.0) can be
built and verified against the same rules without reading Python. The Python
loader (`src/symeraseme/registry/loader.py`) and its tests are one conformance
implementation of this document; the Go loader will be the second.

## 1. Versioning

- The contract version lives in exactly two places, and they must agree:
  - `registry/manifest.json` → `schema_version`
  - `registry/schemas/broker.schema.json` → `schema_version` (top-level
    document field, ignored by JSON Schema validators, read by tooling)
- Any change that adds, removes, or renames a field, enum value, or
  validation rule is a **major** version bump (`1` → `2`). The manifest and
  schema are updated together in one commit.
- A consumer must refuse to process a registry whose manifest version it
  does not understand (`RegistryError` in Python).

## 2. Layout

```
registry/
  manifest.json            contract version + schema pointers
  schemas/
    broker.schema.json     JSON Schema (draft 2020-12) for every broker file
  brokers/
    <jurisdiction>/        one YAML file per broker
      <broker-id>.yaml     id must equal the file stem
  laws/                    letter templates (Jinja2), not part of this contract
  templates/               dashboard/report templates, not part of this contract
```

- Files starting with `_` (e.g. `_example.yaml`) are documentation-only and
  are **not** validated and **not** loaded.
- Jurisdiction directories: `eu/`, `uk/`, `us/`. A broker's `jurisdictions`
  field is independent of the directory it lives in.

## 3. Broker document

Top-level fields (all YAML → JSON scalars):

| Field | Type | Required | Rules |
|---|---|---|---|
| `id` | string | yes | `^[a-z0-9-]+$`, equals file stem |
| `name` | string | yes | |
| `website` | string | yes | `format: uri` (https:// or http://) |
| `category` | string | yes | closed enum: `people-search`, `marketing`, `credit`, `analytics`, `background-check`, `social-media`, `other` |
| `jurisdictions` | string[] | yes | closed enum, min 1: `AT CH DE DK EU FI FR GB IE IL LU NL NO SE UK US` |
| `laws` | string[] | yes | closed enum, min 1: `GDPR CCPA CPRA LGPD PIPEDA` |
| `data_sensitivity` | int | no | 1..5, default 3 |
| `priority` | string | yes | closed enum: `high`, `medium`, `low` |
| `opt_out` | object[] | yes | min 1 channel, see §4 |
| `verification` | object | no | keyword sets, see §5 |
| `disabled` | bool | no | default false; true = broker excluded from default plans, pair with `notes` |
| `added_date` | string | no | `format: date` (ISO 8601) |
| `source` | string | no | provenance reference |
| `status` | string | no | closed enum: `active`, `deprecated`, `merged`, `out-of-business`; default `active` |
| `notes` | string | no | |

**Unknown top-level fields are rejected** (`additionalProperties: false`).

## 4. Opt-out channels

`opt_out` is a non-empty array of channel objects. Each channel has:

| Field | Type | Required | Rules |
|---|---|---|---|
| `type` | string | yes | closed enum: `email`, `web_form`; selects the channel variant |
| `endpoint` | string | no | **email only**; `format: email` |
| `url` | string | no | **web_form only**; `format: uri` |
| `form_spec` | object | no | **web_form only**; see §6 |
| `template` | string | no | closed enum: `ccpa-deletion`, `gdpr-art17` |
| `locale` | string | no | `^[a-z]{2}(-[A-Z]{2})?$` (RFC 5646) |
| `required_fields` | string[] | no | closed enum items: `full_name`, `email`, `address`, `date_of_birth`, `state` |
| `supports_suppression` | bool | no | |
| `expected_response_days` | int | no | ≥ 1 |
| `disabled` | bool | no | channel-level disable |

Variant rules (`oneOf`, exactly one variant applies):

- **email**: `type: email`, requires `endpoint`. Other fields above are
  optional (template/locale/required_fields/supports_suppression/
  expected_response_days are commonly present).
- **web_form**: `type: web_form`, requires `url` + `form_spec`.

**Unknown channel fields are rejected.** A channel carrying both email and
web_form fields (e.g. `endpoint` + `url`) fails validation.

## 5. Verification block

Optional object; when present it must contain only:

| Field | Type | Rules |
|---|---|---|
| `ack_keywords` | string[] | reply signals acceptance |
| `rejection_keywords` | string[] | reply signals refusal |
| `human_required_keywords` | string[] | reply requires manual verification |

All three are optional inside the block. Unknown keys are rejected.

## 6. Web-form specification

`form_spec` = `{ "steps": [FormStep...], "timeout_seconds": number, "rate_limit_delay": number, "headless": bool }` — only `steps` is required.

A `FormStep` is a step DSL executed by a browser driver. All keys optional,
at least one must be present (`minProperties: 1`), unknown keys rejected:

| Field | Type | Rules |
|---|---|---|
| `goto` | string | URL; `.` = form's own URL |
| `fill` | object | CSS-selector → value; keys must look like CSS selectors (`[#.[]`); `${field_name}` interpolates identity fields |
| `select` | object | CSS-selector → option value; same key rules as `fill` |
| `click` | string | CSS selector |
| `wait_for` | string | CSS selector to wait for |
| `wait_seconds` | number | ≥ 0 |
| `screenshot` | string | label without extension |
| `assert_text` | string | text that must appear |
| `solve_captcha` | object | see below |

`solve_captcha`: `type` (closed enum: `recaptcha-v2`, `recaptcha-v3`,
`hcaptcha`, `turnstile`) + `site_key` (min length 8, real key) are required;
`provider` (closed enum: `capsolver`, `2captcha`, `anticaptcha`), `action`,
`min_score` (0..1), `is_invisible` optional. Unknown keys rejected.

## 7. Validation entry points

Two independent checkers must both pass for any change to the registry:

1. **Schema validation** — every non-`_` YAML under `registry/brokers/**`
   validates against `registry/schemas/broker.schema.json` and `id` equals
   the file stem. Python checkers:
   - CI job `schema-validate` (`.github/workflows/ci.yml`)
   - `tests/unit/test_registry_contract.py::test_every_live_broker_document_validates_against_pinned_schema`
2. **Loader conformance** — the golden fixtures
   (`tests/fixtures/registry-contract/`) load through the runtime loader
   unchanged, and the invalid fixture is rejected:
   `tests/unit/test_registry_contract.py`.

The Go conformance suite (milestone v1.0.0) will run the same two checks
against the same files.

## 8. Golden fixtures

| File | Branch covered |
|---|---|
| `golden-webform-us.yaml` | full web_form: every formStep action incl. captcha, CCPA/US |
| `golden-email-eu.yaml` | full email channel with all optional fields, GDPR/EU multi-jurisdiction (DE, AT, EU) |
| `golden-multi-uk.yaml` | both channel types in one broker, UK |
| `golden-minimal-us.yaml` | minimal required fields, disabled channel, no optional blocks |
| `invalid-unknown-field.yaml` | negative: unknown top-level field + invalid enums → must fail |

## 9. Implicit behaviours (traps for the port)

These are behaviorally true in the Python implementation today and must be
replicated, but are not visible in the schema:

- `template`/`locale`/`required_fields`/`supports_suppression`/
  `expected_response_days` are **not** email-only in practice — web_form
  channels carry them too (observed 583 of 624 web_form entries). The schema
  allows them on both variants; the Go model must not reject them.
- `verification` exists on 100% of live entries but is optional in the
  schema; the Go loader must treat a missing block as `null`, not as an
  error.
- `disabled` exists both on the broker and on the channel; a disabled channel
  is skipped by exporters but still validates.
- Observed enum coverage: `laws` uses only `GDPR` + `CCPA` today; the other
  three enum values are future-proofing and must remain accepted.
- `data_sensitivity` and `status` are absent from exactly the `_example.yaml`
  files — a real broker without them is not known to exist; treat defaults
  (3 / active) as authoritative.
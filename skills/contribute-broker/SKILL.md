# Contribute Broker to Symaira EraseMe Registry

**Skill for AI agents (Claude Code, OpenClaw, Cursor, etc.)**

When you discover a data broker that is not in the Symaira EraseMe registry, use this workflow to contribute it back via a Pull Request.

## When to use this skill

Use this skill when:
- You discover a new data broker during research or web browsing
- A user mentions a broker that does not exist in the registry
- You find updated information about an existing broker (new opt-out URL, changed email, etc.)
- The user explicitly asks you to add or update a broker entry

## Workflow

### 1. Check if broker already exists

```bash
# Search by name
grep -ri "<broker_name>" registry/brokers/

# Or use the broker ID pattern
ls registry/brokers/us/ | grep -i "<partial_name>"
```

If the broker exists, skip to step 5 (Update existing).

### 2. Gather broker information

Collect the following REQUIRED fields:
- **Name**: Legal company name
- **Website**: Homepage URL (with https://)
- **Category**: One of: people-search, marketing, credit, analytics, background-check, social-media, other
- **Jurisdictions**: ISO country codes (e.g., US, DE, EU)
- **Laws**: Applicable privacy laws (GDPR, CCPA, CPRA, LGPD, PIPEDA)
- **Opt-out method**: Email address or web form URL
- **Source**: Which registry/website verified this information

### 3. Create the YAML file

Create a new file at `registry/brokers/<jurisdiction>/<broker-id>.yaml`:

```yaml
id: <broker-id>
name: <Full Legal Name>
website: https://<website>
category: <category>
jurisdictions:
  - <country_code>
laws:
  - <law>
data_sensitivity: 3
priority: medium
added_date: '<YYYY-MM-DD>'
status: active
source: '<Where you found this broker>'

opt_out:
  - type: email
    endpoint: <privacy@broker.com>
    template: <ccpa-deletion|gdpr-art17>
    locale: <en|de|...>
    required_fields:
      - full_name
      - email
    supports_suppression: true
    expected_response_days: <45|30>

verification:
  ack_keywords:
    - received
    - request
  rejection_keywords:
    - cannot
    - denied
  human_required_keywords:
    - verify
    - identification
```

### 4. Validate the file

```bash
# Install dependencies if needed
pip install pyyaml jsonschema

# Validate
python scripts/registry_sync.py --validate-all
```

### 5. Create a Pull Request

```bash
# Create a feature branch
git checkout -b add-broker-<broker-id>

# Stage and commit
git add registry/brokers/<jurisdiction>/<broker-id>.yaml
git commit -m "registry: add <Broker Name> data broker

- Source: <where you found it>
- Jurisdiction: <country>
- Opt-out: <email|web_form>"

# Push and create PR
git push -u origin add-broker-<broker-id>
gh pr create --fill --title "registry: add <Broker Name>" --body "..."
```

### 6. Update existing broker (if information changed)

If the broker exists but information is outdated:

```bash
# Edit the existing file
# Update only the changed fields
# Add a note about what changed

# Validate
python scripts/registry_sync.py --validate-all

# Commit with descriptive message
git add registry/brokers/<jurisdiction>/<existing-id>.yaml
git commit -m "registry: update <Broker Name> opt-out email

Old: old@example.com
New: privacy@example.com
Source: <where you verified this>"
```

## Important rules

1. **Never fabricate data**: Only add brokers you have verified from a reliable source
2. **Always include source**: The `source` field must reference where you found the broker
3. **Validate before PR**: Run `--validate-all` to ensure schema compliance
4. **One broker per PR**: Keep PRs focused on a single broker addition or update
5. **Respect existing format**: Match the style of existing broker YAML files
6. **Privacy first**: Never include personal data in broker definitions

## Example: Adding a new broker

```bash
# User mentions: "I found this broker called DataMax LLC"
# 1. Check if it exists
grep -ri "datamax" registry/brokers/
# Result: Not found

# 2. Research
curl -s "https://datamax.example.com/privacy" | grep -i "opt.out\|delete\|remove"
# Found: privacy@datamax.example.com

# 3. Create YAML
cat > registry/brokers/us/datamax-us.yaml << 'EOF'
id: datamax-us
name: DataMax LLC
website: https://www.datamax.example.com
category: marketing
jurisdictions:
  - US
laws:
  - CCPA
data_sensitivity: 3
priority: medium
added_date: '2025-05-21'
status: active
source: 'Company privacy page'

opt_out:
  - type: email
    endpoint: privacy@datamax.example.com
    template: ccpa-deletion
    locale: en
    required_fields:
      - full_name
      - email
    supports_suppression: true
    expected_response_days: 45

verification:
  ack_keywords:
    - received
    - request
  rejection_keywords:
    - cannot
    - denied
  human_required_keywords:
    - verify
EOF

# 4. Validate
python scripts/registry_sync.py --validate-all

# 5. Create PR
git checkout -b add-broker-datamax
git add registry/brokers/us/datamax-us.yaml
git commit -m "registry: add DataMax LLC data broker"
git push -u origin add-broker-datamax
gh pr create --fill
```

## Quick reference: Broker categories

| Category | Description |
|----------|-------------|
| people-search | Sites that aggregate personal info (Spokeo, Whitepages) |
| marketing | Email/phone list brokers, ad tech |
| credit | Credit bureaus, financial data |
| analytics | Data analytics, market research |
| background-check | Employment/criminal background checks |
| social-media | Social media platforms |
| other | Everything else |

## Quick reference: Jurisdiction codes

| Code | Region |
|------|--------|
| US | United States |
| DE | Germany |
| EU | European Union |
| UK | United Kingdom |
| CA | Canada |
| BR | Brazil |

## Quick reference: Laws

| Law | Region |
|-----|--------|
| GDPR | EU/EEA |
| CCPA | California |
| CPRA | California (updated) |
| LGPD | Brazil |
| PIPEDA | Canada |

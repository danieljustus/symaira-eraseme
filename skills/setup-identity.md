# Setup Identity

Guide an AI agent or user through creating and managing their identity vault.

## Prerequisites

- Go binary `symeraseme` is on PATH
- No existing identity profile (run `symeraseme show-profile` to check)

## Creating an identity profile

The identity profile stores the user's personal information in an encrypted
vault. This data is used to fill in removal request forms and templates.

### Step 1: Initialize the profile

```bash
symeraseme init-profile
```

You will be prompted for:
- **Full name** — Legal name as it appears on official records.
- **Email address** — Primary email used for removal requests.

### Step 2: Verify the profile

```bash
symeraseme show-profile
```

Expected output:
```
Name:  Jane Doe
Email: jane@example.com
```

### JSON output (for AI agents)

```bash
symeraseme show-profile --output json
```

```json
{
  "full_name": "Jane Doe",
  "email_addresses": ["jane@example.com"],
  "addresses": [],
  "phone_numbers": [],
  "jurisdictions": []
}
```

## Required fields and best practices

| Field | Required | Best practice |
|-------|----------|---------------|
| Full name | Yes | Use legal name as it appears on government ID |
| Email | Yes | Primary email; add aliases later via identity vault |
| Addresses | No | Add if plan includes brokers requiring postal verification |
| Phone numbers | No | Add if plan includes brokers requiring SMS verification |
| Jurisdictions | No | Add GDPR for EU, CCPA for California; affects broker filtering by `--jurisdiction` |

## When to update the profile

Update the profile when:

- The user changes their name (marriage, legal name change)
- The user switches primary email addresses
- Removal requests start failing due to outdated contact info
- The user moves to a new jurisdiction (affects which laws apply)

## Error handling

| Symptom | Cause | Fix |
|---------|-------|-----|
| `No identity profile found` | Profile not yet created | Run `init-profile` |
| `File exists` error | Profile already exists | Use `show-profile` to view; update via `init-profile` (overwrites) |
| Keyring errors | No system keyring available | Set `SYMERASEME_DATA_DIR` to a writable path |

## Troubleshooting

**Q: Can I have multiple profiles?**
A: No. The system supports one identity vault per machine. Use different
   machines for different identities, or manually backup/restore the vault file.

**Q: How is my data stored?**
A: The identity profile is encrypted at rest using AES-256-GCM, and the
   event store database uses a standard Fernet envelope (AES-128-CBC + HMAC-SHA256).
   The key is resolved through the configured secure secret provider.

**Q: What if I mistype my name?**
A: Re-run `init-profile` with the correct information. It overwrites the
   existing profile.

## Secrets from Symaira Vault

Symaira EraseMe supports canonical `symvault://` URIs for credentials.

```bash
# Store secrets in Symaira Vault
symvault set anthropic/prod-key "sk-ant-..."
symvault set captcha/capsolver "CAP-..."
symvault set email/imap-password "your-imap-password"
```

Then set environment variables to point at the vault:

```bash
export ANTHROPIC_API_KEY="symvault://anthropic/prod-key"
export CAPSOLVER_API_KEY="symvault://captcha/capsolver"
export IMAP_PASSWORD="symvault://email/imap-password"
```

**Resolution order**: `symvault://` → environment → platform secure store.

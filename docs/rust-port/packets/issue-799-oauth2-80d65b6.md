# Issue 799 OAuth2 follow-up context

- **Task:** make the existing XOAUTH2 transport reachable from production env, MCP and CLI paths
- **Base SHA:** `80d65b64a7248d4d2f1740c1e636caecab93a88e`
- **Scope:** IMAP OAuth2 configuration, secret resolution/redaction, MCP schema fixture and production-path tests
- **Required precedence:** explicit OAuth reference > environment OAuth reference > password; OAuth presence prevents password resolution
- **Security:** explicit references cannot silently fall back to environment; Python-compatible keyring account uses final overridden username
- **Acceptance:** 26 tools unchanged; raw/base64 SASL secrets absent from errors; full/race/lint/build and Windows compile green

# Security Policy

## Supported Versions

| Version | Supported |
|---|---|
| `v0.1.x` | :white_check_mark: |

---

## Reporting a Vulnerability

EstzKit manages software installations and system configuration on user workstations. We take security and package integrity very seriously.

**Please DO NOT report security vulnerabilities through public GitHub issues.**

Instead, report vulnerabilities through **GitHub Private Vulnerability Reporting**:
1. Navigate to the repository's **Security** tab.
2. Under "Reporting", click **Report a vulnerability**.
3. Provide details of the issue, affected versions, and reproduction steps or proof-of-concept.

### Scope of Security Reports
- Unintended privilege escalation beyond explicit user UAC consent.
- Arbitrary code execution vulnerabilities via manifest parsing.
- Path traversal or SQLite database corruption attacks.
- Provider injection or untrusted shell execution.

### Response Commitment
We aim to acknowledge and triage reports within 48 hours and coordinate a fix before public disclosure.

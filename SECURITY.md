# Security Policy

## Supported Versions

Only the latest released version of `repograph` and `repograph-core` receives security fixes. Older versions are not supported.

| Crate            | Supported  |
|------------------|------------|
| `repograph`      | latest only |
| `repograph-core` | latest only |

## Reporting a Vulnerability

Please report suspected security vulnerabilities **privately** via GitHub's [Security Advisories](https://github.com/maikbasel/repograph/security/advisories/new) flow. Do not open a public issue.

You can expect:

- Initial acknowledgement within 5 business days.
- A coordinated disclosure timeline once the report is triaged.
- Credit in the release notes for the fixing version, unless you prefer to remain anonymous.

If you'd rather email, contact the maintainer at <maik.basel@gmx.de> with subject prefix `[security]`.

## Automated Auditing

This project runs [`rustsec/audit-check`](https://github.com/rustsec/audit-check) daily via GitHub Actions and `cargo-deny` on every PR against the [RustSec Advisory Database](https://rustsec.org/). New advisories typically result in a fix within one release cycle.

## Verifying Release Artifacts

Every binary tarball, `.zip`, and installer script attached to a GitHub Release is signed with the maintainer's GPG key. To verify a download:

```bash
# 1. Import the maintainer's public key (one-time)
curl -sSL https://github.com/maikbasel.gpg | gpg --import

# 2. Download both the artifact and its detached signature
gh release download v0.1.0 \
  --repo maikbasel/repograph \
  --pattern 'repograph-x86_64-unknown-linux-gnu.tar.xz' \
  --pattern 'repograph-x86_64-unknown-linux-gnu.tar.xz.asc'

# 3. Verify
gpg --verify repograph-x86_64-unknown-linux-gnu.tar.xz.asc \
              repograph-x86_64-unknown-linux-gnu.tar.xz
```

A "Good signature from Maik Basel" line indicates a valid release. The shell + PowerShell installer scripts and `cargo install repograph` rely on cargo-dist's SHA-256 checksums (also attached to each release) and crates.io's own integrity checks; GPG signatures are an additional layer for users who want supply-chain assurance.

The signing key fingerprint will be published here once the first signed release ships.

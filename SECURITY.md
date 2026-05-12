# Security policy

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.4.x   | Yes       |
| < 0.4   | No        |

## Reporting a vulnerability

Please do not report security vulnerabilities through public GitHub issues.

Instead, send an email to **jeroen@flexworks.eu** with:

- A description of the vulnerability and its potential impact.
- Steps to reproduce or a proof-of-concept (if available).
- Any suggested mitigations.

You will receive an acknowledgement within 48 hours and a more detailed response within 5 business days outlining next steps.

Please do not disclose the issue publicly until a fix has been released and coordinated with you.

## Scope

The following are in scope:

- Authentication bypass or signature validation weaknesses in the proxy layer.
- Credential leakage through logs, headers, or error responses.
- Denial of service through malformed requests.
- Insecure defaults in `ProxyServerConfig` (e.g. `verify=False`, `skip_signature_validation=True`) that could be silently inherited in production.

## Out of scope

- Vulnerabilities in upstream dependencies (report those to the respective project).
- Issues that require physical access to the host.

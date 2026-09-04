# Security policy

## Supported versions

The latest Bastle release is supported with security fixes. Pre-release builds
and the archived Spider project are not covered by this policy.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting for
`Cheviiot/bastle`. Do not open a public issue containing exploit details,
credentials, cookies, profiles, or other private site data.

Include the Bastle version, Flatpak runtime version, reproduction steps, impact,
and whether the issue requires a malicious website or local access. A maintainer
will acknowledge a complete report within seven days and coordinate disclosure
after a fix is available.

Bastle isolates site data between its managed applications, but does not claim
to be a general-purpose security sandbox. Flatpak, WebKitGTK, the desktop
portals, and the operating system remain part of the security boundary.

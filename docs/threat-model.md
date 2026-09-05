<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Bastle threat model

This document covers Bastle's WebKit and Chromium runtimes, per-application
data, portal calls, backups, and internal engine process boundary. It describes
security goals, not a claim that arbitrary websites are safe or fully
compatible.

## Assets and trust boundaries

- Every web application has a separate configuration, policy, WebKit profile,
  cookie store, and cache. A website is untrusted input even when the user
  intentionally installs it.
- Bastle's local repository and the Flatpak sandbox are trusted to enforce
  application identity and filesystem separation. The host desktop, WebKitGTK,
  xdg-desktop-portal, and the selected portal backend are external trusted
  components.
- File Chooser and Dynamic Launcher Portal responses are user-authorized input.
  Bastle does not bypass a missing or denied portal by writing to the host.
- A `.bastle-backup` is untrusted until its archive structure, size limits,
  manifest, IDs, configuration, and policy have all been validated. Site data
  is sensitive and is exported only in a passphrase-encrypted archive.
- The Chromium engine is included in the Bastle Flatpak and therefore shares
  its sandbox permissions. It still runs in a separate process with a separate
  profile and cache per app ID. Selecting Chromium never copies WebKit cookies
  or silently changes the selected engine.

## Security and privacy goals

- Prevent one Bastle application from accidentally reading or deleting another
  application's metadata or profile through IDs, archive paths, or races.
- Preserve local data if a portal operation fails and roll back staged changes
  when launcher installation does not complete.
- Treat unknown schema versions and corrupt data as diagnostics; never
  downgrade, overwrite, or automatically delete them.
- Require an explicit user decision for website permissions, navigation
  restrictions, proxy overrides, background activity, content filters, backup
  of site data, and engine changes.
- Keep new privacy and power features disabled by default. Ordinary third-party
  resources remain available unless the user imports and enables a content
  filter.
- Never persist proxy credentials. Custom proxy URIs containing credentials are
  rejected.
- Use the Background Portal for authorization and autostart. A refusal only
  disables background behavior; it does not prevent a normal foreground run.
- Ship compatibility recommendations locally without telemetry or remote
  catalog updates.

## Principal threats and controls

| Threat | Control |
| --- | --- |
| Malicious title, URL, icon, policy, or filter input | Scheme restrictions, normalization, control-character removal, bounded reads, JSON/schema validation, sandboxed image decoding, and WebKit filter compilation before activation. |
| Archive traversal, links, device files, decompression abuse, or partial restore | Relative allowlisted paths, no links, entry and aggregate limits, staged extraction, runtime locks, and per-application transactional restore. |
| Partial or concurrent metadata writes | Per-application locks, temporary files, atomic rename, parent-directory sync, and field-aware policy merge. |
| Launcher or background access without consent | Dynamic Launcher and Background portals only; denial is surfaced and host files are not written directly. |
| Unexpected top-level navigation | Optional origin allowlist with one-time, persistent, external-browser, or block choices. WebKit identifies the main resource at response-policy time, so a disallowed server may receive the initial request before Bastle prevents the response from becoming the top-level document. |
| Proxy credential disclosure | Credentials, query strings, fragments, and paths are rejected in custom proxy URIs; Bastle stores only a normalized endpoint. |
| Over-broad content blocking | Imported WebKit content-extension rules are opt-in and scoped to one app; no implicit third-party-resource blocking is added. |
| Hidden background activity | Per-app opt-in state, portal authorization, a system status message, and an explicit Stop action. The portal grant belongs to the Bastle Flatpak as a whole, while Bastle keeps the enabled-app list in each app's policy. |
| Chromium engine compromise | One Flatpak sandbox with no host filesystem or Flatpak-control permission, a narrow internal D-Bus protocol, pinned/offline build inputs, validated inputs, separate per-app Electron processes and profiles, and safe Electron defaults. |

## Availability and compatibility limits

- Bastle does not promise DRM/Widevine, browser extensions, proprietary browser
  APIs, anti-bot bypasses, or behavior identical to Chromium.
- WebKit, the desktop portal backend, the network, and sites can fail
  independently. Bastle must retain editable local metadata and offer precise
  diagnostics rather than claiming success.
- Origin allowlists are not content-security policies or network firewalls.
  They control which HTTP(S) response may become the top-level document;
  subresources are unaffected unless content filters are explicitly enabled.
- A proxy protects only WebKit network traffic configured for that application's
  `NetworkSession`; portal traffic and Bastle's metadata fetches are outside
  that runtime session.
- Background execution remains subject to desktop policy, resource pressure,
  logout, process termination, and portal implementation differences.

## Review triggers

The embedded engine is not a separate Flatpak security boundary. A compromise
of either native runtime must be evaluated against the permissions of the
single Bastle sandbox. Process and profile separation reduce accidental
cross-application access, but are not equivalent to separate sandboxes.

Update this model before adding new Flatpak permissions, transferring site data
between engines, persisting secrets, changing archive formats, enabling remote
catalog updates, or adding a new privileged D-Bus method.

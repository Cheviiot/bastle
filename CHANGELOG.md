# Changelog

All notable Bastle changes are recorded here. The format follows Keep a
Changelog and versions follow Semantic Versioning.

## [Unreleased]

## [0.5.1] - 2026-09-05

### Changed

- Embed the Chromium engine and its broker into the main Bastle Flatpak, so
  Chromium applications no longer require a separately installed package.
- Keep WebKitGTK as the default while presenting Chromium as a built-in,
  explicitly selected per-application engine.
- Move the Chromium runtime source into the main repository and activate it
  through an internal D-Bus service owned by `io.github.cheviiot.bastle`.
- Publish a small, GPG-signed, multi-architecture Flatpak repository through
  GitHub Pages while retaining direct bundles in GitHub Releases; no Flathub
  submission is planned.
- Clarify the `GPL-3.0-only` license, preserved Spider authorship, and Bastle's
  status as an independent continuation without endorsement by prior authors.
- Replace the project README with a compact Russian introduction and install
  guide.
- Replace the house-based icon with the Obsidian Glass identity built around
  three independent application windows, including a matching symbolic icon.
- Retire the idle Chromium broker automatically so Flatpak updates take effect
  without requiring a logout.

### Fixed

- Apply one 4096-byte User-Agent limit before saving and at both Chromium
  validation boundaries.
- Restore Chromium's default User-Agent when a custom value is cleared.
- Apply the selected User-Agent to Chromium popup and OAuth windows.
- Require an explicit allow decision before Chromium permission checks pass.
- Handle Electron's singular `mediaType` permission-check detail as well as
  the request handler's `mediaTypes` array.
- Persist Chromium window size and maximization through `AppService`.
- Close existing Chromium connections after changing an application's proxy.

### Security

- Preserve separate WebKit and Chromium profiles and safe Electron defaults
  without claiming a second Flatpak sandbox boundary.
- Keep the single Flatpak free of host filesystem access, `--device=all`, and
  Flatpak-control permissions.

## [0.5.0] - 2026-09-05

### Added

- `AppConfigV3` with an explicit `WebKit` or `Chromium` engine and an atomic
  v1/v2 migration that keeps existing Bastle applications on WebKitGTK.
- A local, release-pinned compatibility catalog that explains known WebKitGTK
  incompatibilities without telemetry or network updates.
- An authenticated, versioned D-Bus boundary for the separately sandboxed
  `bastle-chromium` companion, including delayed profile-deletion cleanup.
- Explicit Chromium recommendations during creation, editing, and recognized
  WebKitGTK load failures; the engine changes only after user confirmation.
- Live Dynamic Launcher, File Chooser, and Documents capability diagnostics,
  including interface versions and supported launcher types.

### Fixed

- Distinguish unavailable, unsupported, cancelled, denied, and failed portal
  outcomes without weakening create, repair, delete, or restore transactions.
- Cancel the underlying WebKit download when destination selection fails so a
  retry cannot retain an abandoned transfer.

### Security

- Keep WebKit and Chromium profiles separate, strip WebKit-only content filters
  from companion policy, and never silently fall back between engines.
- Reject Chromium site-data backup through the WebKit profile path so stale
  local data cannot be mislabeled as the companion profile.

## [0.4.0] - 2026-09-05

### Added

- `AppPolicyV2` with an atomic migration from v1 that preserves saved website
  permission decisions while leaving every new feature disabled by default.
- Optional top-level navigation allowlists with one-time, persistent,
  external-browser, and block actions for origins outside the list.
- Per-application WebKit proxy modes for system settings, direct connections,
  and normalized HTTP(S)/SOCKS endpoints without stored credentials.
- Per-application background and autostart choices authorized by the Background
  Portal, with system status notifications and an explicit Stop action.
- User-imported, WebKit-validated content-extension filters scoped to one web
  application and included in metadata backups.
- A documented threat model covering websites, profiles, portals, backups, and
  the future Chromium companion boundary.

### Changed

- Restore keeps permission, navigation, proxy, and content-filter policy while
  requiring explicit reauthorization for background mode and autostart.
- Background changes are serialized across processes and merged per field so
  concurrent editors cannot silently overwrite newer choices.

### Fixed

- Surface every background-startup failure instead of leaving an invisible
  process running, and correctly release background holds on notification
  activation.
- Preserve exact-policy backup matches and avoid duplicate applications during
  restore.

### Security

- Reject proxy URIs containing credentials, paths, queries, fragments, or
  unsupported schemes, and bound imported content-filter source data.
- Apply origin restrictions only to confirmed top-level response resources;
  subframes and ordinary third-party resources remain unaffected unless the
  user explicitly enables content filters.

## [0.3.0] - 2026-09-05

### Added

- Versioned `.bastle-backup` backup and restore with conflict preview,
  per-application transactional launcher installation, and partial-failure
  reporting.
- Optional passphrase-encrypted transfer of cookies and WebKit site storage
  using `age`.
- Runtime profile locks that prevent site-data backup while an application is
  running.
- Dynamic Launcher Portal capability diagnostics showing the active desktop,
  interface version, and supported launcher types.
- Native `aarch64` Flatpak CI and release bundles alongside `x86_64`.

### Security

- Reject backup archives containing absolute or traversing paths, links,
  special files, duplicate entries, unexpected files, excessive entry counts,
  or excessive uncompressed data.
- Run UI smoke tests in a dedicated D-Bus session and Xvfb display with the
  Wayland backend disabled.

## [0.2.0] - 2026-09-05

### Added

- Versioned per-application permission policy storage with atomic writes and
  origin normalization.
- Per-origin WebKit permission prompts with session-only or persistent choices,
  a permission editor, and safe rejection of unsupported request types.
- Native system notifications that return to the corresponding Bastle app.
- A download manager with destination selection, progress, cancellation,
  completed/failed states, and retry through the same isolated network session.
- Related WebKit popup windows for `window.open`, target-blank links, and OAuth
  flows, sharing the originating application's isolated network session.
- Discoverable reload, reload-without-cache, stop, home, bounded zoom, and
  fullscreen actions with keyboard shortcuts.

### Fixed

- Include the web-app window Blueprint in translation extraction and repair a
  malformed Russian AppStream translation.
- Build development and CI Flatpaks from the generated, offline Cargo source
  set instead of relying on live crates.io downloads.

### Changed

- Migrate existing Bastle application configurations from schema v1 to v2
  while preserving IDs, icons, profiles, and cache.

### Removed

- Spider settings import, first-run prompt, legacy parser, GSettings state, and
  all Spider-specific runtime service interfaces.

## [0.1.1] - 2026-09-05

### Fixed

- Prevent the application manager from aborting when an application card is
  opened for editing, and cover the editor with an Xvfb-only UI regression
  test.

## [0.1.0] - 2026-09-04

### Added

- Independent Bastle identity, application ID, icon, and English/Russian UI.
- Versioned per-application JSON repository with atomic writes and diagnostics.
- Transactional create/delete/repair service backed by Dynamic Launcher Portal.
- Explicit, idempotent, settings-only Spider importer with per-entry results.
- Bounded metadata and favicon fetching with normalized 256×256 PNG output.
- Fedora 44 Distrobox instructions, x86_64 GNOME 50 Flatpak build, CI, security
  audit, and tagged GitHub release workflow.

### Security

- HTTP(S)-only start URLs, desktop-file generation through GLib KeyFile, title
  sanitization, validated theme colors, sandboxed glycin decoding, and safe file
  URI downloads.

[Unreleased]: https://github.com/Cheviiot/bastle/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/Cheviiot/bastle/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/Cheviiot/bastle/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/Cheviiot/bastle/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Cheviiot/bastle/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Cheviiot/bastle/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/Cheviiot/bastle/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Cheviiot/bastle/releases/tag/v0.1.0

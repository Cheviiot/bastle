# Changelog

All notable Bastle changes are recorded here. The format follows Keep a
Changelog and versions follow Semantic Versioning.

## [Unreleased]

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

[Unreleased]: https://github.com/Cheviiot/bastle/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/Cheviiot/bastle/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/Cheviiot/bastle/releases/tag/v0.1.0

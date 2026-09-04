# Contributing to Bastle

Thank you for helping build Bastle. Please search existing issues before filing
a report and keep each pull request focused on one change.

## Workflow

1. Create a branch from `main`.
2. Work inside the documented `bastle-dev` Fedora 44 Distrobox.
3. Add tests for behavior changes.
4. Run the local gate:

   ```sh
   cargo fmt --check
   cargo check
   cargo clippy --all-targets -- -D warnings
   cargo test
   meson setup build
   meson compile -C build
   meson test -C build
   ```

5. Open a pull request. Changes are squash-merged after required checks pass.

Use English for code, identifiers, technical documentation, and commit
messages. User-facing strings must be translatable; update the Russian catalog
when adding UI text.

Do not add broad Flatpak permissions, a bundled browser engine, WebKit patches,
Chromium user-agent shims, or access to Spider's sandbox. Discuss any permission
change in an issue first.

By contributing, you agree that your contribution is provided under
`GPL-3.0-or-later`.

<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Chromium add-on for Bastle

This directory contains the Chromium engine published as the
`io.github.cheviiot.bastle.Chromium` Flatpak add-on. It is built from the same
repository and release as Bastle, but its large Electron payload is not stored
inside the main application ref.

The engine is activated on demand through the private
`io.github.cheviiot.bastle.Chromium` D-Bus service exported by Bastle. Flatpak
mounts the add-on at `/app/extensions/chromium`. WebKitGTK remains the default,
and Bastle uses Chromium only after an explicit user choice.

The C broker validates every request, serializes profile operations, and starts
one sandboxed Electron process per Bastle application ID. The JavaScript layer
uses `nodeIntegration=false`, `contextIsolation=true`, `sandbox=true`, and
`webSecurity=true`.

Run the non-rendered engine tests inside `bastle-dev`:

```sh
node chromium/tests/validate.test.js
node chromium/tests/navigation-policy.test.js
node chromium/tests/proxy.test.js
```

Rendered checks must use Xvfb with the Wayland socket disabled.

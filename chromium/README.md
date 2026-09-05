<!-- SPDX-License-Identifier: GPL-3.0-only -->

# Bastle Chromium engine

This directory contains the Chromium engine bundled into the main Bastle
Flatpak. It is an internal runtime component, not a separately installed or
user-facing application.

The engine is activated on demand through the private
`io.github.cheviiot.bastle.Chromium` D-Bus service exported by the same
`io.github.cheviiot.bastle` Flatpak. WebKitGTK remains the default, and Bastle
uses Chromium only after an explicit user choice.

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

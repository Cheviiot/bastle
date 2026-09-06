// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const {
  remoteSecurityDefaults,
  trustedShellSender,
  validateShellCommand,
} = require('../src/shell-security');

const remote = remoteSecurityDefaults();
assert.equal(remote.nodeIntegration, false);
assert.equal(remote.contextIsolation, true);
assert.equal(remote.sandbox, true);
assert.equal(remote.webSecurity, true);
assert.equal(remote.webviewTag, false);
assert.equal(Object.hasOwn(remote, 'preload'), false);

assert.deepEqual(validateShellCommand({ command: 'back' }), { command: 'back' });
assert.deepEqual(
  validateShellCommand({ command: 'toolbar-visibility', visible: false }),
  { command: 'toolbar-visibility', visible: false },
);
assert.deepEqual(validateShellCommand({ command: 'menu-open', visible: true }), {
  command: 'menu-open',
  visible: true,
});
for (const invalid of [
  null,
  'back',
  { command: 'open-url', url: 'file:///etc/passwd' },
  { command: 'back', visible: true },
  { command: 'toolbar-visibility' },
  { command: 'toolbar-visibility', visible: 'yes' },
]) assert.throws(() => validateShellCommand(invalid));

const shellSender = { getURL: () => 'bastle-ui://shell/shell.html' };
assert.equal(trustedShellSender(shellSender, shellSender), true);
assert.equal(trustedShellSender({ getURL: shellSender.getURL }, shellSender), false);
assert.equal(
  trustedShellSender({ getURL: () => 'https://example.org/' }, shellSender),
  false,
);

// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const { persistWindowState } = require('../src/window-state');

const invocations = [];
const window = {
  getNormalBounds() { return { width: 1440, height: 900 }; },
  isMaximized() { return true; },
};
persistWindowState(window, 'abcdefghijkl', (...args) => {
  invocations.push(args);
  return { status: 0 };
});

assert.deepEqual(invocations, [[
  '/app/bin/bastle',
  [
    '--save-chromium-window-state', 'abcdefghijkl',
    '--chromium-window-width', '1440',
    '--chromium-window-height', '900',
    '--chromium-window-maximized',
  ],
  { stdio: 'ignore', timeout: 5000 },
]]);
assert.throws(() => persistWindowState(window, 'abcdefghijkl', () => ({ status: 1 })));

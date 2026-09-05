// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const { applyUserAgent } = require('../src/user-agent');

const applied = [];
const webContents = { setUserAgent(value) { applied.push(value); } };
const browserSession = { getUserAgent() { return 'Default Chromium Agent'; } };

applyUserAgent(webContents, browserSession, 'Custom Agent');
applyUserAgent(webContents, browserSession, '');

assert.deepEqual(applied, ['Custom Agent', 'Default Chromium Agent']);

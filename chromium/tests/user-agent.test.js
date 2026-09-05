// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const { applyUserAgent } = require('../src/user-agent');

const sessionValues = [];
const contentsValues = [];
const browserSession = { setUserAgent(value) { sessionValues.push(value); } };
const webContents = { setUserAgent(value) { contentsValues.push(value); } };

applyUserAgent(browserSession, webContents, 'Custom Agent', 'Default Chromium Agent');
applyUserAgent(browserSession, webContents, '', 'Default Chromium Agent');

assert.deepEqual(sessionValues, ['Custom Agent', 'Default Chromium Agent']);
assert.deepEqual(contentsValues, ['Custom Agent', 'Default Chromium Agent']);

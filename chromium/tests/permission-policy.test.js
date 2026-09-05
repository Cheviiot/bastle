// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const { permissionAllowed } = require('../src/permission-policy');

const origin = 'https://example.org';
const kinds = ['camera', 'microphone'];

assert.equal(permissionAllowed(origin, kinds, {}, new Set()), false);
assert.equal(permissionAllowed(origin, kinds, { camera: 'allow' }, new Set()), false);
assert.equal(permissionAllowed(origin, kinds, { camera: 'allow', microphone: 'allow' },
  new Set()), true);
assert.equal(permissionAllowed(origin, kinds, {},
  new Set([`${origin}\0camera`, `${origin}\0microphone`])), true);
assert.equal(permissionAllowed(origin, kinds, { camera: 'block', microphone: 'allow' },
  new Set([`${origin}\0camera`])), false);

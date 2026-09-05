// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const { permissionAllowed, permissionKinds } = require('../src/permission-policy');

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

assert.deepEqual(permissionKinds('media', { mediaTypes: ['video', 'audio'] }), kinds);
assert.deepEqual(permissionKinds('media', { mediaType: 'video' }), ['camera']);
assert.deepEqual(permissionKinds('media', { mediaType: 'audio' }), ['microphone']);
assert.deepEqual(permissionKinds('media', { mediaType: 'unknown' }), []);
assert.deepEqual(permissionKinds('media', {}), []);
assert.deepEqual(permissionKinds('media', null), []);
assert.deepEqual(permissionKinds('geolocation'), ['geolocation']);
assert.deepEqual(permissionKinds('unknown'), []);

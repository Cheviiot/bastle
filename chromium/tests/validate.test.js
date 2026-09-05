// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const { validateConfig, validateOrigin, validatePolicy, webUrl } = require('../src/validate');

const policy = {
  schema_version: 2,
  permissions: { 'https://example.org': { notifications: 'allow' } },
  navigation: { enabled: true, allowed_origins: ['https://example.org'] },
  proxy: { mode: 'system', uri: null },
  background: { enabled: false, autostart: false },
  content_filters: {},
};
const config = {
  schema_version: 1,
  id: 'abcdefghijkl',
  url: 'https://example.org/',
  title: 'Example',
  user_agent: '',
  width: 1200,
  height: 800,
  maximized: false,
  start_in_background: false,
  policy,
};

assert.equal(validateConfig(structuredClone(config)).id, 'abcdefghijkl');
assert.equal(webUrl('https://example.org/path').origin, 'https://example.org');
assert.throws(() => webUrl('file:///etc/passwd'));
assert.throws(() => webUrl('//example.org'));
assert.throws(() => validateOrigin('https://example.org/path'));
assert.throws(() => validateConfig({ ...config, id: '../bad-value' }));
assert.throws(() => validateConfig({ ...config, title: 'Bad\nTitle' }));
assert.equal(validateConfig({ ...config, title: '🏠'.repeat(512) }).title, '🏠'.repeat(512));
assert.throws(() => validateConfig({ ...config, title: '🏠'.repeat(513) }));
assert.throws(() => validatePolicy({ ...policy, schema_version: 99 }));
assert.throws(() => validatePolicy({
  ...policy,
  permissions: { 'https://example.org': { unknown: 'allow' } },
}));
assert.throws(() => validatePolicy({
  ...policy,
  proxy: { mode: 'custom', uri: 'http://user:secret@example.org' },
}));

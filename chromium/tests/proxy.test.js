// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const assert = require('node:assert/strict');
const { configureProxy } = require('../src/proxy');

async function exercise(proxy, expected) {
  const calls = [];
  const browserSession = {
    async setProxy(value) { calls.push(['setProxy', value]); },
    async closeAllConnections() { calls.push(['closeAllConnections']); },
  };
  await configureProxy(browserSession, proxy);
  assert.deepEqual(calls, [['setProxy', expected], ['closeAllConnections']]);
}

(async () => {
  await exercise({ mode: 'system', uri: null }, { mode: 'system' });
  await exercise({ mode: 'no_proxy', uri: null }, { mode: 'direct' });
  await exercise(
    { mode: 'custom', uri: 'socks5://127.0.0.1:9050' },
    { mode: 'fixed_servers', proxyRules: 'socks5://127.0.0.1:9050' },
  );
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

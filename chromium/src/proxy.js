// SPDX-License-Identifier: GPL-3.0-only
'use strict';

async function configureProxy(browserSession, proxy) {
  if (proxy.mode === 'no_proxy') await browserSession.setProxy({ mode: 'direct' });
  else if (proxy.mode === 'custom') await browserSession.setProxy({
    mode: 'fixed_servers', proxyRules: proxy.uri,
  });
  else await browserSession.setProxy({ mode: 'system' });

  // Electron may otherwise reuse sockets opened with the previous policy when
  // a running application receives an updated configuration.
  await browserSession.closeAllConnections();
}

module.exports = { configureProxy };

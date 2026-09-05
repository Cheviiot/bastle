// SPDX-License-Identifier: GPL-3.0-only
'use strict';

function applyUserAgent(browserSession, webContents, configuredUserAgent, defaultUserAgent) {
  const effectiveUserAgent = configuredUserAgent || defaultUserAgent;
  browserSession.setUserAgent(effectiveUserAgent);
  webContents.setUserAgent(effectiveUserAgent);
}

module.exports = { applyUserAgent };

// SPDX-License-Identifier: GPL-3.0-only
'use strict';

function applyUserAgent(webContents, browserSession, configuredUserAgent) {
  webContents.setUserAgent(configuredUserAgent || browserSession.getUserAgent());
}

module.exports = { applyUserAgent };

// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const COMMANDS = new Set([
  'back',
  'forward',
  'reload',
  'reload-bypass-cache',
  'stop',
  'home',
  'zoom-in',
  'zoom-out',
  'zoom-reset',
  'toggle-fullscreen',
  'quit',
  'toolbar-visibility',
  'menu-open',
]);

function remoteSecurityDefaults() {
  return {
    nodeIntegration: false,
    nodeIntegrationInWorker: false,
    nodeIntegrationInSubFrames: false,
    contextIsolation: true,
    sandbox: true,
    webSecurity: true,
    allowRunningInsecureContent: false,
    webviewTag: false,
    devTools: false,
  };
}

function validateShellCommand(message) {
  if (!message || typeof message !== 'object' || Array.isArray(message)) {
    throw new TypeError('invalid shell command');
  }
  const keys = Object.keys(message);
  if (!keys.includes('command') || keys.some((key) => !['command', 'visible'].includes(key))) {
    throw new TypeError('invalid shell command fields');
  }
  if (typeof message.command !== 'string' || !COMMANDS.has(message.command)) {
    throw new TypeError('unsupported shell command');
  }
  if (message.command === 'toolbar-visibility' || message.command === 'menu-open') {
    if (typeof message.visible !== 'boolean' || keys.length !== 2) {
      throw new TypeError('invalid toolbar visibility');
    }
  } else if (keys.length !== 1) {
    throw new TypeError('unexpected shell command argument');
  }
  return message;
}

function trustedShellSender(sender, expected) {
  if (!sender || sender !== expected || typeof sender.getURL !== 'function') return false;
  try {
    const url = new URL(sender.getURL());
    return url.protocol === 'bastle-ui:' && url.hostname === 'shell';
  } catch {
    return false;
  }
}

module.exports = { remoteSecurityDefaults, trustedShellSender, validateShellCommand };

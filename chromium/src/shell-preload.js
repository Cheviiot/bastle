// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('bastleShell', Object.freeze({
  command(command, visible) {
    const message = ['toolbar-visibility', 'menu-open'].includes(command)
      ? { command, visible }
      : { command };
    return ipcRenderer.invoke('bastle:shell-command', message);
  },
  onState(callback) {
    if (typeof callback !== 'function') return;
    ipcRenderer.on('bastle:shell-state', (_event, state) => callback(state));
    ipcRenderer.on('bastle:shell-reveal', () => callback({ reveal: true }));
  },
}));

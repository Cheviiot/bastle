// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const { spawnSync } = require('node:child_process');

function persistWindowState(window, id, run = spawnSync) {
  const bounds = window.getNormalBounds();
  const args = [
    '--save-chromium-window-state', id,
    '--chromium-window-width', String(bounds.width),
    '--chromium-window-height', String(bounds.height),
  ];
  if (window.isMaximized()) args.push('--chromium-window-maximized');
  const result = run('/app/bin/bastle', args, { stdio: 'ignore', timeout: 5000 });
  if (result.error || result.status !== 0) {
    throw result.error || new Error(`Bastle exited with status ${result.status}`);
  }
}

module.exports = { persistWindowState };

// SPDX-License-Identifier: GPL-3.0-only
'use strict';

function permissionAllowed(origin, kinds, decisions, sessionAllows) {
  return kinds.every((kind) => decisions[kind] !== 'block' &&
    (decisions[kind] === 'allow' || sessionAllows.has(`${origin}\0${kind}`)));
}

module.exports = { permissionAllowed };

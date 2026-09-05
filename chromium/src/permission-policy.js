// SPDX-License-Identifier: GPL-3.0-only
'use strict';

function permissionKinds(permission, details = {}) {
  if (!details || typeof details !== 'object') details = {};
  switch (permission) {
    case 'media': {
      const requested = new Set([
        ...(Array.isArray(details.mediaTypes) ? details.mediaTypes : []),
        ...(typeof details.mediaType === 'string' ? [details.mediaType] : []),
      ]);
      return [
        ...(requested.has('video') ? ['camera'] : []),
        ...(requested.has('audio') ? ['microphone'] : []),
      ];
    }
    case 'geolocation': return ['geolocation'];
    case 'notifications': return ['notifications'];
    case 'clipboard-read': return ['clipboard'];
    case 'pointerLock': return ['pointer_lock'];
    case 'storage-access': return ['third_party_storage'];
    default: return [];
  }
}

function permissionAllowed(origin, kinds, decisions, sessionAllows) {
  return kinds.every((kind) => decisions[kind] !== 'block' &&
    (decisions[kind] === 'allow' || sessionAllows.has(`${origin}\0${kind}`)));
}

module.exports = { permissionAllowed, permissionKinds };

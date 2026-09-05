// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const APP_ID = /^[a-z0-9]{12}$/;
const PERMISSION_KINDS = new Set([
  'camera', 'microphone', 'geolocation', 'notifications', 'clipboard',
  'pointer_lock', 'third_party_storage',
]);
const DECISIONS = new Set(['ask', 'allow', 'block']);
const PROXY_MODES = new Set(['system', 'no_proxy', 'custom']);

function webUrl(value) {
  const parsed = new URL(value);
  if (!['http:', 'https:'].includes(parsed.protocol) || !parsed.hostname) {
    throw new Error('start URL must use HTTP or HTTPS and include a host');
  }
  return parsed;
}

function validateOrigin(value) {
  const parsed = webUrl(value);
  if (parsed.origin !== value) throw new Error(`origin is not normalized: ${value}`);
}

function validatePolicy(policy) {
  if (!policy || typeof policy !== 'object' || Array.isArray(policy) ||
      policy.schema_version !== 2) {
    throw new Error('unsupported Bastle policy');
  }
  const permissions = policy.permissions || {};
  for (const [origin, choices] of Object.entries(permissions)) {
    validateOrigin(origin);
    if (!choices || typeof choices !== 'object' || Array.isArray(choices)) {
      throw new Error('invalid permission decisions');
    }
    for (const [kind, decision] of Object.entries(choices)) {
      if (!PERMISSION_KINDS.has(kind) || !DECISIONS.has(decision)) {
        throw new Error('unknown permission kind or decision');
      }
    }
  }
  const navigation = policy.navigation || { enabled: false, allowed_origins: [] };
  if (typeof navigation.enabled !== 'boolean' ||
      !Array.isArray(navigation.allowed_origins)) throw new Error('invalid navigation policy');
  navigation.allowed_origins.forEach(validateOrigin);
  const proxy = policy.proxy || { mode: 'system', uri: null };
  if (!PROXY_MODES.has(proxy.mode)) throw new Error('invalid proxy mode');
  if (proxy.mode === 'custom') {
    const uri = new URL(proxy.uri);
    if (!['http:', 'https:', 'socks:', 'socks4:', 'socks4a:', 'socks5:'].includes(uri.protocol) ||
        !uri.hostname || uri.username || uri.password || uri.pathname !== '/' ||
        uri.search || uri.hash) throw new Error('invalid custom proxy');
  } else if (proxy.uri !== null && proxy.uri !== undefined) {
    throw new Error('proxy URI is valid only in custom mode');
  }
  const background = policy.background || { enabled: false, autostart: false };
  if (typeof background.enabled !== 'boolean' || typeof background.autostart !== 'boolean' ||
      (!background.enabled && background.autostart)) throw new Error('invalid background policy');
  return { ...policy, permissions, navigation, proxy, background };
}

function validateConfig(config) {
  if (!config || typeof config !== 'object' || config.schema_version !== 1 ||
      !APP_ID.test(config.id) || typeof config.title !== 'string' || !config.title.trim() ||
      /[\u0000-\u001f\u007f]/u.test(config.title) || [...config.title].length > 512 ||
      typeof config.user_agent !== 'string' || /[\r\n]/u.test(config.user_agent) ||
      config.user_agent.length > 4096 || !Number.isInteger(config.width) ||
      config.width < 320 || config.width > 8192 || !Number.isInteger(config.height) ||
      config.height < 200 || config.height > 8192 ||
      typeof config.maximized !== 'boolean' ||
      typeof config.start_in_background !== 'boolean') {
    throw new Error('invalid Chromium engine runtime request');
  }
  webUrl(config.url);
  config.policy = validatePolicy(config.policy);
  return config;
}

module.exports = { validateConfig, validateOrigin, validatePolicy, webUrl };

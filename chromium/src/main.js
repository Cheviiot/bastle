// SPDX-License-Identifier: GPL-3.0-or-later
'use strict';

const fs = require('node:fs');
const net = require('node:net');
const path = require('node:path');
const { app, BrowserWindow, dialog, Menu, Notification, session, shell } = require('electron');
const {
  quarantinePendingPopupNavigation,
  wireNavigationPolicy,
} = require('./navigation-policy');
const { validateConfig, webUrl } = require('./validate');

function readRequest() {
  const prefix = '--bastle-config=';
  const argument = process.argv.find((value) => value.startsWith(prefix));
  if (!argument) throw new Error('missing private runtime request');
  const filename = argument.slice(prefix.length);
  const runtimeRoot = path.resolve(process.env.XDG_RUNTIME_DIR, 'bastle-chromium');
  const resolved = path.resolve(filename);
  if (!resolved.startsWith(`${runtimeRoot}${path.sep}`)) throw new Error('unsafe request path');
  const stats = fs.lstatSync(resolved);
  if (!stats.isFile() || stats.isSymbolicLink() || stats.size > 33 * 1024 * 1024) {
    throw new Error('invalid runtime request file');
  }
  try {
    return validateConfig(JSON.parse(fs.readFileSync(resolved, 'utf8')));
  } finally {
    fs.unlinkSync(resolved);
  }
}

let current = readRequest();
const profileRoot = path.join(process.env.XDG_DATA_HOME, 'bastle-chromium', 'profiles', current.id);
const cacheRoot = path.join(process.env.XDG_CACHE_HOME, 'bastle-chromium', current.id);
fs.mkdirSync(profileRoot, { recursive: true, mode: 0o700 });
fs.mkdirSync(cacheRoot, { recursive: true, mode: 0o700 });
app.setPath('userData', profileRoot);
app.setPath('sessionData', profileRoot);
app.setPath('cache', cacheRoot);
const gotLock = app.requestSingleInstanceLock(current);
if (!gotLock) app.exit(0);

let mainWindow = null;
let runtimeServer = null;
const sessionAllows = new Set();
const pendingPopupNavigations = new WeakMap();

function originOf(value) {
  return webUrl(value).origin;
}

function navigationAllowed(value) {
  if (!current.policy.navigation.enabled) return true;
  const parsed = new URL(value);
  if (['about:', 'blob:'].includes(parsed.protocol)) return true;
  return current.policy.navigation.allowed_origins.includes(originOf(value));
}

async function promptNavigation(value, parentContents = mainWindow?.webContents) {
  const parentWindow = parentContents ? BrowserWindow.fromWebContents(parentContents) : mainWindow;
  const result = await dialog.showMessageBox(parentWindow, {
    type: 'question',
    title: 'Open another origin?',
    message: 'This address is outside the application navigation allowlist.',
    detail: value,
    buttons: ['Block', 'Open Externally', 'Open Once'],
    defaultId: 0,
    cancelId: 0,
  });
  if (result.response === 1) await shell.openExternal(value);
  return result.response === 2;
}

async function handleNavigation(event, value) {
  if (quarantinePendingPopupNavigation(pendingPopupNavigations, event)) return;
  try {
    if (navigationAllowed(value)) return;
  } catch {
    event.preventDefault();
    return;
  }
  event.preventDefault();
  if (await promptNavigation(value, event.sender)) {
    if (!event.sender.isDestroyed()) await event.sender.loadURL(value);
  }
}

function handleFrameNavigation(event) {
  quarantinePendingPopupNavigation(pendingPopupNavigations, event);
}

function permissionKinds(permission, details) {
  switch (permission) {
    case 'media': {
      const requested = details.mediaTypes || [];
      return [
        ...(requested.includes('video') ? ['camera'] : []),
        ...(requested.includes('audio') ? ['microphone'] : []),
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

async function askPermission(origin, kinds) {
  const result = await dialog.showMessageBox(mainWindow, {
    type: 'question',
    title: 'Website permission',
    message: `Allow ${kinds.join(' and ')} for this session?`,
    detail: origin,
    buttons: ['Not Now', 'Allow for This Session'],
    defaultId: 0,
    cancelId: 0,
  });
  return result.response === 1;
}

function configurePermissions(browserSession) {
  browserSession.setPermissionRequestHandler(async (webContents, permission, callback, details) => {
    const kinds = permissionKinds(permission, details);
    if (!kinds.length) {
      console.error(`Denied unsupported website permission: ${permission}`);
      callback(false);
      return;
    }
    let origin;
    try { origin = originOf(details.requestingUrl || webContents.getURL()); }
    catch { callback(false); return; }
    const decisions = current.policy.permissions[origin] || {};
    if (kinds.some((kind) => decisions[kind] === 'block')) { callback(false); return; }
    if (kinds.every((kind) => decisions[kind] === 'allow' ||
        sessionAllows.has(`${origin}\0${kind}`))) { callback(true); return; }
    const allowed = await askPermission(origin, kinds);
    if (allowed) kinds.forEach((kind) => sessionAllows.add(`${origin}\0${kind}`));
    callback(allowed);
  });
  browserSession.setPermissionCheckHandler((_webContents, permission, requestingOrigin, details) => {
    const kinds = permissionKinds(permission, details || {});
    if (!kinds.length) return false;
    let origin;
    try { origin = originOf(requestingOrigin); } catch { return false; }
    const decisions = current.policy.permissions[origin] || {};
    return kinds.every((kind) => decisions[kind] !== 'block');
  });
}

async function configureProxy(browserSession) {
  const proxy = current.policy.proxy;
  if (proxy.mode === 'no_proxy') await browserSession.setProxy({ mode: 'direct' });
  else if (proxy.mode === 'custom') await browserSession.setProxy({
    mode: 'fixed_servers', proxyRules: proxy.uri,
  });
  else await browserSession.setProxy({ mode: 'system' });
}

function configureDownloads(browserSession) {
  browserSession.on('will-download', (_event, item) => {
    const destination = dialog.showSaveDialogSync(mainWindow, {
      defaultPath: path.join(app.getPath('downloads'), path.basename(item.getFilename())),
    });
    if (!destination) { item.cancel(); return; }
    item.setSavePath(destination);
    item.once('done', (_downloadEvent, state) => {
      if (Notification.isSupported()) new Notification({
        title: state === 'completed' ? 'Download complete' : 'Download failed',
        body: path.basename(destination),
      }).show();
    });
  });
}

function safePopup(details, openerContents) {
  let target;
  try { target = new URL(details.url); } catch { return { action: 'deny' }; }
  if (['mailto:', 'tel:'].includes(target.protocol)) {
    shell.openExternal(details.url);
    return { action: 'deny' };
  }
  if (!['http:', 'https:', 'about:', 'blob:'].includes(target.protocol)) {
    return { action: 'deny' };
  }
  const requiresApproval = ['http:', 'https:'].includes(target.protocol) &&
    !navigationAllowed(details.url);
  return {
    action: 'allow',
    overrideBrowserWindowOptions: {
      autoHideMenuBar: true,
      show: !requiresApproval,
      webPreferences: safeWebPreferences(),
    },
  };
}

function handleCreatedPopup(window, details, openerContents) {
  let requiresApproval = false;
  try {
    const target = new URL(details.url);
    requiresApproval = ['http:', 'https:'].includes(target.protocol) &&
      !navigationAllowed(details.url);
  } catch {
    window.close();
    return;
  }
  if (!requiresApproval) return;

  const contents = window.webContents;
  pendingPopupNavigations.set(contents, true);
  contents.stop();
  window.hide();
  contents.loadURL('about:blank').catch((error) => console.error(error));
  promptNavigation(details.url, openerContents)
    .then(async (approved) => {
      pendingPopupNavigations.delete(contents);
      if (window.isDestroyed()) return;
      if (approved) {
        await contents.loadURL(details.url);
        if (window.isDestroyed()) return;
        window.show();
        window.focus();
      } else {
        window.close();
      }
    })
    .catch((error) => {
      pendingPopupNavigations.delete(contents);
      if (!window.isDestroyed()) window.close();
      console.error(error);
    });
}

let isolatedSession;
function safeWebPreferences() {
  return {
    session: isolatedSession,
    nodeIntegration: false,
    nodeIntegrationInWorker: false,
    nodeIntegrationInSubFrames: false,
    contextIsolation: true,
    sandbox: true,
    webSecurity: true,
    allowRunningInsecureContent: false,
    webviewTag: false,
  };
}

function listenRuntimeSocket(socketPath) {
  return new Promise((resolve, reject) => {
    const server = net.createServer((socket) => socket.destroy());
    server.once('error', reject);
    server.once('listening', () => {
      server.removeListener('error', reject);
      server.on('error', (error) => {
        console.error('Runtime profile lock failed:', error);
        app.exit(1);
      });
      resolve(server);
    });
    server.listen(socketPath);
  });
}

async function createWindow(config) {
  current = validateConfig(config);
  if (!mainWindow || mainWindow.isDestroyed()) {
    mainWindow = new BrowserWindow({
      title: current.title,
      width: current.width,
      height: current.height,
      show: !current.start_in_background,
      autoHideMenuBar: true,
      webPreferences: safeWebPreferences(),
    });
    if (current.maximized) mainWindow.maximize();
    wireNavigationPolicy(
      mainWindow.webContents,
      handleNavigation,
      safePopup,
      handleCreatedPopup,
      handleFrameNavigation,
    );
    mainWindow.on('close', (event) => {
      if (current.policy.background.enabled && !app.isQuitting) {
        event.preventDefault();
        mainWindow.hide();
        if (Notification.isSupported()) {
          const notification = new Notification({
            title: `${current.title} is running in background`,
            body: 'Click to show the application. Quit from the application menu to stop it.',
          });
          notification.on('click', () => { mainWindow.show(); mainWindow.focus(); });
          notification.show();
        }
      }
    });
  }
  if (current.user_agent) mainWindow.webContents.setUserAgent(current.user_agent);
  await configureProxy(isolatedSession);
  await mainWindow.loadURL(current.url).catch((error) => console.error(error));
  if (!current.start_in_background) { mainWindow.show(); mainWindow.focus(); }
  else if (Notification.isSupported()) {
    const notification = new Notification({
      title: `${current.title} is running in background`,
      body: 'Click to show the application. Use Stop in the application menu to end it.',
    });
    notification.on('click', () => { mainWindow.show(); mainWindow.focus(); });
    notification.show();
  }
}

if (gotLock) {
  app.on('second-instance', (_event, _argv, _cwd, additionalData) => {
    createWindow(additionalData).catch((error) => console.error(error));
  });
  app.on('before-quit', () => { app.isQuitting = true; });
  app.on('window-all-closed', () => app.quit());
  app.whenReady().then(async () => {
    const runtimeSocket = path.join(
      process.env.XDG_RUNTIME_DIR, 'bastle-chromium', `${current.id}.sock`,
    );
    fs.mkdirSync(path.dirname(runtimeSocket), { recursive: true, mode: 0o700 });
    try { fs.unlinkSync(runtimeSocket); } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    runtimeServer = await listenRuntimeSocket(runtimeSocket);
    app.once('will-quit', () => {
      runtimeServer?.close();
      try { fs.unlinkSync(runtimeSocket); } catch (error) {
        if (error.code !== 'ENOENT') console.error(error);
      }
    });
    Menu.setApplicationMenu(Menu.buildFromTemplate([{
      label: 'Application',
      submenu: [
        { label: 'Show', click: () => { mainWindow?.show(); mainWindow?.focus(); } },
        { label: 'Stop', accelerator: 'CmdOrCtrl+Q', click: () => {
          app.isQuitting = true;
          app.quit();
        } },
      ],
    }]));
    isolatedSession = session.fromPartition(`persist:bastle-${current.id}`, { cache: true });
    configurePermissions(isolatedSession);
    configureDownloads(isolatedSession);
    await createWindow(current);
  }).catch((error) => {
    console.error(error);
    app.exit(1);
  });
}

// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const fs = require('node:fs');
const nodeNet = require('node:net');
const path = require('node:path');
const {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
  Menu,
  Notification,
  protocol,
  session,
  shell,
  WebContentsView,
} = require('electron');
const {
  quarantinePendingPopupNavigation,
  wireNavigationPolicy,
} = require('./navigation-policy');
const { permissionAllowed, permissionKinds } = require('./permission-policy');
const { configureProxy } = require('./proxy');
const {
  remoteSecurityDefaults,
  trustedShellSender,
  validateShellCommand,
} = require('./shell-security');
const { applyUserAgent } = require('./user-agent');
const { validateConfig, webUrl } = require('./validate');
const { persistWindowState } = require('./window-state');

const SHELL_ORIGIN = 'bastle-ui://shell';
const TOOLBAR_HEIGHT = 52;
const HOT_ZONE_HEIGHT = 6;
const MENU_HEIGHT = 390;
const SHELL_CSP = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'";
const SHELL_ASSETS = new Map([
  ['/shell.html', ['shell.html', 'text/html; charset=utf-8']],
  ['/shell.css', ['shell.css', 'text/css; charset=utf-8']],
  ['/shell.js', ['shell.js', 'text/javascript; charset=utf-8']],
]);

function isRussianLocale() {
  return app.getLocale().toLowerCase().startsWith('ru');
}

function shellStrings() {
  if (isRussianLocale()) {
    return {
      lang: 'ru',
      back: 'Назад',
      forward: 'Вперёд',
      menu: 'Меню',
      reload: 'Обновить',
      reloadBypassCache: 'Обновить без кэша',
      stop: 'Остановить загрузку',
      home: 'Главная',
      zoomIn: 'Увеличить масштаб',
      zoomOut: 'Уменьшить масштаб',
      zoomReset: 'Сбросить масштаб',
      fullscreen: 'Полный экран',
      quit: 'Остановить приложение',
    };
  }
  return {
    lang: 'en',
    back: 'Back',
    forward: 'Forward',
    menu: 'Menu',
    reload: 'Reload',
    reloadBypassCache: 'Reload without cache',
    stop: 'Stop loading',
    home: 'Home',
    zoomIn: 'Zoom in',
    zoomOut: 'Zoom out',
    zoomReset: 'Reset zoom',
    fullscreen: 'Fullscreen',
    quit: 'Stop application',
  };
}

function nativeStrings() {
  if (isRussianLocale()) {
    return {
      navigationTitle: 'Открыть другой источник?',
      navigationMessage: 'Этот адрес находится за пределами списка разрешённой навигации.',
      block: 'Заблокировать',
      openExternally: 'Открыть внешне',
      openOnce: 'Открыть один раз',
      permissionTitle: 'Разрешение сайта',
      permissionMessage: (kinds) => `Разрешить ${kinds.join(' и ')} на этот сеанс?`,
      notNow: 'Не сейчас',
      allowSession: 'Разрешить на этот сеанс',
      downloadComplete: 'Загрузка завершена',
      downloadFailed: 'Ошибка загрузки',
      backgroundTitle: (title) => `${title} работает в фоне`,
      backgroundBody: 'Нажмите, чтобы показать приложение. Для остановки выйдите через меню приложения.',
      backgroundStopBody: 'Нажмите, чтобы показать приложение. Для остановки используйте пункт «Остановить» в меню.',
    };
  }
  return {
    navigationTitle: 'Open another origin?',
    navigationMessage: 'This address is outside the application navigation allowlist.',
    block: 'Block',
    openExternally: 'Open Externally',
    openOnce: 'Open Once',
    permissionTitle: 'Website permission',
    permissionMessage: (kinds) => `Allow ${kinds.join(' and ')} for this session?`,
    notNow: 'Not Now',
    allowSession: 'Allow for This Session',
    downloadComplete: 'Download complete',
    downloadFailed: 'Download failed',
    backgroundTitle: (title) => `${title} is running in background`,
    backgroundBody: 'Click to show the application. Quit from the application menu to stop it.',
    backgroundStopBody: 'Click to show the application. Use Stop in the application menu to end it.',
  };
}

protocol.registerSchemesAsPrivileged([{
  scheme: 'bastle-ui',
  privileges: { standard: true, secure: true, supportFetchAPI: false, corsEnabled: false },
}]);

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

let mainRuntime = null;
let runtimeServer = null;
let isolatedSession;
let defaultUserAgent;
const runtimeWindows = new Set();
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

function runtimeForSite(contents) {
  return [...runtimeWindows].find((runtime) => runtime.siteView.webContents === contents);
}

function runtimeForShell(contents) {
  return [...runtimeWindows].find((runtime) => runtime.shellView.webContents === contents);
}

function setDialogOpen(runtime, open) {
  if (!runtime || runtime.window.isDestroyed()) return;
  runtime.dialogOpen = open;
  sendShellState(runtime);
}

function revealToolbar(runtime) {
  if (!runtime || runtime.shellView.webContents.isDestroyed()) return;
  runtime.toolbarVisible = true;
  updateRuntimeBounds(runtime);
  if (runtime.shellReady) runtime.shellView.webContents.send('bastle:shell-reveal');
}

async function promptNavigation(value, parentContents = mainRuntime?.siteView.webContents) {
  const runtime = runtimeForSite(parentContents) || mainRuntime;
  setDialogOpen(runtime, true);
  try {
    const text = nativeStrings();
    const options = {
      type: 'question',
      title: text.navigationTitle,
      message: text.navigationMessage,
      detail: value,
      buttons: [text.block, text.openExternally, text.openOnce],
      defaultId: 0,
      cancelId: 0,
    };
    const result = runtime
      ? await dialog.showMessageBox(runtime.window, options)
      : await dialog.showMessageBox(options);
    if (result.response === 1) await shell.openExternal(value);
    return result.response === 2;
  } finally {
    setDialogOpen(runtime, false);
  }
}

async function handleNavigation(event, value) {
  if (quarantinePendingPopupNavigation(pendingPopupNavigations, event)) return;
  try {
    if (navigationAllowed(value)) return;
  } catch {
    event.preventDefault();
    revealToolbar(runtimeForSite(event.sender));
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

async function askPermission(origin, kinds, contents) {
  const runtime = runtimeForSite(contents) || mainRuntime;
  setDialogOpen(runtime, true);
  try {
    const text = nativeStrings();
    const options = {
      type: 'question',
      title: text.permissionTitle,
      message: text.permissionMessage(kinds),
      detail: origin,
      buttons: [text.notNow, text.allowSession],
      defaultId: 0,
      cancelId: 0,
    };
    const result = runtime
      ? await dialog.showMessageBox(runtime.window, options)
      : await dialog.showMessageBox(options);
    return result.response === 1;
  } finally {
    setDialogOpen(runtime, false);
  }
}

function configurePermissions(browserSession) {
  browserSession.setPermissionRequestHandler(async (webContents, permission, callback, details) => {
    const kinds = permissionKinds(permission, details);
    if (!kinds.length) {
      console.error(`Denied unsupported website permission: ${permission}`);
      revealToolbar(runtimeForSite(webContents));
      callback(false);
      return;
    }
    let origin;
    try { origin = originOf(details.requestingUrl || webContents.getURL()); }
    catch { callback(false); return; }
    const decisions = current.policy.permissions[origin] || {};
    if (kinds.some((kind) => decisions[kind] === 'block')) { callback(false); return; }
    if (permissionAllowed(origin, kinds, decisions, sessionAllows)) {
      callback(true);
      return;
    }
    const allowed = await askPermission(origin, kinds, webContents);
    if (allowed) kinds.forEach((kind) => sessionAllows.add(`${origin}\0${kind}`));
    callback(allowed);
  });
  browserSession.setPermissionCheckHandler((_webContents, permission, requestingOrigin, details) => {
    const kinds = permissionKinds(permission, details || {});
    if (!kinds.length) return false;
    let origin;
    try { origin = originOf(requestingOrigin); } catch { return false; }
    const decisions = current.policy.permissions[origin] || {};
    return permissionAllowed(origin, kinds, decisions, sessionAllows);
  });
}

function configureDownloads(browserSession) {
  browserSession.on('will-download', (_event, item, webContents) => {
    const runtime = runtimeForSite(webContents) || mainRuntime;
    revealToolbar(runtime);
    setDialogOpen(runtime, true);
    const options = {
      defaultPath: path.join(app.getPath('downloads'), path.basename(item.getFilename())),
    };
    const destination = runtime
      ? dialog.showSaveDialogSync(runtime.window, options)
      : dialog.showSaveDialogSync(options);
    setDialogOpen(runtime, false);
    if (!destination) { item.cancel(); return; }
    item.setSavePath(destination);
    item.once('done', (_downloadEvent, state) => {
      revealToolbar(runtime);
      if (Notification.isSupported()) new Notification({
        title: state === 'completed' ? nativeStrings().downloadComplete : nativeStrings().downloadFailed,
        body: path.basename(destination),
      }).show();
    });
  });
}

function safeWebPreferences() {
  return { session: isolatedSession, ...remoteSecurityDefaults() };
}

function shellWebPreferences() {
  return {
    ...remoteSecurityDefaults(),
    preload: path.join(__dirname, 'shell-preload.js'),
  };
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
  let requiresApproval = false;
  try {
    requiresApproval = ['http:', 'https:'].includes(target.protocol) &&
      !navigationAllowed(details.url);
  } catch {
    return { action: 'deny' };
  }
  return {
    action: 'allow',
    overrideBrowserWindowOptions: {
      show: false,
      autoHideMenuBar: true,
      webPreferences: safeWebPreferences(),
    },
    createWindow(options) {
      const runtime = createRuntimeWindow({
        title: current.title,
        width: Math.max(320, Number(options.width) || 720),
        height: Math.max(240, Number(options.height) || 640),
        show: !requiresApproval,
        isMain: false,
        adoptedContents: options.webContents,
      });
      const contents = runtime.siteView.webContents;
      applyUserAgent(isolatedSession, contents, current.user_agent, defaultUserAgent);
      if (requiresApproval) {
        pendingPopupNavigations.set(contents, true);
        promptNavigation(details.url, openerContents)
          .then(async (approved) => {
            pendingPopupNavigations.delete(contents);
            if (runtime.window.isDestroyed()) return;
            if (!approved) { runtime.window.close(); return; }
            await contents.loadURL(details.url);
            runtime.window.show();
            runtime.window.focus();
          })
          .catch((error) => {
            pendingPopupNavigations.delete(contents);
            if (!runtime.window.isDestroyed()) runtime.window.close();
            console.error(error);
          });
      } else if (details.disposition === 'background-tab') {
        contents.loadURL(details.url).catch((error) => console.error(error));
      }
      return contents;
    },
  };
}

function sendShellState(runtime) {
  const contents = runtime.siteView.webContents;
  const shellContents = runtime.shellView.webContents;
  if (!runtime.shellReady || shellContents.isDestroyed() || contents.isDestroyed()) return;
  const history = contents.navigationHistory;
  shellContents.send('bastle:shell-state', {
    title: contents.getTitle() || current.title,
    canGoBack: history.canGoBack(),
    canGoForward: history.canGoForward(),
    loading: contents.isLoading(),
    progress: contents.isLoading() ? 0.2 : 1,
    dialogOpen: runtime.dialogOpen,
    strings: shellStrings(),
  });
}

function updateRuntimeBounds(runtime) {
  if (runtime.window.isDestroyed()) return;
  const { width, height } = runtime.window.getContentBounds();
  runtime.siteView.setBounds({ x: 0, y: 0, width, height });
  const shellHeight = runtime.menuOpen
    ? Math.min(height, MENU_HEIGHT)
    : runtime.toolbarVisible ? Math.min(height, TOOLBAR_HEIGHT) : Math.min(height, HOT_ZONE_HEIGHT);
  runtime.shellView.setBounds({ x: 0, y: 0, width, height: shellHeight });
}

function wireRuntimeSite(runtime) {
  const contents = runtime.siteView.webContents;
  wireNavigationPolicy(contents, handleNavigation, safePopup, () => {}, handleFrameNavigation);
  for (const event of ['did-start-loading', 'did-stop-loading', 'did-navigate', 'did-navigate-in-page']) {
    contents.on(event, () => {
      if (event === 'did-start-loading') revealToolbar(runtime);
      sendShellState(runtime);
    });
  }
  contents.on('did-fail-load', () => {
    revealToolbar(runtime);
    sendShellState(runtime);
  });
  contents.on('page-title-updated', (event, title) => {
    event.preventDefault();
    runtime.window.setTitle(title || current.title);
    sendShellState(runtime);
  });
  contents.on('before-input-event', (_event, input) => {
    if (input.type === 'keyDown' && ['Alt', 'F10'].includes(input.key)) revealToolbar(runtime);
  });
}

function createRuntimeWindow({ title, width, height, show, isMain, adoptedContents }) {
  const window = new BrowserWindow({
    title,
    width,
    height,
    show: false,
    autoHideMenuBar: true,
    backgroundColor: '#00000000',
    webPreferences: remoteSecurityDefaults(),
  });
  const siteView = adoptedContents
    ? new WebContentsView({ webContents: adoptedContents })
    : new WebContentsView({ webPreferences: safeWebPreferences() });
  const shellView = new WebContentsView({ webPreferences: shellWebPreferences() });
  shellView.setBackgroundColor('#00000000');
  window.contentView.addChildView(siteView);
  window.contentView.addChildView(shellView);
  const runtime = {
    window,
    siteView,
    shellView,
    shellReady: false,
    toolbarVisible: true,
    menuOpen: false,
    dialogOpen: false,
    isMain,
  };
  runtimeWindows.add(runtime);
  wireRuntimeSite(runtime);
  updateRuntimeBounds(runtime);
  window.on('resize', () => updateRuntimeBounds(runtime));
  window.on('close', (event) => {
    if (runtime.isMain && current.policy.background.enabled && !app.isQuitting) {
      event.preventDefault();
      window.hide();
      if (Notification.isSupported()) {
        const text = nativeStrings();
        const notification = new Notification({
          title: text.backgroundTitle(current.title),
          body: text.backgroundBody,
        });
        notification.on('click', () => { window.show(); window.focus(); });
        notification.show();
      }
      return;
    }
    if (runtime.isMain) {
      try { persistWindowState(window, current.id); }
      catch (error) { console.error('Failed to persist Chromium window state:', error); }
    }
  });
  window.once('closed', () => {
    runtimeWindows.delete(runtime);
    if (!siteView.webContents.isDestroyed()) siteView.webContents.close();
    if (!shellView.webContents.isDestroyed()) shellView.webContents.close();
    if (mainRuntime === runtime) mainRuntime = null;
  });

  shellView.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  shellView.webContents.on('will-navigate', (event, url) => {
    if (!url.startsWith(`${SHELL_ORIGIN}/`)) event.preventDefault();
  });
  shellView.webContents.once('did-finish-load', () => {
    runtime.shellReady = true;
    sendShellState(runtime);
  });
  shellView.webContents.loadURL(`${SHELL_ORIGIN}/shell.html`).catch((error) => console.error(error));
  if (show) window.show();
  return runtime;
}

function executeShellCommand(runtime, message) {
  const contents = runtime.siteView.webContents;
  const history = contents.navigationHistory;
  switch (message.command) {
    case 'back': if (history.canGoBack()) history.goBack(); break;
    case 'forward': if (history.canGoForward()) history.goForward(); break;
    case 'reload': contents.reload(); break;
    case 'reload-bypass-cache': contents.reloadIgnoringCache(); break;
    case 'stop': contents.stop(); break;
    case 'home': contents.loadURL(current.url).catch((error) => console.error(error)); break;
    case 'zoom-in': contents.setZoomFactor(Math.min(3, contents.getZoomFactor() + 0.1)); break;
    case 'zoom-out': contents.setZoomFactor(Math.max(0.5, contents.getZoomFactor() - 0.1)); break;
    case 'zoom-reset': contents.setZoomFactor(1); break;
    case 'toggle-fullscreen': runtime.window.setFullScreen(!runtime.window.isFullScreen()); break;
    case 'quit': app.isQuitting = true; app.quit(); break;
    case 'toolbar-visibility': runtime.toolbarVisible = message.visible; updateRuntimeBounds(runtime); break;
    case 'menu-open': runtime.menuOpen = message.visible; updateRuntimeBounds(runtime); break;
    default: throw new TypeError('unsupported shell command');
  }
  sendShellState(runtime);
}

function listenRuntimeSocket(socketPath) {
  return new Promise((resolve, reject) => {
    const server = nodeNet.createServer((socket) => socket.destroy());
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
  if (!mainRuntime || mainRuntime.window.isDestroyed()) {
    mainRuntime = createRuntimeWindow({
      title: current.title,
      width: current.width,
      height: current.height,
      show: !current.start_in_background,
      isMain: true,
    });
    if (current.maximized) mainRuntime.window.maximize();
  }
  const contents = mainRuntime.siteView.webContents;
  applyUserAgent(isolatedSession, contents, current.user_agent, defaultUserAgent);
  await configureProxy(isolatedSession, current.policy.proxy);
  await contents.loadURL(current.url).catch((error) => {
    revealToolbar(mainRuntime);
    console.error(error);
  });
  if (!current.start_in_background) {
    mainRuntime.window.show();
    mainRuntime.window.focus();
  } else if (Notification.isSupported()) {
    const text = nativeStrings();
    const notification = new Notification({
      title: text.backgroundTitle(current.title),
      body: text.backgroundStopBody,
    });
    notification.on('click', () => {
      mainRuntime?.window.show();
      mainRuntime?.window.focus();
    });
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
    protocol.handle('bastle-ui', (request) => {
      try {
        const url = new URL(request.url);
        if (request.method !== 'GET' || url.hostname !== 'shell') return new Response('', { status: 404 });
        const asset = SHELL_ASSETS.get(url.pathname);
        if (!asset) return new Response('', { status: 404 });
        const [filename, contentType] = asset;
        return new Response(fs.readFileSync(path.join(__dirname, filename)), {
          headers: {
            'Content-Type': contentType,
            'Content-Security-Policy': SHELL_CSP,
            'Cache-Control': 'no-store',
          },
        });
      } catch {
        return new Response('', { status: 400 });
      }
    });
    ipcMain.handle('bastle:shell-command', (event, rawMessage) => {
      const runtime = runtimeForShell(event.sender);
      if (!runtime || !trustedShellSender(event.sender, runtime.shellView.webContents)) {
        throw new Error('untrusted Chromium shell sender');
      }
      const message = validateShellCommand(rawMessage);
      executeShellCommand(runtime, message);
      return true;
    });

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
    Menu.setApplicationMenu(null);
    isolatedSession = session.fromPartition(`persist:bastle-${current.id}`, { cache: true });
    defaultUserAgent = isolatedSession.getUserAgent();
    configurePermissions(isolatedSession);
    configureDownloads(isolatedSession);
    await createWindow(current);
  }).catch((error) => {
    console.error(error);
    app.exit(1);
  });
}

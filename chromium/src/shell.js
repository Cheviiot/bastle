// SPDX-License-Identifier: GPL-3.0-only
'use strict';

const toolbar = document.getElementById('toolbar');
const hotZone = document.getElementById('hot-zone');
const title = document.getElementById('title');
const back = document.getElementById('back');
const forward = document.getElementById('forward');
const progress = document.getElementById('progress');
const menuButton = document.getElementById('menu-button');
const menu = document.getElementById('menu');

function applyStrings(strings) {
  if (!strings || typeof strings !== 'object') return;
  document.documentElement.lang = strings.lang || 'en';
  const labels = {
    back: strings.back,
    forward: strings.forward,
    'menu-button': strings.menu,
    'menu-reload': strings.reload,
    'menu-reload-bypass-cache': strings.reloadBypassCache,
    'menu-stop': strings.stop,
    'menu-home': strings.home,
    'menu-zoom-in': strings.zoomIn,
    'menu-zoom-out': strings.zoomOut,
    'menu-zoom-reset': strings.zoomReset,
    'menu-fullscreen': strings.fullscreen,
    'menu-quit': strings.quit,
  };
  for (const [id, label] of Object.entries(labels)) {
    const element = document.getElementById(id);
    if (element && typeof label === 'string') {
      if (id === 'menu-button' || id === 'back' || id === 'forward') {
        element.setAttribute('aria-label', label);
      } else {
        element.textContent = label;
      }
    }
  }
}

let loading = true;
let pointerInside = false;
let focusInside = false;
let dialogOpen = false;
let hideTimer = null;

function command(name, visible) {
  window.bastleShell.command(name, visible).catch(() => {});
}

function showToolbar() {
  clearTimeout(hideTimer);
  hideTimer = null;
  toolbar.hidden = false;
  command('toolbar-visibility', true);
}

function canHide() {
  return !loading && !pointerInside && !focusInside && menu.hidden && !dialogOpen;
}

function scheduleHide() {
  clearTimeout(hideTimer);
  if (!canHide()) return;
  hideTimer = setTimeout(() => {
    if (!canHide()) return;
    toolbar.hidden = true;
    command('toolbar-visibility', false);
  }, 1500);
}

function setMenu(open) {
  menu.hidden = !open;
  menuButton.setAttribute('aria-expanded', String(open));
  command('menu-open', open);
  if (open) showToolbar(); else scheduleHide();
}

hotZone.addEventListener('pointerenter', showToolbar);
hotZone.addEventListener('pointerdown', showToolbar);
toolbar.addEventListener('pointerenter', () => { pointerInside = true; showToolbar(); });
toolbar.addEventListener('pointerleave', () => { pointerInside = false; scheduleHide(); });
toolbar.addEventListener('focusin', () => { focusInside = true; showToolbar(); });
toolbar.addEventListener('focusout', () => {
  queueMicrotask(() => {
    focusInside = toolbar.contains(document.activeElement) || menu.contains(document.activeElement);
    scheduleHide();
  });
});
menuButton.addEventListener('click', () => setMenu(menu.hidden));
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && !menu.hidden) setMenu(false);
  if (event.key === 'Alt' || event.key === 'F10') showToolbar();
});
document.addEventListener('click', (event) => {
  const button = event.target.closest('[data-command]');
  if (!button) {
    if (!menu.hidden && !event.target.closest('#menu-button')) setMenu(false);
    return;
  }
  command(button.dataset.command);
  if (button.closest('#menu')) setMenu(false);
});

window.bastleShell.onState((state) => {
  applyStrings(state.strings);
  if (state.reveal) { showToolbar(); return; }
  if (typeof state.title === 'string') title.textContent = state.title;
  if (typeof state.canGoBack === 'boolean') back.disabled = !state.canGoBack;
  if (typeof state.canGoForward === 'boolean') forward.disabled = !state.canGoForward;
  if (typeof state.progress === 'number') {
    const value = Math.max(0, Math.min(1, state.progress));
    progress.style.transform = `scaleX(${value})`;
  }
  if (typeof state.loading === 'boolean') {
    loading = state.loading;
    progress.hidden = !loading;
    if (loading) showToolbar(); else scheduleHide();
  }
  if (typeof state.dialogOpen === 'boolean') {
    dialogOpen = state.dialogOpen;
    if (dialogOpen) showToolbar(); else scheduleHide();
  }
});

showToolbar();

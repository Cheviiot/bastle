// SPDX-License-Identifier: GPL-3.0-or-later
'use strict';

const assert = require('node:assert/strict');
const {
  quarantinePendingPopupNavigation,
  wireNavigationPolicy,
} = require('../src/navigation-policy');

class FakeWebContents {
  constructor() {
    this.listeners = new Map();
    this.popupHandler = null;
  }

  setWindowOpenHandler(handler) {
    this.popupHandler = handler;
  }

  on(event, handler) {
    this.listeners.set(event, handler);
  }

  emit(event, ...values) {
    this.listeners.get(event)(...values);
  }
}

const navigations = [];
const popups = [];
const createdWindows = [];
const handleNavigation = (...values) => navigations.push(values);
const frameNavigations = [];
const handleFrameNavigation = (...values) => frameNavigations.push(values);
const handlePopup = (...values) => {
  popups.push(values);
  return { action: 'deny' };
};
const handleCreatedWindow = (window, details, opener) => {
  assert.equal(typeof window.webContents.listeners.get('will-redirect'), 'function');
  createdWindows.push([window, details, opener]);
};

const parent = new FakeWebContents();
wireNavigationPolicy(
  parent,
  handleNavigation,
  handlePopup,
  handleCreatedWindow,
  handleFrameNavigation,
);
assert.equal(parent.listeners.get('will-navigate'), handleNavigation);
assert.equal(parent.listeners.get('will-redirect'), handleNavigation);
assert.equal(parent.listeners.get('will-frame-navigate'), handleFrameNavigation);
parent.emit('will-redirect', 'redirect-event', 'https://outside.example/');
assert.deepEqual(navigations, [['redirect-event', 'https://outside.example/']]);
parent.emit('will-frame-navigate', 'frame-event', 'https://frame.example/');
assert.deepEqual(frameNavigations, [['frame-event', 'https://frame.example/']]);
assert.deepEqual(parent.popupHandler({ url: 'https://example.org/' }), { action: 'deny' });
assert.equal(popups[0][1], parent);

const child = new FakeWebContents();
const childWindow = { webContents: child };
const childDetails = { url: 'https://popup.example/' };
parent.emit('did-create-window', childWindow, childDetails);
assert.equal(child.listeners.get('will-navigate'), handleNavigation);
assert.equal(child.listeners.get('will-redirect'), handleNavigation);
assert.equal(child.listeners.get('will-frame-navigate'), handleFrameNavigation);
assert.equal(typeof child.popupHandler, 'function');
assert.deepEqual(createdWindows[0], [childWindow, childDetails, parent]);

const grandchild = new FakeWebContents();
child.emit('did-create-window', { webContents: grandchild });
assert.equal(grandchild.listeners.get('will-redirect'), handleNavigation);
assert.equal(grandchild.listeners.get('will-frame-navigate'), handleFrameNavigation);
assert.equal(createdWindows.length, 2);

const pendingPopups = new WeakMap();
pendingPopups.set(child, true);
let prevented = false;
const pendingEvent = {
  sender: child,
  preventDefault() { prevented = true; },
};
assert.equal(quarantinePendingPopupNavigation(pendingPopups, pendingEvent), true);
assert.equal(prevented, true);

prevented = false;
const ordinaryEvent = {
  sender: parent,
  preventDefault() { prevented = true; },
};
assert.equal(quarantinePendingPopupNavigation(pendingPopups, ordinaryEvent), false);
assert.equal(prevented, false);

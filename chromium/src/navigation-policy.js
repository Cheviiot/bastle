// SPDX-License-Identifier: GPL-3.0-or-later
'use strict';

function wireNavigationPolicy(
  webContents,
  handleNavigation,
  handlePopup,
  handleCreatedWindow = () => {},
  handleFrameNavigation = () => {},
) {
  webContents.setWindowOpenHandler((details) => handlePopup(details, webContents));
  webContents.on('will-navigate', handleNavigation);
  webContents.on('will-redirect', handleNavigation);
  webContents.on('will-frame-navigate', handleFrameNavigation);
  webContents.on('did-create-window', (window, details) => {
    wireNavigationPolicy(
      window.webContents,
      handleNavigation,
      handlePopup,
      handleCreatedWindow,
      handleFrameNavigation,
    );
    handleCreatedWindow(window, details, webContents);
  });
}

function quarantinePendingPopupNavigation(pendingPopups, event) {
  if (!pendingPopups.has(event.sender)) return false;
  event.preventDefault();
  return true;
}

module.exports = { quarantinePendingPopupNavigation, wireNavigationPolicy };

// The settings window's own title bar: minimize, maximize or restore, close.
// Dragging and double-click to maximize come from `data-tauri-drag-region`;
// resizing from the edges and the rounded corners and shadow come from Windows.
(() => {
  const win = window.__TAURI__.window.getCurrentWindow();
  const bar = document.querySelector('.titlebar');
  const max = document.getElementById('tb-max');

  document.getElementById('tb-min').addEventListener('click', () => win.minimize());
  max.addEventListener('click', () => win.toggleMaximize());
  // Goes through the usual close, which keeps unsaved edits by hiding instead.
  document.getElementById('tb-close').addEventListener('click', () => win.close());

  async function syncMaximized() {
    const maximized = await win.isMaximized().catch(() => false);
    const label = maximized ? 'Restore' : 'Maximize';
    max.textContent = maximized ? '' : '';
    max.title = label;
    max.setAttribute('aria-label', label);
  }
  syncMaximized();
  win.onResized(syncMaximized);

  // Like Windows' own title bar, dim it while another window is active.
  window.addEventListener('focus', () => bar.classList.remove('inactive'));
  window.addEventListener('blur', () => bar.classList.add('inactive'));
})();

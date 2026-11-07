// Draws a Bloom window in the chosen look and keeps it up to date.
//
// The look is a mode (light, dark, or follow Windows) and an accent colour. The
// stylesheets already have the light and dark colours and the original pink;
// this switches the mode with a `data-theme` attribute, and for any other
// accent adds a style that overrides the accent's colours for each mode. The
// accent colours arrive already adjusted so that text in them stays readable.
(() => {
  const { invoke } = window.__TAURI__.core;
  const { listen } = window.__TAURI__.event;
  const STYLE_ID = 'bloom-accent';

  function accentRules(p) {
    return `--accent: ${p.accent}; --accent-ink: ${p.ink}; --accent-soft: ${p.soft}; --sel: ${p.selected}; --hover: ${p.hover};`;
  }

  /** Applies { mode, light, dark }: light and dark are null for the default accent. */
  function apply(theme) {
    const root = document.documentElement;
    if (theme.mode === 'light' || theme.mode === 'dark') root.setAttribute('data-theme', theme.mode);
    else root.removeAttribute('data-theme');

    let style = document.getElementById(STYLE_ID);
    if (!theme.light || !theme.dark) {
      if (style) style.remove();
      return;
    }
    if (!style) {
      style = document.createElement('style');
      style.id = STYLE_ID;
      document.head.append(style);
    }
    style.textContent = [
      `:root { ${accentRules(theme.light)} }`,
      `@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { ${accentRules(theme.dark)} } }`,
      `:root[data-theme="dark"] { ${accentRules(theme.dark)} }`,
      `:root[data-theme="light"] { ${accentRules(theme.light)} }`,
    ].join('\n');
  }

  async function refresh() {
    try {
      apply(await invoke('get_theme'));
    } catch {
      // Keep the stylesheet's own look rather than a half-applied one.
    }
  }

  refresh();
  // Changing the look in Settings redraws every open window.
  listen('theme-changed', refresh);
  window.bloomTheme = { apply, refresh };
})();

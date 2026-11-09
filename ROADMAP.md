# Roadmap

Things that were looked at and deliberately left for later, with enough detail
to pick them up cold. Nothing here is a commitment.

## Make the icon follow the accent color

Wanted: the icon should match the accent color chosen in Settings, instead of
always being its one fixed color.

Where it stands: the icon is a fixed set of files in `icons/`. The tray uses
the app's default window icon (`src/tray.rs`), the windows use the same one,
and the installer and the exe embed `icons/icon.ico` when they are built. The
accent is a runtime setting with presets and a custom color (`src/theme.rs`).

What is possible, roughly in order of effort:

- Tint the tray icon and the window icons at runtime. When the accent changes,
  recolor the icon image in memory and apply it with the tray's `set_icon` and
  each window's `set_icon`. Bloom already sends a `theme-changed` event when
  the look changes, so that is the hook. The icon needs a flat shape, or a
  separate mask, so that only the accent-colored part gets recolored.
- The exe, the installer and the Start menu shortcut cannot change at runtime,
  because Windows reads them from the file. They would stay at the default
  color unless a build is made per accent, which is not worth it.
- The taskbar icon of a running window follows `set_icon`, so it would match;
  a pinned shortcut would not.

Open question: whether the default (current) accent should keep today's exact
icon colors, so nothing changes for anyone who never touches the setting.

## Smaller follow-ups

- Caret lookup runs on the engine thread. Opening the suggestion list asks the
  focused app where its text cursor is (`src/platform/caret.rs`), which is a
  cross-process call. A hung app can stall the engine, and keys queue behind
  it. Moving the lookup off the engine thread, or giving it a timeout, would
  fix that. v0.3.0 only made each lookup cheaper by reusing one UI Automation
  client per thread.
- Matcher scans every trigger on each key (`src/matcher.rs`, four passes).
  Negligible at hundreds of triggers. If libraries reach thousands, index the
  triggers by their last character, or build a trie.
- The suggestion dropdown has not been checked visually with software
  rendering (`--disable-gpu`, added in v0.3.0). The search palette and the
  settings window were checked. Open the dropdown in a few apps and confirm
  the blur, corners and scrolling.
- Search window and dropdown speed have not been benchmarked on a large
  snippet library. v0.3.0's changes are based on reading the code and on
  memory measurements only.
- Keys typed while a fill-in form is opening are dropped, not typed into the
  app (see `expand` in `src/engine.rs`). Deliberate, since the search window
  takes the keyboard, but worth revisiting if anyone loses a keystroke there.
- The low-level mouse hook runs on every mouse move system-wide, only to catch
  button presses. Every alternative found was worse (Raw Input cannot tell
  keys swallowed by Mouse Without Borders), so it stays unless a better source
  of click events turns up.

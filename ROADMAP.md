# Roadmap

Things that were looked at and deliberately left for later, with enough detail
to pick them up cold. Nothing here is a commitment.

## Smaller follow-ups

- Caret lookup now runs on its own thread with a 300 ms timeout
  (`caret_position_bounded` in `src/platform/caret.rs`), so a hung app no longer
  stalls the engine. The stuck call itself cannot be cancelled: it stays on the
  worker until the app answers, and lookups in the meantime give no list. If
  that ever matters, the worker could be abandoned and replaced, at the cost of
  leaking a thread each time.
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

# Roadmap

Things that were looked at and deliberately left for later, with enough detail
to pick them up cold. Nothing here is a commitment.

## Backspace undo can garble text if you keep typing

Status: suspected from reading the code, not reproduced.

Backspace right after an expansion undoes it (`take_back` in `src/engine.rs`).
After the Backspace, Bloom waits for the keyboard to be quiet for 60 ms, then
erases `erase - 1` characters (the app already removed one) and types the
trigger back. It assumes the text on screen is exactly the expansion minus that
one character.

If you type a key in that window, the assumption is wrong. Example: undo
`uv run main.py`, then type `x` 50 ms later. The screen holds the 13 remaining
characters plus `x`, Bloom still erases 13, and the result is `u;run` with the
`x` lost. A key that lands during the erase or the typing interleaves with it.

The same race existed for normal expansions and was fixed in v0.3.0, by
holding back typed characters while Bloom types (`begin_hold`, `finish_hold`
and `absorb_late_keys`). The fix here would do the same:

- Start the hold as soon as `take_back` begins.
- Count keys that already got through, so the erase length is right.
- Replay the held keys after the trigger is restored.

Before changing anything, write a test that presses Backspace right after an
expansion and then types quickly, and confirm it fails on the current code.
`scripts/smoke.ps1` is the starting point; the scored version used for the
expansion fix typed a trigger, then a space 40 ms later, and compared the
result with the expected text.

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

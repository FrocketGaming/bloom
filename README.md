# Bloom

A text expander for Windows, written in Rust. Type a trigger like `;sig` and Bloom replaces it with your text, or press **Ctrl+Shift+Space** to search all your snippets.

- **Instant triggers.** Snippets expand as soon as you finish typing the trigger. If one trigger is the start of another (`;sig` and `;sign`), Bloom waits for your next key to decide.
- **Snippet search.** A frosted, centered search box with a live preview of what will be inserted.
- **Suggestions as you type.** After the first few characters of a trigger, a short list of matching snippets opens at your text cursor.
- **Variables.** `{{date}}`, `{{time}}`, `{{clipboard}}`, and `{{cursor}}` to choose where the cursor ends up.
- **Fill-in forms.** `{{field:Name}}` asks for a value before inserting; `{{field:Tone|Warm|Formal}}` offers a choice.
- **Plain files.** Snippets live in TOML files that reload as soon as you save them, plus a settings window for editing them.

## Install

Download `Bloom_x.y.z_x64-setup.exe` from the [latest release](https://github.com/FrocketGaming/bloom/releases/latest) and run it. It installs for your user account, so no admin rights are needed, and adds Bloom to the Start Menu. Requires Windows 10 or 11.

The installer isn't code-signed yet, so Windows may show "Windows protected your PC". Click **More info**, then **Run anyway**. Each release lists the installer's SHA-256 hash if you want to verify the download.

Bloom runs in the system tray. Click the flower icon to open the snippet editor, or right-click it for more options. Uninstalling from Windows Settings removes the app but keeps your snippets in `%APPDATA%\Bloom`.

## Building from source

You need [Rust](https://rustup.rs) with the MSVC toolchain, and the Visual Studio C++ Build Tools.

```sh
cargo build --release
./target/release/bloom.exe
```

Use `cargo run` during development; that build also opens a console window with log output. To build the installer, you also need [Node.js](https://nodejs.org):

```sh
npx @tauri-apps/cli@2 build
```

The installer is written to `target/release/bundle/nsis/`.

### Releasing

1. Update `version` in both `Cargo.toml` and `tauri.conf.json`, and merge that change.
2. Tag the merge commit and push the tag: `git tag v0.2.0 && git push origin v0.2.0`.
3. The Release workflow builds the installer and creates a draft release. Review it on GitHub, then publish.

## Writing snippets

Snippets are stored in `%APPDATA%\Bloom\snippets\*.toml`. Each file becomes a group in the editor.

```toml
[[snippet]]
trigger = ";fu"
name = "Meeting follow-up"
body = """
Hi {{field:Name}},

Thanks for meeting on {{date}}. Next steps:

- {{cursor}}

{{field:Sign-off|Best|Cheers}},
Sam"""
```

| Placeholder | Inserts |
| --- | --- |
| `{{date}}` / `{{date:%d/%m/%Y}}` | Today's date, optionally in a [custom format](https://docs.rs/chrono/latest/chrono/format/strftime/index.html) |
| `{{time}}` | The current time |
| `{{clipboard}}` | The text on your clipboard |
| `{{cursor}}` | Nothing; the cursor is placed here afterwards |
| `{{field:Label}}` | A value you type in before inserting |
| `{{field:Label\|A\|B}}` | One of the listed options |
| `{{var:name}}` | One of [your own variables](#your-own-variables): its text, or a choice from its list |

Triggers only expand at the start of a word (after a space, punctuation or a new line), so `;fu` fires but `x;fu` doesn't. Set `word = false` on a snippet to let it expand mid-word too. Snippets without a `trigger` appear only in search.

Starting triggers with `;` is recommended: a semicolon is almost never followed directly by a letter in prose or code, so triggers don't fire by accident, and it doesn't need Shift. Any text works as a trigger, though.

Settings such as the hotkey are in `%APPDATA%\Bloom\bloom.toml`, or under **Settings** in the Bloom window.

## Your own variables

Define something once and use it in any snippet as `{{var:name}}`, instead of retyping it in every snippet. Open **Variables** in the Bloom window to add them, or edit `%APPDATA%\Bloom\variables.toml` yourself; Bloom reloads it as soon as you save.

```toml
[variables]
email = "sam@example.com"
address = """
221 Harbor Street, Suite 4
Portland, OR 97209"""
signoff = ["Best", "Cheers", "Kind regards"]
```

- **Text** (`email`) is inserted exactly as written. Placeholders inside it, such as `{{date}}`, are not expanded; put those in the snippet next to it.
- **A list** (`signoff`) asks you to pick one when the snippet is used, in the same form as `{{field:...}}`, labelled with the variable's name. Using it twice in one snippet asks once.
- **Names** use letters, digits, `-` and `_`, and ignore case, so `{{var:Email}}` and `{{var:email}}` are the same variable.
- **A name nothing defines** (a typo, or a variable you removed) is inserted as typed, so the mistake is visible. It is underlined in the snippet's preview and listed in the warnings at the top of the Bloom window.
- **The Variables page** shows how many snippets use each variable and offers an **Add** button for names that snippets use but nothing defines. The **Variable** drop-down above a snippet's text inserts a placeholder for you.
- **Comments in the file are kept** when the page saves it. If the file has a syntax error, Bloom leaves it alone and says so, and an entry it cannot read is never deleted by a save.

## Undoing an expansion

If Bloom expands something you didn't mean to, press **Backspace** straight away. It erases the expansion and types your trigger back, so you are exactly where you were before it fired.

- **It has to be the very next key.** Any other key, a mouse click or switching windows means you have moved on, and Backspace deletes one character as usual. There is no time limit.
- **You can carry on from the restored trigger.** Type the rest of a longer trigger, or press Backspace again and retype the last letter, and Bloom expands it as it normally would.
- **It works for triggers you typed and for snippets chosen from the suggestion list.** A character you typed straight after a trigger is put back too.
- **It does not apply** to text inserted from the search window (there was no trigger to put back), to snippets that place the cursor inside the text with `{{cursor}}` (your next key is meant for that spot), or to expansions longer than 1000 characters.
- **Some older text boxes** delete accents and emoji one piece at a time. After undoing a snippet that contains those, such a box can leave part of that text behind. Bloom never deletes more than it inserted.
- **To turn it off,** use **Undo with Backspace** under **Settings**, or set `undo_with_backspace = false` in `bloom.toml`.

## Suggestions while you type

Type a trigger's symbol and the next two characters (`;si`) and a small list opens just below your text cursor, showing the snippets that trigger could still become. The highlighted row also shows the start of its text.

| Key | Does |
| --- | --- |
| Up / Down | Move the highlight, wrapping around |
| Enter or Tab | Insert the highlighted snippet in place of what you typed |
| Esc | Close the list. It stays closed until you type something else |
| Keep typing | The list narrows. Finish a trigger and it expands as usual |

- **Only for triggers that start with a symbol,** such as `;sig`, and only at the start of a word (or anywhere, for snippets set to `word = false`). Triggers made of plain letters never open the list, and snippets without a trigger stay in search.
- **Snippets with fill-in fields** show an ellipsis after their name. Choosing one opens the usual form; cancelling it puts back what you typed.
- **The list never takes focus.** While it is open it takes the arrow keys, Enter, Tab and Esc, and gives them back the moment it closes. Those keys go to your app unchanged when Ctrl, Alt or Shift is held.
- **It needs the app to report where its text cursor is.** Most apps do. In one that doesn't, no list opens and triggers expand exactly as before.
- **Never shown** in password fields, while Bloom is paused, or in Bloom's own windows.
- **To turn it off,** use **Suggestions while typing** under **Settings**, or set `suggestions = false` in `bloom.toml`.

## Using two computers (Mouse Without Borders)

Bloom only reacts to keys that actually reach an app on the PC it runs on. With [Mouse Without Borders](https://learn.microsoft.com/windows/powertoys/mouse-without-borders) sharing one keyboard between PCs:

- **Run Bloom on both PCs.** Type on the PC with the keyboard and it expands there. Move the mouse to the other PC and type, and that PC's Bloom expands in its own windows.
- **The PC you left stays quiet.** Mouse Without Borders blocks the keys there before they reach any app, and Bloom checks for that, so nothing expands on a screen you aren't typing into.
- **Use the same keyboard layout on both PCs.** Mouse Without Borders forwards keys, not characters, so a different layout on the second PC turns `;` into something else.
- **Send the keyboard to one PC at a time.** In Mouse Without Borders' mode that controls all PCs at once, every Bloom would expand the same trigger.
- **If expansions come out garbled on the second PC,** set **How Bloom inserts text** to **Paste it** there. Typed expansions have dropped or mixed up characters on a PC receiving the keyboard (seen in Notepad), while pasting stays reliable. Share clipboard keeps working: Bloom waits long enough before restoring your clipboard that Mouse Without Borders shares the restored copy, not the snippet.

Bloom's own typing is tagged, so it never expands its own output, while keys replayed by Mouse Without Borders count as real typing. Tools that ignore all simulated keys can't expand on the second PC.

### Windows that run as administrator

Windows blocks ordinary apps from typing into elevated windows, so Bloom can't insert into Task Manager, an elevated terminal, or anything else running as administrator unless Bloom runs elevated too. Bloom notices when Windows refuses its keys and says so the next time you open its window.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Windows-specific code (input listening, key translation, typing and focus) lives in `src/platform/`, so other platforms can be added alongside it.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

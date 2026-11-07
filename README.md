# Bloom

A text expander for Windows, written in Rust. Type a trigger like `:sig` and Bloom replaces it with your text, or press **Ctrl+Shift+Space** to search all your snippets.

- **Instant triggers.** Snippets expand as soon as you finish typing the trigger. If one trigger is the start of another (`:sig` and `:sign`), Bloom waits for your next key to decide.
- **Snippet search.** A frosted, centered search box with a live preview of what will be inserted.
- **Variables.** `{{date}}`, `{{time}}`, `{{clipboard}}`, and `{{cursor}}` to choose where the cursor ends up.
- **Fill-in forms.** `{{field:Name}}` asks for a value before inserting; `{{field:Tone|Warm|Formal}}` offers a choice.
- **Plain files.** Snippets live in TOML files that reload as soon as you save them, plus a settings window for editing them.

## Running it

You need [Rust](https://rustup.rs) with the MSVC toolchain, and the Visual Studio C++ Build Tools.

```sh
cargo build --release
./target/release/bloom.exe
```

Bloom runs in the system tray. Click the flower icon to open the snippet editor, or right-click it for more options. Use `cargo run` during development; that build also opens a console window with log output.

## Writing snippets

Snippets are stored in `%APPDATA%\Bloom\snippets\*.toml`. Each file becomes a group in the editor.

```toml
[[snippet]]
trigger = ":fu"
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

Set `word = true` on a snippet to expand only when the trigger starts a new word. Snippets without a `trigger` appear only in search.

Settings such as the hotkey are in `%APPDATA%\Bloom\bloom.toml`, or under **Settings** in the Bloom window.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Windows-specific code (keyboard hook, typing, focus) lives in `src/platform/`, so other platforms can be added alongside it.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.

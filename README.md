# Kura

Kura (蔵) means "storehouse" in Japanese.

Kura keeps my memes in one place so I can find them again. You add images from a
file or a URL, tag them, and then browse them in a terminal UI with image
previews. One key press copies the selected meme to the clipboard.

## Features

- **Storage by content hash.** Each image is stored under its SHA-256 hash, so
  adding the same image twice is caught.
- **Tags.** Images and tags live in SQLite. Deleting an image removes its tags.
- **Add from a URL.** The file type is detected from the image's first bytes,
  not from the URL.
- **Terminal UI.** Browse with `j`/`k` and see a live image preview. Images load
  in the background and the most recent previews are cached.
- **Search.** Press `/` to fuzzy search names and tags. `#reaction` filters by tag.
- **Hako.** The TUI is built on Hako (箱, "box"), my own small async TUI
  framework on top of ratatui. It lives in this repo under `hako/`.

## Running it

```sh
cargo build --release
```

Kura uses sqlx's compile-time checked queries. The query cache in `.sqlx/` is
committed, so a normal build doesn't need a database.

If you change a query, the cache has to be regenerated against a database that
has the schema:

```sh
sqlite3 dev.db "
CREATE TABLE images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    file_path TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    image_hash TEXT NOT NULL REFERENCES images(hash) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(image_hash, tag)
);"
echo 'DATABASE_URL=sqlite:./dev.db' > .env

mise install        # installs sqlx-cli, pinned in mise.toml
cargo sqlx prepare
```

`dev.db` is only used while compiling. At runtime Kura keeps its data in
your system's standard locations:

| What | Where (Linux) |
| --- | --- |
| Images | `~/Pictures/kura/` |
| Database | `~/.local/share/kura/kura.db` |
| Preview cache | `~/.cache/kura/` |

Copying to the clipboard needs Wayland and `wl-copy`. Opening an image uses
`xdg-open`. Image previews need a terminal that can draw images, such as kitty,
Ghostty, WezTerm, or one with sixel support.

## Usage

```sh
kura add "this is fine" --file-path ~/Downloads/dog.png
kura add "cat vibing" --url https://example.com/cat.gif

kura tag "cat vibing" reaction
kura list --tag reaction
kura untag "cat vibing" reaction
kura rename "cat vibing" "cat jam"
kura remove "cat jam"

kura tui
kura pick
```

`kura nuke` deletes every stored image and the database. `--clear-cache`
clears the preview cache.

### TUI keys

| Key | Action |
| --- | --- |
| `j` / `↓` | Next image |
| `k` / `↑` | Previous image |
| `/` | Search by name or tag. `#word` keeps only images with a tag starting with `word` |
| `Esc` | Clear the search, or cancel an input. With no search, quit |
| `Enter` | Copy image to clipboard |
| `o` | Open in the default viewer |
| `r` | Rename |
| `t` | Add a tag |
| `u` | Remove a tag |
| `q` | Quit |

### Picker

`kura pick` opens the same browser as `kura tui`, with the search box closed.
Use `j` and `k`, or Down and Up, to move the selection. Press `/` to open the
search box and type to filter. Enter closes the search box and keeps the
filter. Esc closes the search box and clears the filter.

In the list, Enter copies the selected image to the clipboard and exits. Esc
clears an active filter first, and otherwise exits. `q` exits without copying.

To open the picker from a hotkey, float it with a window rule. Ghostty's
`--class` sets the window class that the rule matches. This example uses
Hyprland's Lua config:

```lua
hl.window_rule({
    name = "kura-pick",
    match = { class = "^org\\.arsene\\.kura-pick$" },
    float = true,
    center = true,
    size = "875 600",
})

hl.bind("SUPER + I", hl.dsp.exec_cmd("ghostty --class=org.arsene.kura-pick -e kura pick"))
```

## Hako

Hako is the reusable part. Kura is its first user.

- **App loop.** Implement `hako::App` (`handle_event`, `on_action`, `render`)
  and call `hako::run(app)`. Input events become actions, and actions are
  handled asynchronously.
- **Component stack.** Components get each event from the top of the stack
  down. A component either lets the event pass on or stops it there, and it can
  queue a callback. Callbacks run after the loop finishes, so they can push or
  pop components without fighting the borrow checker.
- **Terminal.** One tokio task merges keyboard input, a tick timer, and a
  separate render timer, so the tick rate and frame rate are set independently.
- **Widgets.** Text input, scroll list, and form fields with errors.
- **Utilities.** Image type detection from file bytes, terminal image
  rendering, and a clipboard interface.

## Status

This is a personal project and still changing. `kura serve` is an unfinished
experiment in writing an HTTP server by hand. The clipboard only works on
Wayland so far.

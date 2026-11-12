# Kura

Kura (蔵) means "storehouse" in Japanese.

Kura keeps my memes in one place so I can find them again. You add images from a
file or a URL, tag them, and then find them in a terminal UI with image
previews, or in the Mac app. One key press copies the selected meme to the
clipboard.

## Features

- **Storage by content hash.** Each image is stored under its SHA-256 hash, so
  adding the same image twice is caught.
- **Tags.** Images and tags live in SQLite. Deleting an image removes its tags.
- **Add from a URL.** The file type is detected from the image's first bytes,
  not from the URL.
- **Terminal UI.** Browse with `j`/`k` and see a live image preview. Images load
  in the background and the most recent previews are cached.
- **Search.** Press `/` to fuzzy search names, tags and the text written inside
  each image. `#reaction` filters by tag.
- **Picker.** `kura pick` copies an image and exits, for binding to a hotkey.
- **HTTP API and Mac app.** `kura serve` exposes the library over HTTP with a
  live change feed. The SwiftUI Mac app runs its own server and refreshes live.

## Layout

| Path | What it is |
| --- | --- |
| `kura-core/` | The library: SQLite database, image files, thumbnails, change feed |
| `kura-server/` | HTTP API over `kura-core` (axum), with live updates over SSE (`GET /events`) |
| `src/` | The `kura` binary: CLI commands, the TUI and the picker |
| `hako/` | Hako (箱, "box"): my own small async TUI framework and utilities |
| `hako/http/` | `hako-http`: an HTTP/1.x request parser written by hand, and a Router sketch |
| `mac/` | The SwiftUI Mac app; it starts its own `kura-server` |

## Where your library lives

| What | macOS | Linux |
| --- | --- | --- |
| Images | `~/Pictures/kura/` | `~/Pictures/kura/` |
| Database | `~/Library/Application Support/com.kura/kura.db` | `~/.local/share/kura/kura.db` |
| Preview cache | `~/Library/Caches/com.kura/` | `~/.cache/kura/` |

Images are stored as `<hash>.<ext>`, and their names and tags live in the
database. Add, rename and remove images through Kura rather than in a file
manager. `kura doctor` checks that the files and the database agree. The
preview cache is always safe to delete.

## Requirements

- Rust (stable) and [`just`](https://github.com/casey/just)
- For the Mac app: macOS 26 and Xcode 26 (Swift 6.2)
- To regenerate the sqlx query cache: `sqlx-cli`, pinned in `mise.toml`
  (`mise install`)

Copying an image puts it on the clipboard as a file, so it pastes into chat
apps as an attachment (GIFs stay animated). On macOS it's also copied as a
picture, for apps like Notes and Preview; this uses the built-in `osascript`.
On Linux it needs Wayland and `wl-copy`, and opening an image uses `xdg-open`.
Image previews need a terminal that can draw images, such as kitty, Ghostty,
WezTerm, or one with sixel support.

## Running it

`just` lists every task. Tasks run against one of two environments, picked with
`KURA_ENV`:

- `dev` (default): a throwaway library in `./.kura-dev`, debug build. Safe to wreck.
- `prod`: your real library, release build.

```sh
just tui                         # browse the dev library in the terminal
KURA_ENV=prod just tui           # browse your real library
just run add meme.png -t funny   # any kura command
just serve                       # HTTP API on 127.0.0.1:7878
just doctor                      # check the library; `just doctor --fix` cleans up
just reset-dev                   # delete the dev library (never the real one)
```

To install the binary itself: `cargo install --path .`

### Mac app

```sh
just install    # build Kura.app and install it into /Applications
```

The installed app uses your real library and runs its own server, so you
don't need `just serve`. Run `just install` again after changing the code.

While working on the app:

- `just mac` runs it from source on the current environment's library.
- `just app` builds `mac/build/Kura.app` and opens it.
- `just xcode` opens the package in Xcode.
- `just mac-test` runs its tests.

In the app, the sidebar's search works like the TUI's (`#tag` filters by tag).
The server does the matching, so both find the same images in the same order.
The app reads the text in each image on your Mac (Apple Vision) and stores it
in the library, so search also finds words inside a meme. The first pass over a
big library takes a while; the window's subtitle shows "Reading text…" until
it's done.

| Key | Action |
| --- | --- |
| `⌘F` or `/` | Search |
| `⌘C` | Copy the image (as a picture and as a file, so GIFs stay animated) |
| `Return` or double-click | Copy the image and hide Kura, like `kura pick` |
| `⌘O` or `+` | Add images from files |
| `⌘V` | Add the image, file or link on the clipboard |
| `⌘E` | Rename the image |
| `⌘⌫` | Delete the image, after asking |
| `⌘R` | Reload |

You can also drop images onto the window: files from Finder, or images and
links dragged from a browser. Formats Kura doesn't store (HEIC, TIFF
screenshots) are converted to PNG first. Video clips (MP4, MOV) become looping
GIFs: the first 15 seconds at 12 frames a second, at most 480 px on the long
side. Only the Mac app converts video; `kura add` takes images only.

Under the preview, a tag's `×` removes it and `+` adds one. The text in the
image sits below the preview too; its menu edits that text by hand (your text
wins over what was read), or reads it again. The right-click menu on a row has
Rename and Delete as well. `⌘E` and `⌘⌫` work on the selected image and leave
the search field alone while you type in it.

## Usage

```sh
kura add ~/Downloads/dog.png -n "this is fine"
kura add https://example.com/cat.gif -n "cat vibing" -t reaction
kura add old.png --created-at "2026-05-14 00:29:43"   # keep an import's original date

kura tag "cat vibing" music
kura list --tag reaction
kura tags
kura untag "cat vibing" music
kura rename "cat vibing" "cat jam"
kura caption "cat jam"            # show the text written in the image
kura caption "cat jam" "when the moon hits"
kura remove "cat jam"              # or: kura remove --file <any copy of it>

kura tui
kura pick
kura serve
kura doctor [--fix]
```

`kura caption <image> [TEXT | --none | --clear | --reread]` sets the text that
search finds inside an image. `TEXT` sets it by hand, and your text wins over
the text the Mac app read with OCR. `--none` says there is no text in the image,
`--clear` drops your text so the OCR text shows again, and `--reread` drops the
OCR text so the Mac app reads the image again. With no argument it prints the
caption, `(no text)`, or `(not read yet)`.

An image is named by its name or its hash. Without `-n`, the name comes from
the file name. `--library <folder>` uses another library, `--clear-cache`
clears the preview cache, and `kura nuke` deletes every stored image and the
database.

### TUI keys

| Key | Action |
| --- | --- |
| `j` / `↓` | Next image |
| `k` / `↑` | Previous image |
| `/` | Search by name, tag or caption text. `#word` keeps only images with a tag starting with `word` |
| `Esc` | Clear the search, or cancel an input. With no search, quit |
| `Enter` | Copy image to clipboard |
| `o` | Open in the default viewer |
| `r` | Rename |
| `t` | Add a tag |
| `u` | Remove a tag |
| `q` | Quit |

### Search

Words are fuzzy matched against the name and tags, with the usual fzf
operators (`'exact`, `^prefix`, `suffix$`, `!not`). A word that misses the name
and tags can still match the image's caption: it must start a word of the
text (`wal` finds "WHEN THE BOSS WALKS IN"), or with `'` appear anywhere in it.
Captions ignore case and accents, and one-letter words skip them. An image
whose name or tags match always ranks above one found only by its caption, and a
caption hit shows the matching words after its name. `!word` also hides images
whose caption has the word.

Only the Mac app reads text out of images, so images added from the terminal
are found by caption once the app has read them. The list's footer says how
many are `not read yet`. The TUI refreshes when the app or the CLI changes the
library, without a key press.

Captions are stored in a new table (migration `0004`). A `kura` built before
it refuses to open a library that has it, with the "upgraded by a newer
version" message, so update every copy of the binary.

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

Hako is the reusable part. Kura is its first user. The TUI pieces sit behind
the `tui` feature, so `kura-core` and `kura-server` use only the plain utilities.

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
  rendering, and a clipboard (macOS and Wayland).

## Development

```sh
just test     # every test in the workspace
just lint     # rustfmt check and clippy
just ci       # lint, tests, and that .sqlx/ is current
```

Kura uses sqlx's compile-time checked queries. The query cache in `.sqlx/` is
committed, so a normal build doesn't need a database. After changing a query,
run `just sqlx-prepare` and commit `.sqlx/`. It migrates `dev.db` (used only
while compiling) and regenerates the cache.

## Status

This is a personal project and still changing. `hako-http` is an unfinished
experiment in writing an HTTP server by hand; `kura serve` uses axum for now.

## Goals

- I wanna build out a tui
- Also want to learn how to create my own HTTP server.
- Learn a bit more about async/Tokio etc.

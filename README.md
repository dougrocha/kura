# Kura

Kura stands for "storage" in Japanese (蔵).

My own app to store my memes and find them again quickly. It's a Rust library
with three front ends: a CLI and TUI, an HTTP server, and a SwiftUI Mac app.

## Layout

| Path | What it is |
| --- | --- |
| `kura-core/` | The library: SQLite database, image files, thumbnails, change feed |
| `kura-server/` | HTTP API over `kura-core` (axum), with live updates over SSE (`GET /events`) |
| `src/` | The `kura` binary: CLI commands and the TUI |
| `hako/` | My own little std lib (TUI widgets, clipboard), used by the TUI |
| `mac/` | The SwiftUI Mac app; it starts its own `kura-server` |

## Where your library lives

- Images: `~/Pictures/kura`, stored as `<hash>.<ext>`
- Database: `~/Library/Application Support/com.kura/kura.db`
- Thumbnails: `~/Library/Caches/com.kura` (safe to delete)

Names and tags live in the database, so add, rename and remove images through
Kura rather than in Finder. `kura doctor` checks that the two agree.

## Requirements

- Rust (stable) and [`just`](https://github.com/casey/just)
- For the Mac app: macOS 26 and Xcode 26 (Swift 6.2)
- For `just ci`, `just migrate` and `just sqlx-prepare`: `sqlx-cli`
  (`cargo install sqlx-cli --no-default-features --features sqlite`)

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

### TUI keys

`j`/`k` or arrows to move · `Enter` copy the image to the clipboard ·
`o` open it · `r` rename · `t` add a tag · `u` remove a tag · `q` quit

## CLI

```sh
kura add <file-or-url> [-n name] [-t tag]... [--created-at "2026-05-14 00:29:43"]
kura list [-t tag]
kura tags
kura tag <image> <tag>    /    kura untag <image> <tag>
kura rename <image> <new-name>
kura remove <image>       /    kura remove --file <copy-of-the-image>
kura doctor [--fix]
kura serve [--bind 127.0.0.1:7878]
kura tui
kura nuke                 # deletes the whole library: images, database, thumbnails
```

`<image>` is a name or a hash. `--library <folder>` uses another library.
`--created-at` keeps an imported image's original date (RFC 3339, or
`YYYY-MM-DD HH:MM:SS` in UTC).

## Development

```sh
just test     # every test in the workspace
just lint     # rustfmt check and clippy
just ci       # lint, tests, and that .sqlx/ is current
```

After changing a SQL query, run `just sqlx-prepare` and commit `.sqlx/`.

## Goals

- I wanna build out a tui
- Also want to learn how to create my own HTTP server.
- Learn a bit more about async/Tokio etc.

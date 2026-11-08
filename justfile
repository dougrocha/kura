# Kura tasks. `just` lists them.
#
# Environments, picked with KURA_ENV (default: dev):
#   dev   a throwaway library in ./.kura-dev, debug build. Safe to wreck.
#   prod  your real library (~/Pictures/kura), release build.
#
#   just serve                  dev server on 127.0.0.1:7878
#   KURA_ENV=prod just tui      the TUI on your real library
#   just env=prod list          same thing, as a just variable

env := env("KURA_ENV", "dev")
bind := env("KURA_BIND", "127.0.0.1:7878")
dev_library := justfile_directory() / ".kura-dev"
library_path := if env == "dev" { dev_library } else { "" }

library_flag := if env == "dev" { "--library '" + dev_library + "'" } else if env == "prod" { "" } else { error("KURA_ENV must be dev or prod, not '" + env + "'") }
profile_flag := if env == "prod" { "--release" } else { "" }
target_dir := if env == "prod" { "release" } else { "debug" }

[private]
default:
    @just --list --unsorted
    @echo "\nenvironment: {{env}}  (KURA_ENV=dev|prod)"

# ---------------------------------------------------------------- run

# Run any kura command, e.g. `just run add meme.png -t funny`
run *args:
    cargo run {{profile_flag}} -q --bin kura -- {{library_flag}} {{args}}

# Serve the HTTP API
serve:
    @just env={{env}} run serve --bind {{bind}}

# Serve, restarting on every code change (dev)
watch:
    cargo watch -q -c -w src -w kura-core -w kura-server -w hako -x 'run -q --bin kura -- {{library_flag}} serve --bind {{bind}}'

# Browse the library in the terminal
tui:
    @just env={{env}} run tui

# Check the library against its files; `just doctor --fix` cleans up
doctor *args:
    @just env={{env}} run doctor {{args}}

# Follow the live change feed of a running server
events:
    curl -sN http://{{bind}}/events

# The Mac app from source (`swift run`); it starts its own server for this environment
mac:
    cargo build {{profile_flag}} -q -p kura-server --bin kura-server
    cd mac && KURA_BIN="{{justfile_directory()}}/target/{{target_dir}}/kura-server" KURA_LIBRARY="{{library_path}}" swift run

# Build mac/build/Kura.app (release, server inside)
bundle:
    mac/bundle.sh

# Build and open Kura.app on this environment's library
app: bundle
    open -n {{ if env == "dev" { "--env KURA_LIBRARY='" + dev_library + "' " } else { "" } }}mac/build/Kura.app

# Install Kura.app into /Applications (your real library, like any app)
install: bundle
    rm -rf /Applications/Kura.app
    cp -R mac/build/Kura.app /Applications/
    @echo "Installed /Applications/Kura.app"

# Open the Mac app in Xcode (set KURA_BIN in the scheme's environment, or run `just bundle`)
xcode:
    open mac/Package.swift

# ---------------------------------------------------------------- debug

# Run a kura command under lldb, e.g. `just debug list`
debug *args:
    cargo build {{profile_flag}} --bin kura
    rust-lldb -- target/{{target_dir}}/kura {{library_flag}} {{args}}

# Serve with backtraces and full error detail
debug-serve:
    RUST_BACKTRACE=1 just env={{env}} serve

# ---------------------------------------------------------------- check

# Every test in the workspace
test *args:
    cargo test --workspace {{args}}

# Formatting and clippy
lint:
    cargo fmt --all --check >/dev/null || echo "fmt: some files are unformatted; run: just fmt"
    cargo clippy --workspace --all-targets

# What CI would run: lint, tests, and that the offline query data is current
ci: lint test
    cargo sqlx prepare --workspace --check

fmt:
    cargo fmt --all

# ---------------------------------------------------------------- database

# Apply migrations to dev.db, the database sqlx checks queries against
migrate:
    cargo sqlx migrate run --source kura-core/migrations

# Refresh .sqlx/ after changing a query (commit the result)
sqlx-prepare: migrate
    cargo sqlx prepare --workspace

# Delete the dev library (never your real one)
reset-dev:
    cargo run -q --bin kura -- --library '{{dev_library}}' nuke

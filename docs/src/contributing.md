# Contributing

Contributions are welcome. Please open an issue first to discuss significant changes before submitting a pull request.

## Setup

```sh
git clone https://github.com/your-username/grove
cd grove
cargo build
```

## Running tests

```sh
cargo test
```

The test suite covers frecency logic (`src/db/repo.rs`) and git operations (`src/git.rs`).

## Running the tool locally

```sh
cargo run -- --help
cargo run -- open
cargo run -- repo list
```

Or install from your local clone:

```sh
cargo install --path .
```

## Project structure

```
src/
├── main.rs          # Entry point: parses CLI, loads config/DB, dispatches
├── cli.rs           # All clap CLI structs and enums
├── config.rs        # Config struct, shell/multiplexer enums, path helpers
├── multiplexer.rs   # Multiplexer trait, SessionName, template rendering
├── zellij.rs        # Zellij backend implementation
├── tmux.rs          # tmux backend implementation
├── git.rs           # Git operations (clone, worktree add/list/remove/prune)
├── db/
│   ├── mod.rs       # Db struct, migrations, public API
│   └── repo.rs      # Repo model, CRUD, frecency logic
└── cmd/
    ├── open.rs      # grove open
    ├── clone.rs     # grove clone
    ├── repo.rs      # grove repo *
    ├── tree.rs      # grove tree *
    ├── session.rs   # grove session *
    ├── config.rs    # grove config *
    ├── init.rs      # grove init
    └── completions.rs  # grove completions

templates/
├── layout.kdl       # Default zellij layout (minijinja)
└── tmux.sh          # Default tmux setup script (minijinja)
```

## Adding a new multiplexer backend

1. Implement the `Multiplexer` trait from `src/multiplexer.rs`.
2. Add a new variant to `MultiplexerBackend` and `ResolvedBackend` in `src/config.rs`.
3. Wire it up in `src/main.rs` where the backend is resolved.
4. Add a default layout template under `templates/`.

## Code style

grove uses standard `rustfmt` formatting. Run before submitting:

```sh
cargo fmt
cargo clippy
```

<div align="center">

# grove

**Instant worktrees. Instant sessions. Zero friction.**

grove pairs git worktrees with your terminal multiplexer ([zellij](https://zellij.dev) or [tmux](https://github.com/tmux/tmux)) so you can switch between any repo and branch in a single keystroke — each with its own isolated workspace, editor, and shell.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

[Getting Started](#getting-started) •
[Installation](#installation) •
[Configuration](#configuration) •
[Commands](#commands) •
[Documentation](https://seftontycho.github.io/grove)

</div>

---

## What is grove?

Context switching is expensive. Stashing changes, checking out branches, killing your editor, relaunching — it all adds up. grove eliminates this entirely.

With grove, every repo+branch combination gets a **dedicated git worktree** (so your working trees never interfere) and a **dedicated multiplexer session** (so your terminal layout is always ready). Jump between `frontend/main`, `api/feat/auth`, and `infra/hotfix` as fast as you can type — no stashing, no losing your place.

grove works with both **zellij** and **tmux**, and auto-detects which one you're using.

By default, both **zellij** and **tmux** sessions open with three named tabs/windows: `shell` (focused), `editor` (`nvim .`), and `opencode`. The layout is fully customisable — see [Configuration](#configuration).

## Getting started

```sh
# 1. Set up the shell alias
eval "$(grove init zsh)"   # or bash / fish

# 2. Configure your project directories
grove config edit

# 3. Clone a repo as a bare repository
grove clone git@github.com:your-org/your-repo.git work

# 4. Open a worktree + session (interactive)
gv
```

That's it. grove will ask you which repo and branch you want, create the worktree if needed, and drop you straight into a zellij session for it.

## Installation

```sh
cargo install --git https://github.com/seftontycho/grove
```

### From source

If you want to hack on grove locally:

```sh
git clone https://github.com/seftontycho/grove
cd grove
cargo install --path .
```

### Prerequisites

grove requires the following tools to be installed and available on your `$PATH`:

| Tool | Purpose |
|------|---------|
| [git](https://git-scm.com) | Worktree management |
| [zellij](https://zellij.dev) or [tmux](https://github.com/tmux/tmux) | Terminal sessions |

grove auto-detects which multiplexer to use. If you're inside a zellij or tmux session it will use that one, otherwise it picks whichever binary it finds on `$PATH` first (zellij wins on a tie). You can override this in the config.

## Shell integration

grove provides a `gv` shell function for fast access. Add the appropriate line to your shell config:

**Zsh** (`~/.zshrc`):
```sh
eval "$(grove init zsh)"
```

**Bash** (`~/.bashrc`):
```sh
eval "$(grove init bash)"
```

**Fish** (`~/.config/fish/config.fish`):
```sh
grove init fish | source
```

Once set up, `gv` is your daily driver:

```sh
gv              # open a repo+branch interactively
gv myrepo main  # open a specific repo and branch directly
gv -l           # list all active sessions
gv -c           # close the current worktree and session
```

Shell completions are also available:

```sh
# Zsh
grove completions zsh > ~/.zfunc/_grove

# Bash
grove completions bash > /etc/bash_completion.d/grove

# Fish
grove completions fish > ~/.config/fish/completions/grove.fish
```

## Configuration

grove uses a TOML config file, typically at `~/.config/grove/config.toml`.

```toml
shell = "zsh"          # zsh | bash | fish  (auto-detected from $SHELL if unset)
multiplexer = "auto"   # auto | zellij | tmux

[directories]
work = { path = "/Users/you/work" }
oss  = { path = "/Users/you/oss"  }
```

The `multiplexer` field defaults to `"auto"`, which prefers the currently-running multiplexer (via `$ZELLIJ` / `$TMUX` env vars) and falls back to whichever binary is on `$PATH`.

The `[directories]` table defines named locations where grove clones and looks for bare repositories. When you run `grove clone`, you pick which directory to clone into.

```sh
grove config show    # print config path and current contents
grove config edit    # open in $EDITOR
```

## Commands

### `grove open [query] [branch]`

The core command. Opens a worktree and attaches to its zellij session, creating them if needed.

- If a session already exists, re-attaches to it.
- If a worktree already exists for the branch, reuses it.
- Both `query` and `branch` are fuzzy-interactive if omitted.

```sh
grove open              # fuzzy-select repo, then branch
grove open myrepo       # fuzzy-select branch for myrepo
grove open myrepo main  # go directly to myrepo/main
```

Branch selection includes all remote branches plus an option to create a new branch.

### `grove clone <url> [dir]`

Clones a repository as a bare repo into one of your configured directories.

```sh
grove clone git@github.com:org/repo.git work
grove clone https://github.com/org/repo   # interactive directory selection
```

### `grove repo`

Manage tracked repositories.

```sh
grove repo list          # list all repos (name, directory, frecency score, path)
grove repo add <path>    # track an existing bare repo
grove repo rm <name>     # stop tracking a repo
```

Repos are sorted by **frecency** — a combination of frequency and recency — so your most-used projects always appear at the top of the list.

### `grove tree`

Manage worktrees for a repository.

```sh
grove tree list [repo]    # list all worktrees for a repo
grove tree close [query]  # close a worktree and kill its session
grove tree prune [repo]   # prune stale worktree entries
```

`grove tree close` handles orphaned sessions — it will clean up a zellij session even if the worktree directory has already been removed.

### `grove session`

Manage zellij sessions directly.

```sh
grove session list           # list all active grove sessions
grove session attach <name>  # attach to a session by name
```

Sessions are named `<repo>/<branch>` (e.g., `myrepo/main`).

### `grove init <shell>`

Print the `gv` shell function. Pipe to your shell config or `eval` directly.

```sh
grove init zsh   # zsh | bash | fish
```

## How it works

grove stores repo metadata in a SQLite database (`~/.local/share/grove/grove.db`). When you open a repo+branch:

1. grove checks whether a worktree exists at `<bare-repo>/worktrees/<branch>`, creating it if not.
2. grove resolves which multiplexer to use (zellij or tmux).
3. grove checks whether a session named `<repo>/<branch>` (zellij) or `<repo>-<branch>` (tmux) already exists.
4. If the session exists, grove attaches to it. Otherwise, it creates a new session using the layout template.
5. The frecency score for the repo is incremented.

Both backends open a session with three named tabs/windows — `shell` (focused), `editor` (`nvim .`), and `opencode` — all with the working directory set to the worktree path.

The layout templates are customisable — place your own at `~/.config/grove/templates/layout.kdl` (zellij) or `~/.config/grove/templates/tmux.sh` (tmux) and grove will use them instead of the built-in defaults. Templates are rendered with [minijinja](https://github.com/mitsuhiko/minijinja) and have access to `worktree_path`, `shell`, `session_name`, `repo`, and `branch` variables.

## Contributing

Contributions are welcome. Please open an issue first to discuss significant changes.

```sh
git clone https://github.com/seftontycho/grove
cd grove
cargo build
cargo test
```

## License

MIT

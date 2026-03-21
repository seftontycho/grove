# Introduction

grove is a CLI tool that pairs **git worktrees** with **terminal multiplexer sessions** ([zellij](https://zellij.dev) or [tmux](https://github.com/tmux/tmux)) to make context-switching between projects and branches instant and frictionless.

## The problem

Switching between tasks is expensive. The typical workflow looks like this:

1. Stash or commit your current changes
2. Check out a different branch
3. Kill your editor (it's now pointing at the wrong files)
4. Relaunch your editor
5. Remember what you were doing

Do this a dozen times a day and you've lost an hour to overhead.

## The grove approach

grove gives every repo+branch combination its own **isolated workspace**:

- A **git worktree** — a separate working directory checked out to that branch, so you never need to stash anything
- A **multiplexer session** — a persistent terminal session with your full layout ready to go

Switching contexts becomes a single command: `gv`. grove drops you straight into the right session, creating the worktree and session if they don't exist yet.

grove works with both **zellij** and **tmux**, auto-detecting which one you're using.

By default, both zellij and tmux sessions open with three named tabs/windows:

| Tab/Window | Contents |
|------------|----------|
| `shell` | Your shell, focused by default |
| `editor` | `nvim .` |
| `opencode` | `opencode` |

These are just defaults — the layout is fully customisable. Place your own template at `~/.config/grove/templates/zellij.kdl` (zellij) or `~/.config/grove/templates/tmux.sh` (tmux) and grove will use it instead.

## Who is grove for?

grove is for developers who:

- Work across multiple repositories or many branches simultaneously
- Use [zellij](https://zellij.dev) or [tmux](https://github.com/tmux/tmux) as their terminal multiplexer
- Use [neovim](https://neovim.io) as their editor
- Want their environment to be ready instantly when they switch tasks

## Quick start

```sh
# Install
cargo install --git https://github.com/seftontycho/grove

# Set up the shell alias
eval "$(grove init zsh)"

# Configure your project directories
grove config edit

# Clone a repo
grove clone git@github.com:your-org/your-repo.git work

# Open a worktree + session
gv
```

See [Installation](./installation.md) for full setup instructions.

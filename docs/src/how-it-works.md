# How It Works

This page describes grove's internals for anyone who wants to understand what happens under the hood.

## Data storage

grove stores all repository metadata in a SQLite database:

| Platform | Path |
|----------|------|
| Linux | `~/.local/share/grove/grove.db` |
| macOS | `~/Library/Application Support/grove/grove.db` |

The database has a single `repos` table tracking name, path, remote URL, configured directory, status, frecency score, and timestamps. There is no network dependency — grove is entirely local.

## The `grove open` flow

When you run `grove open myrepo feat/auth`:

1. **Repo lookup** — grove queries the database for a repo matching `myrepo` (exact name first, then fuzzy substring ordered by frecency).
2. **Frecency update** — the repo's score is incremented and `last_accessed_at` is set.
3. **Multiplexer resolution** — grove checks which backend to use: the configured `multiplexer` value, or auto-detection via `$ZELLIJ` / `$TMUX` env vars and `$PATH`.
4. **Session check** — grove asks the multiplexer for its current session list and looks for a session named `myrepo/feat/auth` (zellij) or `myrepo-feat/auth` (tmux). If found, it attaches immediately.
5. **Worktree check** — grove runs `git worktree list` on the bare repo and checks whether a worktree for `refs/heads/feat/auth` already exists.
6. **Worktree creation** — if no worktree exists, grove runs `git worktree add <bare-repo>/worktrees/feat/auth feat/auth`.
7. **Session creation** — grove renders the layout template with minijinja, writes it to a temp file (zellij) or executes it as a shell script (tmux), then starts the new session.

## Bare repositories

grove clones repositories as **bare** repos (no working tree at the root). The repo itself lives at e.g. `/Users/you/work/myrepo`, and worktrees are created under `/Users/you/work/myrepo/worktrees/<branch>`.

This means you can have as many branches checked out simultaneously as you like, each in its own directory, without any conflicts.

## Session naming

| Backend | Session name format | Example |
|---------|-------------------|---------|
| zellij | `<repo>/<branch>` | `myrepo/feat/auth` |
| tmux | `<repo>-<branch>` | `myrepo-feat-auth` |

tmux does not allow `/` in session names, so grove substitutes `-`.

## Layout templates

Session layouts are rendered with [minijinja](https://docs.rs/minijinja/latest/minijinja/). For zellij, the rendered KDL is written to a temp file (`$TMPDIR/grove-<repo>-<branch>.kdl`) which is deleted immediately after the session is created. For tmux, the rendered shell script is executed directly via `sh -c`.

User-provided templates in `~/.config/grove/templates/` take precedence over the built-in defaults. See [Session Layout](./reference/layout.md) for the full variable reference.

## Config validation

On every startup, grove validates that all configured directory paths exist and are directories. Validation failures are printed to stderr as warnings but do not abort the program.

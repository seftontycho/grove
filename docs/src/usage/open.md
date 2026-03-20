# Opening a Workspace

`grove open` (or the `gv` shell alias) is the command you will use most. It opens a git worktree and a multiplexer session for a given repo and branch, creating them if they don't already exist.

## Usage

```sh
grove open [query] [branch]
gv [query] [branch]
```

Both arguments are optional. If omitted, grove presents an interactive fuzzy-select prompt.

## How selection works

### Repo selection

If `query` is provided, grove first tries an exact name match, then falls back to a fuzzy substring match ordered by frecency. If `query` is omitted, an interactive fuzzy list of all tracked repos is shown, also ordered by frecency.

### Branch selection

If `branch` is provided it is used directly. If omitted, grove lists all remote branches for the selected repo and presents them interactively. The list always includes a **[create new branch]** option at the top — selecting it prompts you to type a new branch name.

Remote branch names have the remote prefix stripped before use (e.g. `origin/feat/auth` becomes `feat/auth`).

## Session and worktree reuse

grove never creates duplicates:

- If a session named `<repo>/<branch>` (zellij) or `<repo>-<branch>` (tmux) already exists, grove attaches to it immediately without creating a new worktree.
- If a worktree already exists for the branch, grove reuses it and only creates a new session.

## Examples

```sh
# Fully interactive — pick repo, then pick branch
gv

# Select branch interactively for a specific repo
gv myrepo

# Go directly to a specific repo and branch
gv myrepo main
gv myrepo feat/new-feature

# Using grove directly instead of the gv alias
grove open myrepo main
```

## What gets opened

A new session is created with the default layout (three tabs/windows: `shell`, `editor`, `opencode`), all with the working directory set to the worktree path. The layout is customisable — see [Session Layout](../reference/layout.md).

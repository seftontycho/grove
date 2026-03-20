# Managing Worktrees

The `grove tree` subcommands let you inspect and clean up git worktrees for your tracked repositories.

## List worktrees

```sh
grove tree list [repo]
```

Lists all worktrees for a repository, including the bare root. If `repo` is omitted, grove presents an interactive repo selection prompt.

Example output:

```
  refs/heads/main [bare]  a1b2c3d4  /Users/you/work/myrepo
  refs/heads/feat/auth    e5f6a7b8  /Users/you/work/myrepo/worktrees/feat/auth
  refs/heads/hotfix-123   c9d0e1f2  /Users/you/work/myrepo/worktrees/hotfix-123
```

## Close a worktree

```sh
grove tree close [query]
```

Interactively select a worktree to close. grove will:

1. Kill the associated multiplexer session (both the zellij and tmux name formats are tried, so this works regardless of which backend created the session).
2. Remove the worktree directory via `git worktree remove`.
3. If the worktree directory is already missing, run `git worktree prune` to clean up the stale entry instead.

If `query` is omitted, an interactive fuzzy list shows all non-bare worktrees for the selected repo.

### Orphaned sessions

`grove tree close` also detects **orphaned sessions** — multiplexer sessions that exist for a repo but whose worktree directory has already been deleted. These appear in the selection list labelled `[orphaned session]` and can be killed directly.

## Prune stale entries

```sh
grove tree prune [repo]
```

Runs `git worktree prune` on the selected repository, removing any stale worktree metadata entries for directories that no longer exist on disk. If `repo` is omitted, grove presents an interactive selection prompt.

This is safe to run at any time and is a lighter alternative to `grove tree close` when you just want to clean up metadata without interactively choosing a specific worktree.

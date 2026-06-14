# Managing Worktrees

The `grove tree` subcommands let you inspect and clean up git worktrees for your tracked repositories.

## List worktrees

```sh
grove tree list [repo]
```

Lists all worktrees for a repository, including the bare root. If `repo` is omitted, grove presents an interactive repo selection prompt.

Example output:

```
  refs/heads/main [bare]  a1b2c3d4  /Users/you/work/myrepo/.bare
  refs/heads/feat/auth    e5f6a7b8  /Users/you/work/myrepo/feat/auth
  refs/heads/hotfix-123   c9d0e1f2  /Users/you/work/myrepo/hotfix-123
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
grove tree prune [repo] [--all] [--orphans] [--merged]
```

Runs `git worktree prune` on the selected repository, removing any stale worktree metadata entries for directories that no longer exist on disk. If `repo` is omitted, grove presents an interactive selection prompt. Pass `--all` to sweep every tracked repo.

This is safe to run at any time and is a lighter alternative to `grove tree close` when you just want to clean up metadata without interactively choosing a specific worktree.

### Cleaning up merged branches

By default `prune` only touches the filesystem — stale metadata, and (with `--orphans`) leftover directories that git no longer tracks. Add `--merged` to also clean up worktrees whose **branch is done**:

```sh
grove tree prune --merged [repo]
```

A worktree is a candidate when its branch is either:

- **merged** into the base branch (`origin/HEAD`, or the local default branch for repos with no remote), or
- **gone** — it tracked a remote branch that has since been deleted, the signal left behind by a squash-merge-then-delete (e.g. GitHub's default PR merge).

grove lists the candidates grouped by reason, prompts once, then removes each confirmed worktree along with its branch and multiplexer session.

Your local work is never at risk: removal goes through `git worktree remove` **without** `--force`, which refuses any worktree holding uncommitted or untracked files. Those are kept and reported rather than deleted. Merged branches are deleted with `git branch -d`; gone branches (whose content can't be verified locally) use `git branch -D` after the prompt.

# Repo Layout Overhaul: `.bare` Container Layout

**Date:** 2026-05-15
**Status:** Approved

## Problem

grove places worktrees at `<repo>/worktrees/<branch>`. For a bare repo the
repo path *is* `$GIT_DIR`, so that path is `$GIT_DIR/worktrees/<branch>` —
exactly the directory git reserves for each worktree's internal admin files
(`HEAD`, `index`, `gitdir`, `commondir`, `logs/`, `ORIG_HEAD`, `FETCH_HEAD`).

Confirmed in a live checkout: a worktree's `.git` file reads
`gitdir: .../worktrees/master`, pointing at itself. The working directory and
git's admin directory are the same folder, so all those plumbing files appear
as untracked entries in `git status`. That is the "messy git files" symptom.

Two further gaps motivated this work:

- There is no subcommand to create a brand-new repo in a usable state. `clone`
  needs a remote URL; `repo add` only tracks an existing repo.
- The `zj-session-bar` zellij plugin (used for in-session switching) is broken
  and should be removed.

## Goals

1. Eliminate the worktree/admin-dir collision by adopting a container layout.
2. Add a subcommand to create a new repo.
3. Support both the new and legacy layouts so existing tracked repos keep
   working, plus an opt-in migration command.
4. Remove the `zj-session-bar` plugin entirely.

## Non-goals

- Changing the SQLite schema.
- Changing how worktree directories are named relative to branch names
  (branch names containing `/` still nest as directories).
- Native in-session zellij switching without a plugin.
- Migrating the SQLite DB rows (the `path` column meaning shifts, but values
  for legacy repos stay valid).

## Part 1: The `.bare` container layout

Every grove-managed repo becomes a **container directory** that holds hidden
git data and one subdirectory per worktree:

```
<dir>/<name>/
├── .bare/         bare git data (objects, refs, config, HEAD…)
├── .git           file containing: "gitdir: ./.bare"
├── master/        worktree
└── feat-auth/     worktree
```

There is no privileged "primary" checkout — every branch is a uniform worktree
subdirectory, matching grove's existing "one directory per branch" model.

### `RepoLayout`

A new type in `src/git.rs` resolves the on-disk layout from a tracked
`repo.path`:

```rust
pub enum RepoLayout {
    /// New layout: `<container>/.bare` is the git dir.
    Container { container: PathBuf },
    /// Legacy layout: `<bare>` is itself a bare repo.
    LegacyBare { bare: PathBuf },
}
```

Detection — `RepoLayout::detect(repo_path: &Path) -> Result<RepoLayout>`:

- If `<repo_path>/.bare` exists and is a git directory → `Container`.
- Else if `<repo_path>` is itself a bare repo (`HEAD` + `objects/` present, or
  `git rev-parse --is-bare-repository` is true) → `LegacyBare`.
- Else → error: not a grove-managed repo.

`RepoLayout` exposes:

- `git_dir() -> PathBuf` — `<container>/.bare` or `<bare>`.
- `worktree_base() -> PathBuf` — `<container>` or `<bare>/worktrees`.
- `worktree_path(branch: &str) -> PathBuf` — `worktree_base().join(branch)`.

### `git.rs` changes

The worktree functions currently take `repo_path` and run `git` with
`current_dir(repo_path)`. They are reworked to take a `&RepoLayout`:

- `worktree_add` — places the worktree at `layout.worktree_path(branch)`, runs
  `git` from `layout.git_dir()`.
- `worktree_list`, `worktree_remove`, `worktree_prune`, `list_remote_branches`
  — run `git` from `layout.git_dir()`.

In the container layout the worktree checkout (`<container>/<branch>`) and
git's admin directory (`<container>/.bare/worktrees/<id>`) no longer overlap,
which is the core fix.

### Caller changes

`open.rs` and `tree.rs` call `RepoLayout::detect(&repo.path)` once and pass the
result into the `git.rs` functions instead of assuming `repo.path` is the git
directory.

`repo.path` stored in the DB is the **container directory** for new repos and
the bare directory for legacy repos. No schema change; the `path` column holds
whichever is correct, and `detect` disambiguates.

## Part 2: `grove clone` — updated

For new clones (`RepoCmd`/`Cmd::Clone`), the container layout is produced:

1. Create `<dir>/<name>/`.
2. `git clone --bare <url> <dir>/<name>/.bare`.
3. Write `<dir>/<name>/.git` containing `gitdir: ./.bare`.
4. Fix the fetch refspec:
   `git --git-dir=.bare config remote.origin.fetch '+refs/heads/*:refs/remotes/origin/*'`.
5. `git --git-dir=.bare fetch origin`.

Step 4–5 matter: a plain `git clone --bare` maps remote branches into local
`refs/heads/*`, so `git branch -r` lists nothing and `grove open`'s branch
picker is empty. The corrected refspec creates `refs/remotes/origin/*`
tracking branches so the picker works. This fixes a latent bug.

`repo.path` recorded in the DB is the container directory.

## Part 3: `grove repo new <name> [dir]` — new subcommand

Creates a new repo in a usable state.

- `name` — repo name (becomes the container directory name).
- `dir` — a configured directory; if omitted, interactive `FuzzySelect`
  (reuse the picker from `clone.rs`).
- Resolve the parent via `Config::resolve_dir`; create it if absent.
- Fail if `<parent>/<name>` already exists.
- Create the container, then `git init --bare --initial-branch=master .bare`.
- Write the `.git` file (`gitdir: ./.bare`).
- Seed **one empty commit** on `master` using plumbing (no temp worktree):
  - write the empty tree: `git --git-dir=.bare hash-object -w -t tree --stdin`
    with empty stdin;
  - create the commit:
    `git --git-dir=.bare commit-tree <tree> -m "Initial commit"`;
  - point the branch at it:
    `git --git-dir=.bare update-ref refs/heads/master <commit>`.
- Track in the DB (`url = None`, `directory = Some(dir)`,
  `path = <parent>/<name>`).

The default branch is `master`. Seeding an empty commit keeps `grove open`
uniform — it can `git worktree add` for any repo without an empty-repo
special case.

CLI: add `RepoCmd::New { name: String, dir: Option<String> }`.

## Part 4: `grove repo migrate [name]` — new subcommand

Converts a legacy bare repo in place to the container layout.

- `name` — repo to migrate; interactive `FuzzySelect` if omitted.
- Resolve the repo and `RepoLayout::detect` it. If already `Container`, print
  a friendly no-op message and return.
- If the current working directory is inside the repo being migrated, warn and
  refuse (its path would change underneath the shell).
- Check every worktree for uncommitted changes. If any are dirty, list them
  and refuse — the user commits or stashes first.
- Migrate the bare data:
  1. Rename the bare directory aside (e.g. to a sibling temp name).
  2. Create the container directory at the original path.
  3. Move the bare directory into it as `.bare/`.
  4. Write the `.git` file (`gitdir: ./.bare`).
- Prune old worktree entries: branches live in the bare data and are
  preserved; worktrees are recreated on demand by `grove open`. Disentangling
  the collided legacy worktree directories is not attempted.
- Print a summary: branches preserved, re-open worktrees with `grove open`.

CLI: add `RepoCmd::Migrate { name: Option<String> }`.

## Part 5: Remove `zj-session-bar` (zellij only)

The plugin is removed entirely, including the in-session pipe mechanism.

- `templates/zellij.kdl` — remove the `zj-session-bar` plugin `pane` from
  `default_tab_template`; keep the `zellij:tab-bar` and `zellij:status-bar`
  panes.
- `src/zellij.rs` — delete `plugin_url`, `pipe_switch_session`,
  `pipe_create_session`, and the `$ZELLIJ`-environment branches in
  `create_session` / `attach_session`.
- When `$ZELLIJ` is set, `create_session` and `attach_session` bail with:
  *"You're inside a zellij session — detach first (Ctrl-o d), then run grove
  again."*
- tmux is unaffected: it already switches in-session via
  `tmux switch-client`.
- Remove now-unused imports (e.g. the `directories` usage in `zellij.rs`;
  verify the crate is still used elsewhere before touching `Cargo.toml`).
- Update README and `docs/src` references to in-session switching / the
  plugin.

## Code organization

`src/cmd/repo.rs` would grow past ~250 lines with `new` and `migrate` added.
It is split into a `src/cmd/repo/` module:

```
src/cmd/repo/
├── mod.rs       re-exports; shared RepoRow table type
├── add.rs
├── new.rs
├── migrate.rs
├── rm.rs
└── list.rs
```

## Files touched

- `src/cli.rs` — add `RepoCmd::New` and `RepoCmd::Migrate`.
- `src/cmd/mod.rs`, `src/main.rs` — wire the new subcommands.
- `src/cmd/repo.rs` → `src/cmd/repo/` module.
- `src/git.rs` — add `RepoLayout`; rework worktree functions; container-aware
  `clone` and a new `init` helper; `.git`-file / empty-commit / refspec
  helpers.
- `src/cmd/clone.rs` — produce the container layout.
- `src/cmd/open.rs`, `src/cmd/tree.rs` — detect and pass `RepoLayout`.
- `src/zellij.rs` — remove plugin code; add the `$ZELLIJ` guard.
- `templates/zellij.kdl` — remove the plugin pane.
- `README.md`, `docs/src/**` — update layout/usage docs (`reference/layout.md`,
  `usage/clone.md`, `usage/repos.md`, `usage/worktrees.md`).

No SQLite schema change.

## Testing

- `src/git.rs` — unit tests for `RepoLayout::detect` (container, legacy bare,
  not-a-repo) and worktree-path resolution for both layouts.
- `repo new` / `repo migrate` / `clone` — tests against temporary directories
  using the real `git` binary (the codebase already shells out to `git`).
  `repo new` test asserts the container layout, the `.git` file, and a single
  commit on `master`. `migrate` test builds a legacy bare repo, migrates it,
  and asserts the container layout with branches preserved.
- zellij — assert the rendered `zellij.kdl` template contains no plugin
  reference; test the `$ZELLIJ` guard returns the expected error.

## Open risks

- `repo migrate` performs directory renames of git data; it must verify
  success at each step and leave the repo recoverable if interrupted.
- Branch names containing `/` create nested worktree directories
  (`feat/auth/`); this is unchanged from current behavior and accepted.

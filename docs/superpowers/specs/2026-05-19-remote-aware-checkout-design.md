# Remote-Aware Worktree Checkout

**Date:** 2026-05-19
**Status:** Approved

## Problem

`grove open`'s branch picker lists remote-tracking refs (e.g. `origin/feat`)
and lets the user select one, but the path from selection to worktree has
three concrete problems:

1. **No fetch.** Newly-pushed remote branches never appear until the user
   manually runs `git fetch` in the bare repo.
2. **No upstream tracking.** Selecting `origin/feat` runs
   `git worktree add -B feat <path>`. Without a start-point, `-B` creates
   `feat` from HEAD (whatever the bare repo's HEAD points at, typically
   `master`) — the remote work is silently lost and no upstream is set.
3. **Existing branches get reset.** The same `-B` flag also resets local
   branches that already exist. A user who happens to have a local `feat`
   and re-picks it loses uncommitted work and any divergent commits.

A fourth, smaller issue: the picker is remote-only, so checking out an
existing local branch goes through "[create new branch]" and hits the
same `-B` reset.

## Goals

1. Auto-fetch before the interactive branch picker so the list is fresh.
2. Show local and remote branches together in one picker, clearly labelled.
3. Set upstream tracking when checking out a remote branch.
4. Stop resetting branches that already exist.
5. Make the positional form (`grove open <repo> <branch>`) resolve sensibly
   whether the user typed a local name, a remote ref, or a brand-new name.

## Non-goals

- A new `grove tree checkout` subcommand.
- `--remote` / `--fetch` flags on `grove open`.
- A standalone `grove fetch` command.
- Choosing the start-point of a brand-new branch (always HEAD).
- Changing how worktree directories are named or laid out on disk.
- Caching the fetch result; every interactive run re-fetches.

## Design

### Auto-fetch

When `grove open` enters the interactive branch picker (i.e. no `branch`
argument was supplied), run `git fetch --all --prune` against the repo's
`.bare` directory before listing branches.

Fetch failure is non-fatal: print a one-line warning to stderr via
`eprintln!("Warning: fetch failed: {e:#}")` (matching the existing warning
style in `migrate_to_container`) and continue with whatever local +
cached-remote refs are present.

The positional form (`grove open <repo> <branch>`) does NOT auto-fetch.
The user has been explicit about which branch they want; fetching there
would slow down the common case and is rarely needed. If they need fresh
state they can run the picker instead.

### Unified picker

Replace the current remote-only list with a single ordered list:

```
[new]    + create new branch
[local]  master
[local]  feat/auth
[remote] origin/feat/billing
[remote] origin/release/1.2
[remote] upstream/main
```

Construction rules:

- `[new]    + create new branch` is always the first entry.
- All local branches follow, sorted alphabetically, each prefixed `[local]  `.
- Remote refs follow, sorted alphabetically, each prefixed `[remote] `, but
  any remote ref whose branch part matches an existing local branch name is
  omitted. The local entry is the canonical one for that name.
- Remote refs are split on the FIRST `/`: everything before is the remote
  name, everything after is the branch name. So `origin/release/1.2` has
  remote `origin` and branch `release/1.2`.
- Symbolic remote refs (e.g. `refs/remotes/origin/HEAD`) are filtered out
  before the picker is built. They're pointers to other refs, not separate
  branches. Concretely, `git branch -r --format=%(refname:short)` outputs
  the symbolic ref as the bare remote name (`origin`) because git strips
  the `/HEAD` tail from the short name; we filter both `<no-slash>` entries
  and any `*/HEAD` entries as a defensive belt-and-braces.

The display label is what the user sees; selection returns a typed
`BranchSource` (see below), not the label string.

### Branch source

```rust
pub enum BranchSource {
    /// Existing local branch — use as-is.
    Local(String),
    /// Remote-tracking ref. `local` is the branch name to create locally,
    /// `upstream` is the full remote ref (e.g. "origin/feat").
    Remote { local: String, upstream: String },
    /// Brand-new branch from HEAD.
    New(String),
}
```

`BranchSource::branch_name()` returns the local branch name in all three
cases — used for the session name and the worktree path.

### Per-selection git commands

| Selection                              | Git command                                       |
|----------------------------------------|---------------------------------------------------|
| `Local("feat")`                        | `git worktree add <path> feat`                    |
| `Remote { local: "feat", upstream: "origin/feat" }` | `git worktree add -b feat <path> origin/feat` |
| `New("mything")`                       | `git worktree add -b mything <path>`              |

`<path>` is always `RepoLayout::worktree_path(branch_name)`.

The current `git::worktree_add(repo, branch)` is replaced with
`git::worktree_add(repo, branch, source: WorktreeSource)`. `WorktreeSource`
mirrors `BranchSource` but without the branch name (which is the second
parameter):

```rust
pub enum WorktreeSource {
    ExistingLocal,
    TrackingRemote { upstream: String },
    NewFromHead,
}
```

Splitting the source from the branch name keeps the function signature
focused: every variant produces a worktree for `branch` at the layout's
worktree path.

### Positional resolution

`grove open <repo> <branch>` resolves `<branch>` to a `BranchSource` in
this order:

1. If `<branch>` exactly matches a local branch → `Local(<branch>)`.
2. If `<branch>` exactly matches a remote ref (e.g. `origin/feat` matches
   the listed ref `origin/feat`) → `Remote { local: <rest>, upstream: <branch> }`
   where `<rest>` is the part after the first `/`.
3. If `<branch>` is of the form `<remote>/<rest>` where `<remote>` is a
   configured remote name (from `git remote`) and the ref doesn't exist,
   reject with `unknown branch <branch>` — don't silently invent a new
   branch with a slash in its name.
4. Otherwise → `New(<branch>)`.

Step 3 catches the case where a user typos a remote branch (e.g.
`origin/fet` instead of `origin/feat`) and would otherwise end up with a
new branch named `origin/fet` checked out at `<container>/origin/fet`. The
nested directory would mostly work but is almost certainly not what they
meant.

### Session reuse short-circuit

The existing "session already exists → attach" check runs before branch
resolution. That stays as-is: if the user typed `grove open repo branch`
and the session is already up, we attach without fetching, resolving, or
touching git.

## Implementation

### `src/git.rs`

- New `pub fn fetch_all(repo_path: &Path) -> Result<()>` — runs
  `git fetch --all --prune` in `layout.git_dir()`.
- New `pub fn list_local_branches(repo_path: &Path) -> Result<Vec<String>>`
  — runs `git branch --format=%(refname:short)`.
- New `pub fn list_remotes(repo_path: &Path) -> Result<Vec<String>>` —
  runs `git remote`. Used by the positional resolver to validate
  `<remote>/<rest>` typos.
- Update `pub fn list_remote_branches` to drop `<remote>/HEAD` entries
  before returning. Filtering at the source keeps both the picker and
  the positional resolver consistent.
- Replace `pub fn worktree_add(repo_path, branch)` with
  `pub fn worktree_add(repo_path, branch, source: WorktreeSource)`.
- Add `pub enum WorktreeSource { ExistingLocal, TrackingRemote { upstream: String }, NewFromHead }`.

### `src/cmd/open.rs`

- Add `enum BranchSource { Local(String), Remote { local: String, upstream: String }, New(String) }`
  with a `branch_name(&self) -> &str` method.
- Replace `select_or_create_branch` with `select_branch_source(&repo) -> Result<BranchSource>`:
  - `git::fetch_all(&repo.path)` — warn on failure, don't bail.
  - `git::list_local_branches(&repo.path)`
  - `git::list_remote_branches(&repo.path)` (already exists)
  - Build display list + parallel `Vec<BranchSource>`.
  - `FuzzySelect` over the display list, return the matching source.
  - For `New`, prompt for the name via `dialoguer::Input` as today.
- New `resolve_branch_arg(&repo, &str) -> Result<BranchSource>` implementing
  the 4-step positional resolution.
- `run` calls one of the two resolvers, then dispatches:
  ```rust
  let source = match source {
      BranchSource::Local(_) => WorktreeSource::ExistingLocal,
      BranchSource::Remote { upstream, .. } => WorktreeSource::TrackingRemote { upstream },
      BranchSource::New(_) => WorktreeSource::NewFromHead,
  };
  let path = git::worktree_add(&repo.path, branch_source.branch_name(), source)?;
  ```
- `find_existing_worktree` stays as-is; the early-return for an existing
  worktree happens before `worktree_add` is called.

### Display formatting

Picker entries use a fixed-width prefix so columns align under fuzzy
filtering. Concretely:

```
[new]    + create new branch
[local]  <branch>
[remote] <remote>/<branch>
```

Width-3 brackets plus one trailing space. Pure presentation; no logic
depends on the label format.

## Edge cases

- **No remotes configured.** `list_remote_branches` returns empty; picker
  shows `[new]` + locals only.
- **Empty repo (no branches).** Picker shows only `[new]`.
- **Fetch fails (offline).** Warning printed; picker still shows whatever
  locals + cached remotes exist.
- **Local branch shadows remote.** Per the construction rule the remote
  entry is hidden. The user picks the local one, which doesn't reset.
  To get fresh remote commits the user pulls inside the worktree as
  normal.
- **Worktree already exists for branch.** Existing `find_existing_worktree`
  path returns the path; no `git worktree add` runs. This works for all
  three `WorktreeSource` variants because the variant only matters when
  creating.
- **`branch` argument is `<remote>/<branch>` typo.** Step 3 of positional
  resolution rejects with a clear error rather than creating a slash-named
  branch.
- **Branch names containing `/`.** Both local (`feat/auth`) and remote
  (`origin/release/1.2`) work — the split-on-first-`/` rule preserves the
  remainder verbatim. Worktree path becomes `<container>/feat/auth` which
  the existing layout already supports (it's how the legacy → container
  migration left things).

## Tests

### `src/git.rs`

- `fetch_all_against_local_source`: init a bare repo as a fake remote with
  one branch, clone it via `clone_bare`, push a new commit + branch to
  the source, call `fetch_all` on the clone, assert the new ref appears
  under `refs/remotes/origin/`.
- `list_local_branches_returns_seeded_branches`: `init_repo` then
  `worktree_add(.., NewFromHead)` for two branches; assert both names
  come back from `list_local_branches`.
- `worktree_add_tracking_sets_upstream`: clone, fetch, call
  `worktree_add(repo, "feat", TrackingRemote { upstream: "origin/feat" })`,
  assert `git config branch.feat.remote == "origin"` and
  `git config branch.feat.merge == "refs/heads/feat"`.
- `worktree_add_existing_local_does_not_reset`: create a local branch
  with a unique commit, then call `worktree_add(.., ExistingLocal)` for
  it. Assert the worktree's HEAD points at that unique commit, not at
  the bare repo's default HEAD.
- `worktree_add_new_from_head_creates_branch`: replaces the current
  `worktree_add_places_worktree_at_container_root` semantics.

### `src/cmd/open.rs`

- Pure picker-building logic (given `(locals, remotes)` → `Vec<(label, BranchSource)>`)
  pulled into a small `fn build_picker_entries(...)` so it's unit-testable
  without dialoguer.
- Tests for:
  - Ordering: `[new]` first, locals before remotes, alphabetical within
    each group.
  - Shadowing: local `feat` + remote `origin/feat` → only the local entry
    appears.
  - Multi-remote: `origin/feat` and `upstream/feat` both listed if no
    local `feat` exists.
  - Branch with `/`: `origin/release/1.2` → label `[remote] origin/release/1.2`,
    source `Remote { local: "release/1.2", upstream: "origin/release/1.2" }`.
  - HEAD filtering: input including `origin/HEAD` produces no `[remote] origin/HEAD`
    entry.

- Positional resolver tests over a real test repo:
  - Local match → `Local`.
  - Exact remote ref match → `Remote { local: stripped, upstream: full }`.
  - `<known-remote>/<unknown>` → error.
  - Unknown name (no slash) → `New`.

## Migration

No data migration. The change is purely behavioural inside
`grove open`. Existing worktrees, sessions, and DB rows are unaffected.

Users who relied on the old `-B` reset to wipe a divergent local branch
have lost that path; the new behaviour for `[local] feat` is "open the
worktree as-is". This is intentional — silent resets were the bug, not
the feature.

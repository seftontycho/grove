# Prune Merged / Stale-Branch Worktrees

**Date:** 2026-06-14
**Status:** Approved

## Problem

`grove prune` cleans up worktrees by looking at the **filesystem**:

- A *stale* worktree is one whose checkout directory no longer exists on
  disk (`is_stale` in `src/cmd/tree.rs`). Prune kills its session and runs
  `git worktree prune` to drop git's dangling record.
- `--orphans` handles the inverse — a directory on disk that git tracks no
  worktree for — and prompts before deleting.

Neither path catches the most common kind of clutter: a worktree that is
**perfectly healthy on disk and tracked by git**, but whose **branch is
done** — merged into the default branch, or merged-and-deleted on the
remote (the GitHub squash-merge-then-delete workflow). git still considers
these live worktrees, so they accumulate indefinitely. The user has to find
and remove each one (worktree + branch + multiplexer session) by hand.

## Goals

1. Detect worktrees whose branch is "done" and offer to clean them up:
   the worktree checkout, its branch ref, and its multiplexer session.
2. Never destroy uncommitted or untracked work — this is a hard constraint.
3. Distinguish high-confidence cases (provably merged) from heuristic ones
   (remote branch deleted, content unverifiable locally) and apply
   appropriate confirmation to each.
4. Compose with the existing `--all` (every repo) and `--orphans` flags.

## Non-goals

- Changing the default `grove prune` behaviour. The new work is opt-in
  behind `--merged`, matching the `--orphans` precedent, because deleting
  *live* worktrees and branches is more destructive than today's
  filesystem-record cleanup.
- A standalone branch-cleanup command independent of worktrees.
- Pruning branches that have **no** worktree (this feature is worktree-scoped;
  bare merged branches are out of scope).
- Caching or persisting any "merged" state; every run recomputes from git.
- Cleaning up remotes or running `git gc`.

## Design

### Invocation

A new opt-in flag on the existing subcommand:

```
grove prune --merged [repo]
grove prune --merged --all
grove prune --merged --orphans          # additive with orphan-dir cleanup
```

`--merged` is additive: when set, prune does its existing filesystem-stale
pass (and `--orphans` pass, if given) **and** the merged-branch pass. With
`--all`, the merged pass runs for every active repo.

### Core principle: lean on git's own safety valves

The hard "never delete local work" constraint is satisfied by *construction*
rather than by a hand-rolled dirtiness check, because two git commands
already refuse unsafe operations by default. Both were verified empirically:

- **`git worktree remove` (without `--force`)** refuses if the working tree
  has **any** uncommitted changes **or** untracked files
  (`fatal: '<path>' contains modified or untracked files`). This is exactly
  the user's constraint.
- **`git branch -d`** refuses to delete a branch git can't prove is merged.

So the pipeline casts a **broad net** to find candidate "done" worktrees,
then lets git's **narrow, conservative removal** reject anything unsafe, and
**reports** what was removed and what was skipped and why. A false positive
in detection can never lose work — at worst a candidate is listed and then
skipped when git refuses to remove it.

### A subtlety: `git branch -d` is *not* a "merged into base" gate

`git branch -d` deletes a branch that is merged into **HEAD _or_ its
upstream**. Verified: a `feat` branch merged to `origin/feat` but *not* to
`main` is still deleted by `-d` (with only a warning). So grove must do its
**own** ancestor check against the base branch; it cannot delegate the
"is this in base?" decision to `-d`. `-d` is used only as the deletion
*mechanism* once grove has classified the branch itself.

### Base branch resolution

The "merged" check needs a base ref. Resolve it per repo:

1. `git symbolic-ref refs/remotes/origin/HEAD` → strip `refs/remotes/` →
   e.g. `origin/main`. This is set by `git clone --bare` and survives
   grove's clone path.
2. Fallback for origin-less repos (created by `grove repo new`):
   `git symbolic-ref --short HEAD` → e.g. `master`.

### Classification

After a best-effort fetch (below), list worktrees and classify each
**non-bare** worktree whose branch is **not** the base branch:

- **Merged** — `git merge-base --is-ancestor <branch> <base>` exits 0. The
  branch's commits are fully contained in base. High confidence.
- **Gone** — the branch's `%(upstream:track)` is `[gone]`
  (`git for-each-ref --format='%(upstream:track)' refs/heads/<branch>`),
  meaning it tracked a remote branch that has since been deleted. This is
  the squash-merge signal; the content can't be verified locally.
- **Neither** — not a candidate; skipped silently.

The base branch's own worktree is never a candidate (it would trivially be
"merged into itself"). The worktree the user is currently inside is also
never removable — git refuses to remove the current worktree, so it surfaces
as a skip.

### Best-effort fetch

`[gone]` is only accurate against fresh remote-tracking refs. When `--merged`
is set, run `git fetch --all --prune` (reuse `git::fetch_all`) once per repo
before classification. Failure is non-fatal: print
`eprintln!("Warning: fetch failed: {e:#}")` (matching the existing warning
style) and continue with whatever refs are cached — the "merged" tier still
works offline; only "gone" detection may be stale.

### Confirmation

Candidates are presented grouped by tier and confirmed before any deletion,
reusing `dialoguer` to match the existing `--orphans` flow:

```
Merged into origin/main (safe to delete):
  feat/login
  fix/typo
Upstream deleted — likely squash-merged, branch delete will be forced:
  feat/billing

Delete the 3 worktree(s) above (worktree + branch + session)? [y/N]
```

A single `Confirm` gate covers the listed set. The "gone" group is labelled
explicitly so the user understands those branch deletions cannot be verified
locally and will use `-D`.

### Removal

For each confirmed candidate:

1. **Kill sessions** — best-effort `mux.kill_session` for both the zellij
   (`repo/branch`) and tmux (`repo-branch`) name forms, reusing the existing
   helper pattern in `prune_repo`.
2. **Remove the worktree** — `git worktree remove <path>` **without
   `--force`**. On failure (dirty/untracked), skip this candidate and record
   it as "kept (uncommitted changes)"; do **not** delete its branch.
3. **Delete the branch** — only if the worktree was removed:
   - Merged tier → `git branch -d <branch>` (guaranteed to succeed, since
     grove already proved it's an ancestor of base).
   - Gone tier → `git branch -D <branch>` (force; `-d` would refuse a
     squash-merged branch whose commits aren't in base).
4. After processing all candidates, run `git worktree prune` to tidy any
   records left dangling.

### Output

Extend the existing one-line summary, e.g.:

```
Pruned 0 stale worktree(s), removed 2 merged + 1 gone worktree(s), \
kept 1 (uncommitted changes) for 'grove'
```

Skipped candidates are always reported with their reason so a "kept" worktree
is never silent.

## Safety invariants

| Risk                                   | Guard                                              | Verified |
|----------------------------------------|----------------------------------------------------|----------|
| Lose uncommitted / untracked work      | `worktree remove` without `--force`                | yes      |
| Delete a branch not actually in base   | grove's own `merge-base --is-ancestor` check       | yes      |
| Force-delete an unverifiable branch    | `-D` only in the gone tier, only after explicit confirm | n/a |
| Remove the default branch's worktree   | base branch excluded from candidates               | yes      |
| Remove the current worktree            | git refuses; surfaced as a skip                    | yes      |

## Implementation

### `src/git.rs`

- `pub fn resolve_base_branch(repo_path: &Path) -> Result<String>` — try
  `git symbolic-ref refs/remotes/origin/HEAD` (strip `refs/remotes/`);
  fall back to `git symbolic-ref --short HEAD`.
- `pub fn is_branch_merged(repo_path: &Path, branch: &str, base: &str) -> Result<bool>`
  — `git merge-base --is-ancestor refs/heads/<branch> <base>`; exit 0 → true,
  exit 1 → false, other → error.
- `pub fn branch_upstream_gone(repo_path: &Path, branch: &str) -> Result<bool>`
  — `git for-each-ref --format='%(upstream:track)' refs/heads/<branch>`;
  true iff the trimmed output is `[gone]`.
- `pub fn delete_branch(repo_path: &Path, branch: &str, force: bool) -> Result<()>`
  — `git branch -d`/`-D <branch>` in `layout.git_dir()`.
- Reuse existing `worktree_list`, `worktree_remove` (already force-free),
  `worktree_prune`, `fetch_all`.

### `src/cmd/tree.rs`

- Add a classification enum, e.g.
  `enum DoneReason { Merged, Gone }`, and a helper that maps a `Worktree`
  (plus resolved base) to `Option<DoneReason>`, skipping bare entries and
  the base branch.
- New `fn prune_merged(repo: &Repo, mux: &dyn Multiplexer) -> Result<MergedOutcome>`:
  best-effort `fetch_all` → resolve base → classify → confirm → remove
  (sessions, worktree, branch) → `worktree_prune`. Returns counts of
  merged/gone removed and skipped-with-reason.
- Thread a `merged: bool` parameter through `prune` and `prune_repo`; when
  set, run `prune_merged` after the existing stale pass.
- Extend `PruneOutcome` (or compose a second outcome struct) so `summary`
  reports merged/gone/kept counts alongside the existing stale/orphan ones.

### `src/cli.rs`

- Add `--merged` to the `Prune` subcommand, documented as "Also remove
  worktrees whose branch has been merged or deleted upstream (worktree +
  branch + session; prompts before deleting)."

### `src/main.rs`

- Pass the new `merged` flag through to `cmd::tree::prune`.

## Edge cases

- **No origin (repo created by `repo new`).** `resolve_base_branch` falls
  back to local `HEAD`; "merged" detection works, "gone" detection finds no
  upstreams and reports nothing — correct.
- **Offline / fetch fails.** Warn and continue; "merged" still works, "gone"
  may be stale. No crash.
- **Dirty or untracked-only worktree on a merged branch.** Listed as a
  candidate, but `worktree remove` refuses, so it's kept and reported; its
  branch is not deleted.
- **Branch merged to its upstream but not to base.** Classified by grove's
  own ancestor check, so it is **not** in the merged tier on that basis. It
  only appears if its upstream is `[gone]` (gone tier) — never silently
  deleted as "merged".
- **Detached-HEAD worktree (no branch).** No branch to classify; never a
  candidate.
- **Squash-merged branch whose remote still exists.** Neither merged
  (not an ancestor) nor gone (upstream present) → not detected. Accepted
  limitation: without the remote-deletion signal there is no safe local
  proof the work landed. Documented, not worked around.
- **Branch names containing `/`** (e.g. `feat/login`). All git invocations
  use the full `refs/heads/<branch>` form or pass the name verbatim, so
  nested names work; the worktree path is the existing layout path.
- **Base branch worktree.** Excluded from candidates by name comparison.

## Tests

### `src/git.rs`

Built on the existing `init_bare_at` + `seed_branch` + `clone_bare` helpers.

- `is_branch_merged_true_for_ancestor`: seed `feat` as an ancestor of
  `master` (fast-forward merge), assert `is_branch_merged` is true.
- `is_branch_merged_false_for_divergent`: seed `feat` with a commit not in
  `master`, assert false.
- `branch_upstream_gone_detects_deleted_remote`: clone a source with `feat`,
  create a tracking local `feat`, delete `feat` on the source, `fetch_all`,
  assert `branch_upstream_gone` is true; assert false for a branch whose
  upstream still exists and for one with no upstream.
- `resolve_base_branch_prefers_origin_head`: clone, assert `origin/<default>`;
  for an `init_repo` repo with no origin, assert the local default name.
- `delete_branch_force_removes_unmerged`: seed a divergent `feat`, assert
  `delete_branch(.., force=false)` errors and `force=true` succeeds.

### `src/cmd/tree.rs`

Mirroring the existing `tree.rs` tests, which are pure-function only
(`is_stale`), the tier logic is extracted so it can be tested without
spawning git or a multiplexer:

- Pull classification into
  `fn classify_worktree(wt: &Worktree, base: &str, merged: bool, gone: bool) -> Option<DoneReason>`
  (taking the already-computed merged/gone booleans):
  - bare entry → `None`.
  - base branch → `None`.
  - merged → `Some(Merged)`.
  - gone (not merged) → `Some(Gone)`.
  - neither → `None`.

There is no mock multiplexer in the suite today, and session-killing in the
existing `prune_repo` is an untested best-effort step. The end-to-end
removal behaviour (worktree removed with `-d`, dirty worktree kept, base
branch never removed) is therefore covered by the git-touching tests in
`src/git.rs` against real `init_bare_at` repos, not by a `tree.rs`
integration test. Introducing a `Multiplexer` test double is out of scope
for this change.

## Migration

No data migration. No schema or on-disk layout change. The feature adds an
opt-in flag and new behaviour; existing worktrees, sessions, and DB rows are
untouched when `--merged` is not passed.

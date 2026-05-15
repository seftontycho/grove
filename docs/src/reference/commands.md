# Command Reference

## `grove open`

Open a worktree and multiplexer session.

```
grove open [query] [branch]
```

| Argument | Description |
|----------|-------------|
| `query` | Repo name or fuzzy substring. Interactive if omitted. |
| `branch` | Branch name. Interactive if omitted. Includes a "[create new branch]" option. |

Reuses an existing worktree and/or session if they already exist.

---

## `grove clone`

Clone a remote repository into a configured directory, using the .bare container layout.

```
grove clone <url> [dir]
```

| Argument | Description |
|----------|-------------|
| `url` | Git remote URL (SSH or HTTPS) |
| `dir` | Named directory from config. Interactive if omitted (skipped if only one directory is configured). |

Automatically tracks the cloned repo in the database.

---

## `grove repo`

### `grove repo list`

Print a table of all tracked repositories sorted by frecency.

```
grove repo list
```

### `grove repo new`

Create a new, empty repository in the `.bare` container layout. The repo is
seeded with a single empty commit on `master` so it is immediately usable.

```
grove repo new <name> [dir]
```

| Argument | Description |
|----------|-------------|
| `name` | Name of the repository (becomes the container directory name) |
| `dir` | Named directory from config. Interactive if omitted. |

### `grove repo migrate`

Convert a legacy bare repository to the `.bare` container layout, in place.
Existing worktrees are preserved — each is relocated into the new layout with
its uncommitted changes intact. A worktree that cannot be relocated
automatically is left for `grove open` to recreate.

```
grove repo migrate [name]
```

| Argument | Description |
|----------|-------------|
| `name` | Repo name. Interactive if omitted. |

### `grove repo add`

Track an existing local repository.

```
grove repo add <path>
```

| Argument | Description |
|----------|-------------|
| `path` | Absolute or relative path to a git repository on disk |

### `grove repo rm`

Stop tracking a repository. Does not touch the files on disk.

```
grove repo rm <name>
```

| Argument | Description |
|----------|-------------|
| `name` | Exact repository name as shown in `grove repo list` |

---

## `grove tree`

### `grove tree list`

List all worktrees for a repository.

```
grove tree list [repo]
```

| Argument | Description |
|----------|-------------|
| `repo` | Repo name or fuzzy substring. Interactive if omitted. |

### `grove tree close`

Close a worktree and kill its multiplexer session.

```
grove tree close [query]
```

| Argument | Description |
|----------|-------------|
| `query` | Repo name or fuzzy substring. Interactive if omitted. |

Also surfaces orphaned sessions (session exists but worktree directory is gone).

### `grove tree prune`

Prune stale worktree entries via `git worktree prune`.

```
grove tree prune [repo]
```

| Argument | Description |
|----------|-------------|
| `repo` | Repo name or fuzzy substring. Interactive if omitted. |

---

## `grove session`

### `grove session list`

List all active multiplexer sessions.

```
grove session list
```

### `grove session attach`

Attach to a named session.

```
grove session attach <name>
```

| Argument | Description |
|----------|-------------|
| `name` | Exact session name as shown in `grove session list` |

---

## `grove config`

### `grove config show`

Print the path to the config file and its current contents.

```
grove config show
```

### `grove config edit`

Open the config file in `$EDITOR` (defaults to `vim`). Validates the file after saving and prints any warnings.

```
grove config edit
```

---

## `grove init`

Print the `gv` shell function to stdout. Intended to be evaluated by your shell.

```
grove init <shell>
```

| Argument | Values |
|----------|--------|
| `shell` | `zsh` \| `bash` \| `fish` |

**Setup:**
```sh
# Zsh / Bash
eval "$(grove init zsh)"

# Fish
grove init fish | source
```

---

## `grove completions`

Generate shell completions for the `grove` binary.

```
grove completions <shell>
```

| Argument | Values |
|----------|--------|
| `shell` | `bash` \| `elvish` \| `fish` \| `powershell` \| `zsh` |

# Managing Repositories

grove keeps a database of tracked repositories. The `grove repo` subcommands let you add, remove, and list them.

## Create a new repo

```sh
grove repo new <name> [dir]
```

Creates a new, empty repository in the `.bare` container layout inside one of your configured directories. The repo is seeded with a single empty commit on `master` and immediately tracked in grove's database.

```sh
grove repo new my-project work
```

## Migrate a legacy bare repo

```sh
grove repo migrate [name]
```

Converts a legacy bare repository to the `.bare` container layout in place. Refuses if any worktree has uncommitted changes. Existing worktrees are discarded (branches are preserved — recreate worktrees with `grove open`).

## List tracked repos

```sh
grove repo list
```

Prints a table of all tracked repositories:

```
| Name     | Dir  | Score | Path                      |
|----------|------|-------|---------------------------|
| myrepo   | work |   42  | /Users/you/work/myrepo    |
| otherlib | oss  |    7  | /Users/you/oss/otherlib   |
```

| Column | Description |
|--------|-------------|
| Name | Repository name |
| Dir | The named directory from config this repo belongs to (or `-` if added manually) |
| Score | Current [frecency](../reference/frecency.md) score — higher means used more recently and frequently |
| Path | Absolute path to the container directory on disk |

Repos are sorted by frecency score descending, so your most active projects appear first.

## Track an existing repo

```sh
grove repo add <path>
```

Registers an existing git repository on disk with grove. The repo name is taken from the directory name. Works with both the `.bare` container layout and plain repositories.

```sh
grove repo add /Users/you/work/myrepo
```

grove verifies that the path is a valid git repository before adding it. This is useful for repos that were not cloned via `grove clone`.

## Stop tracking a repo

```sh
grove repo rm <name>
```

Removes a repository from grove's tracking database. This does **not** delete anything from disk — the repository and all its worktrees are left untouched.

```sh
grove repo rm myrepo
```

# Managing Repositories

grove keeps a database of tracked repositories. The `grove repo` subcommands let you add, remove, and list them.

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
| Path | Absolute path to the bare repository on disk |

Repos are sorted by frecency score descending, so your most active projects appear first.

## Track an existing repo

```sh
grove repo add <path>
```

Registers an existing bare (or normal) git repository on disk with grove. The repo name is taken from the directory name.

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

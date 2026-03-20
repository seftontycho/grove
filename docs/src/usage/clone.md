# Cloning Repositories

`grove clone` clones a remote repository as a **bare** git repository into one of your configured directories, and automatically registers it in grove's tracking database.

## Usage

```sh
grove clone <url> [dir]
```

- `url` — any Git remote URL (SSH or HTTPS)
- `dir` — the name of a configured directory (from `[directories]` in your config). If omitted and you have more than one directory configured, grove will present an interactive selection prompt. If you only have one directory configured it is used automatically.

## Examples

```sh
# Clone into the 'work' directory (from config)
grove clone git@github.com:your-org/your-repo.git work

# Clone using HTTPS
grove clone https://github.com/your-org/your-repo.git work

# Omit the directory for interactive selection
grove clone git@github.com:your-org/your-repo.git
```

## What happens

1. The repository name is inferred from the URL (e.g. `your-repo` from `your-org/your-repo.git`).
2. The repo is cloned bare into `<directory-path>/<repo-name>` (e.g. `/Users/you/work/your-repo`).
3. The repo is added to grove's database so it appears in `grove open` and `grove repo list`.

## Prerequisites

You must have at least one directory configured before cloning. If none are configured, grove will print the path to your config file and exit with an error.

```toml
# ~/.config/grove/config.toml
[directories]
work = { path = "/Users/you/work" }
oss  = { path = "/Users/you/oss"  }
```

## Tracking an existing repo

If you already have a bare repository on disk that you want grove to track, use [`grove repo add`](./repos.md) instead.

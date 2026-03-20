# Configuration

grove is configured via a TOML file. Run `grove config show` to see its location, or `grove config edit` to open it in your `$EDITOR`.

## Example config

```toml
shell = "zsh"
multiplexer = "auto"

[directories]
work = { path = "/Users/you/work" }
oss  = { path = "/Users/you/oss"  }
```

## Fields

### `shell`

The shell to use inside new sessions.

- **Type:** string — `zsh` | `bash` | `fish`
- **Default:** auto-detected from the `$SHELL` environment variable (falls back to `zsh`)

```toml
shell = "zsh"
```

### `multiplexer`

Which terminal multiplexer backend grove should use.

- **Type:** string — `auto` | `zellij` | `tmux`
- **Default:** `auto`

| Value | Behaviour |
|-------|-----------|
| `auto` | Prefer the currently-running multiplexer (`$ZELLIJ` / `$TMUX` env vars), then fall back to whichever binary is found on `$PATH` first (zellij wins on a tie) |
| `zellij` | Always use zellij |
| `tmux` | Always use tmux |

```toml
multiplexer = "tmux"
```

### `[directories]`

A map of named directory entries. Each key is a short label; each value has a `path` field pointing to a directory on disk where grove will clone and look for bare repositories.

```toml
[directories]
work = { path = "/Users/you/work" }
oss  = { path = "/Users/you/oss"  }
```

You can have as many entries as you like. Paths that do not exist or are not directories will produce a warning on startup but will not prevent grove from running.

## Custom layout templates

grove also looks for layout template overrides in the config directory:

| File | Purpose |
|------|---------|
| `~/.config/grove/templates/layout.kdl` | Custom zellij layout |
| `~/.config/grove/templates/tmux.sh` | Custom tmux layout script |

See [Session Layout](./reference/layout.md) for full details.

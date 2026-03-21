# Configuration Reference

## File location

| Platform | Path |
|----------|------|
| Linux | `~/.config/grove/config.toml` |
| macOS | `~/Library/Application Support/grove/config.toml` |

Run `grove config show` to print the exact path on your system.

## Full example

```toml
shell = "zsh"
multiplexer = "auto"

[directories]
work = { path = "/Users/you/work" }
oss  = { path = "/Users/you/oss"  }
```

## Fields

### `shell`

Shell binary to launch inside new sessions.

| | |
|-|-|
| **Type** | string |
| **Values** | `zsh` \| `bash` \| `fish` |
| **Default** | Auto-detected from `$SHELL`; falls back to `zsh` |

```toml
shell = "fish"
```

---

### `multiplexer`

Terminal multiplexer backend to use.

| | |
|-|-|
| **Type** | string |
| **Values** | `auto` \| `zellij` \| `tmux` |
| **Default** | `auto` |

| Value | Behaviour |
|-------|-----------|
| `auto` | Use `$ZELLIJ` or `$TMUX` env vars to detect the running multiplexer; if neither is set, check `$PATH` for `zellij` then `tmux` |
| `zellij` | Always use zellij |
| `tmux` | Always use tmux |

```toml
multiplexer = "tmux"
```

---

### `[directories]`

Named directory entries used for cloning and locating repositories.

| | |
|-|-|
| **Type** | TOML table |
| **Default** | Empty |

Each key is a short label (used in `grove clone` and displayed in `grove repo list`). Each value must have a `path` field.

```toml
[directories]
work    = { path = "/Users/you/work" }
oss     = { path = "/Users/you/oss" }
clients = { path = "/Users/you/clients" }
```

Paths that do not exist or are not directories produce a warning on startup but do not prevent grove from running.

## Layout templates

Layout template files are not part of `config.toml` but live alongside it:

| File | Purpose |
|------|---------|
| `<config_dir>/grove/templates/zellij.kdl` | Custom zellij layout |
| `<config_dir>/grove/templates/tmux.sh` | Custom tmux setup script |

If these files exist grove uses them instead of the built-in defaults. See [Session Layout](./layout.md) for the full template variable reference.

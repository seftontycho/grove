# Session Layout

grove opens a pre-configured session in your multiplexer whenever you create a new workspace. The layout is driven by a template that you can fully customise.

## Default layouts

### Zellij

Three tabs, CWD set to the worktree path:

| Tab | Command | Focused |
|-----|---------|---------|
| `shell` | your shell | yes |
| `editor` | `nvim .` | no |
| `opencode` | `opencode` | no |

### tmux

Three windows, all opened with `cd <worktree_path>` first:

| Window | Command | Focused |
|--------|---------|---------|
| `shell` | your shell | yes |
| `editor` | `nvim .` | no |
| `opencode` | `opencode` | no |

## Customising the layout

Place a custom template file in your grove config directory and grove will use it instead of the built-in default:

| Multiplexer | Template path |
|-------------|---------------|
| zellij | `~/.config/grove/templates/layout.kdl` |
| tmux | `~/.config/grove/templates/tmux.sh` |

Templates are rendered using [minijinja](https://docs.rs/minijinja/latest/minijinja/). The syntax is a subset of [Jinja2](https://jinja.palletsprojects.com/en/stable/templates/) — refer to the [minijinja template documentation](https://docs.rs/minijinja/latest/minijinja/syntax/index.html) for the full syntax reference.

## Template variables

The following variables are available in every template:

| Variable | Type | Description |
|----------|------|-------------|
| `worktree_path` | string | Absolute path to the worktree directory, e.g. `/home/you/work/myrepo/worktrees/main` |
| `shell` | string | The user's shell binary name, e.g. `zsh`, `bash`, or `fish` |
| `session_name` | string | The full session identifier. For zellij: `repo/branch`. For tmux: `repo-branch` |
| `repo` | string | The repository name, e.g. `myrepo` |
| `branch` | string | The branch name, e.g. `main` or `feat/auth` |

## Example: custom zellij layout

```kdl
layout {
    cwd "{{ worktree_path }}"
    tab name="shell" focus=true {
        pane command="{{ shell }}" {}
    }
    tab name="editor" {
        pane split_direction="horizontal" {
            pane command="nvim" {
                args "."
                size "70%"
            }
            pane command="{{ shell }}" {}
        }
    }
}
```

## Example: custom tmux layout

```sh
SESSION="{{ session_name }}"

tmux rename-window -t "$SESSION:0" "shell"
tmux send-keys -t "$SESSION:0" "cd '{{ worktree_path }}' && {{ shell }}" Enter

tmux new-window -t "$SESSION" -n "editor" -c "{{ worktree_path }}"
tmux send-keys -t "$SESSION:editor" "nvim ." Enter

tmux select-window -t "$SESSION:shell"
```

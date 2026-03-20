# Shell Integration

grove provides a `gv` shell function that wraps `grove open` for fast day-to-day use. Setting it up takes one line.

## Setup

Add the appropriate line to your shell config file:

**Zsh** — add to the end of `~/.zshrc`:
```sh
eval "$(grove init zsh)"
```

**Bash** — add to the end of `~/.bashrc`:
```sh
eval "$(grove init bash)"
```

**Fish** — add to the end of `~/.config/fish/config.fish`:
```sh
grove init fish | source
```

Reload your shell or open a new terminal for the changes to take effect.

## The `gv` function

`gv` is your primary interface to grove:

| Command | Action |
|---------|--------|
| `gv` | Open a repo+branch interactively (fuzzy select both) |
| `gv <query>` | Fuzzy-select a repo matching `query`, then select a branch |
| `gv <query> <branch>` | Go directly to the specified repo and branch |
| `gv -l` | List all active grove sessions |
| `gv -c` | Close the current worktree and session |

### Examples

```sh
gv                    # pick repo, then pick branch
gv grove              # pick a branch in the grove repo
gv grove main         # open grove/main directly
gv -l                 # see all active sessions
gv -c                 # close current session
```

## Shell completions

grove can generate completions for the `grove` binary itself (not the `gv` function):

```sh
# Zsh — add to your fpath
grove completions zsh > ~/.zfunc/_grove
# ensure ~/.zfunc is in your fpath and compinit has been called

# Bash
grove completions bash > /etc/bash_completion.d/grove
# or for a user install:
grove completions bash > ~/.local/share/bash-completion/completions/grove

# Fish
grove completions fish > ~/.config/fish/completions/grove.fish
```

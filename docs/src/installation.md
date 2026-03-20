# Installation

## Prerequisites

grove requires the following tools to be installed and available on your `$PATH`:

| Tool | Version | Purpose |
|------|---------|---------|
| [Rust](https://rustup.rs) | stable | Building grove |
| [git](https://git-scm.com) | any recent | Worktree management |
| [zellij](https://zellij.dev) | any recent | Terminal sessions |

## Install grove

### From GitHub (recommended)

```sh
cargo install --git https://github.com/your-username/grove
```

This compiles and installs the latest version from the `main` branch directly.

To pin to a specific release tag:

```sh
cargo install --git https://github.com/your-username/grove --tag v0.1.0
```

### From source

If you want to build from a local clone (useful for development):

```sh
git clone https://github.com/your-username/grove
cd grove
cargo install --path .
```

## Verify the installation

```sh
grove --version
```

## Next steps

Once grove is installed, set up your shell integration:

```sh
# Zsh
eval "$(grove init zsh)"

# Bash
eval "$(grove init bash)"

# Fish
grove init fish | source
```

Then continue to [Shell Integration](./shell-integration.md) and [Configuration](./configuration.md).

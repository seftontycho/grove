# Managing Sessions

The `grove session` subcommands give you direct access to the multiplexer sessions grove has created, without going through the repo/branch selection flow.

## List sessions

```sh
grove session list
```

Lists all active grove sessions as reported by the current multiplexer backend. Each session is shown by its full name.

Example output:

```
  myrepo/main
  myrepo/feat/auth
  otherlib/main
```

This is also accessible via the `gv -l` shell alias.

## Attach to a session

```sh
grove session attach <name>
```

Attaches directly to a named session. The name must match exactly as shown in `grove session list`.

```sh
grove session attach myrepo/main      # zellij session
grove session attach myrepo-main      # tmux session
```

For tmux, if you are already inside a tmux session, grove uses `switch-client` instead of `attach-session` to avoid nesting.

## Tip: use `grove open` for day-to-day switching

`grove session attach` is useful when you already know the exact session name. For interactive switching with fuzzy search, use [`grove open`](./open.md) (or `gv`) instead — it handles both session reuse and worktree creation in one step.

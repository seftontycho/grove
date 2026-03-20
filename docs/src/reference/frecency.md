# Frecency

grove ranks repositories using a **frecency** score — a combination of *frequency* (how often you use a repo) and *recency* (how recently you used it). This is the same approach used by [zoxide](https://github.com/ajeetdsouza/zoxide) for directory ranking.

## How it works

Every time you run `grove open` for a repo, its score is incremented by 1.0 and its `last_accessed_at` timestamp is updated.

Repos are sorted by score descending in `grove repo list` and in the interactive selection prompt in `grove open`, so your most active repos always appear at the top.

## Decay

To keep scores bounded over time, grove applies a global multiplicative decay whenever the sum of all scores exceeds **1000**:

1. All scores are multiplied by **0.9**.
2. Any score that falls below **0.01** is set to zero.

This means scores naturally converge: a repo you stopped using will gradually fall behind repos you use regularly, without any scores growing unboundedly.

## Constants

| Constant | Value | Description |
|----------|-------|-------------|
| `FRECENCY_MAX_TOTAL` | 1000.0 | Total score threshold that triggers a decay pass |
| `FRECENCY_DECAY` | 0.9 | Multiplicative factor applied to all scores on decay |
| `FRECENCY_FLOOR` | 0.01 | Scores below this are zeroed out after decay |

## Example

| Event | Score before | Score after |
|-------|-------------|-------------|
| First `grove open myrepo` | 0.0 | 1.0 |
| Second `grove open myrepo` | 1.0 | 2.0 |
| ... 998 more opens across all repos, total reaches 1001 | — | decay: all scores × 0.9 |
| Score of a repo with 0.005 after decay | 0.005 | 0.0 (floored) |

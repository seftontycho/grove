# Grove default tmux layout template.
# Rendered by minijinja — available variables:
#   {{ worktree_path }}  — absolute path to the worktree
#   {{ shell }}          — user's shell, e.g. zsh
#   {{ session_name }}   — tmux session name (repo-branch)
#   {{ repo }}           — repository name
#   {{ branch }}         — branch name
#
# This script is executed after the tmux session has been created.
# The session already has one window open (window 0).

SESSION="{{ session_name }}"

# Window 1: shell (rename the initial window)
tmux rename-window -t "$SESSION:0" "shell"
tmux send-keys -t "$SESSION:0" "cd '{{ worktree_path }}' && {{ shell }}" Enter

# Window 2: editor
tmux new-window -t "$SESSION" -n "editor" -c "{{ worktree_path }}"
tmux send-keys -t "$SESSION:editor" "nvim ." Enter

# Window 3: opencode
tmux new-window -t "$SESSION" -n "opencode" -c "{{ worktree_path }}"
tmux send-keys -t "$SESSION:opencode" "opencode" Enter

# Focus the shell window
tmux select-window -t "$SESSION:shell"

gitflex is a git wrapper TUI that cleans up old branches and makes it easier to switch, rebase, and merge in a repo with many branches.

Run `gitflex clean` to clear old branches, automatically selecting branches already merged into main/master or authored by others for deletion. It deselects branches that can't be deleted, like those currently checked out, including in other worktrees.

Run `gitflex switch/merge/rebase/delete` to perform the corresponding git operation, but remember the last branch selected for future use. It also provides a handy search feature in case you forgot the branch name

Install using `cargo install gitflex`

Afterwards I recommend setting up aliases for easy access, for example:

```
alias gc='gitflex clean'
alias gs='gitflex switch'
alias gr='gitflex rebase'
alias gm='gitflex merge'
alias gd='gitflex delete'
```

`gitflex` also support directly specifying the branch name on the CLI, so you can reuse your shortcut alias like `gs main` (same as `gitflex switch main` which immediately switches to main)

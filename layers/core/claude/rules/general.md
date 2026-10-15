# General working preferences

Loaded on every project, on every machine. Symlinked here from the dotfiles repo — edit it there, or edit it in place; they are the same file.

Machine-specific instructions belong in `~/.claude/CLAUDE.md`, which is not tracked. Harper/work-specific instructions belong in a rule file that is not published to a public repo.

## Output

Be concise in conversation output. Avoid fillers. Professional and succinct.

## Working directory

Operate within the relative working directory. Use `$PWD/.claude/` for worktrees, temporary files, and other scratch state.

Temporary files go in `$PWD/.claude/tmp/`.

## Session start

Before doing any work:

1. Read `AGENTS.md`, `CONTRIBUTING.md`, and `README.md` from the project root if they exist — they may also be under `.github/`. Use them for project conventions, contribution guidelines, and context.
2. Check `git status` for uncommitted changes.
3. If the working tree is clean, check out `main` and `git pull`.
4. If there are uncommitted changes, **stop and ask** whether to stash, commit, or keep working on the current branch. Do not check out automatically.

## Git workflow — worktrees by default

Default to git worktrees for all development work. Do not create plain branches unless asked.

```bash
git worktree add $PWD/.claude/worktrees/feature-name -b feature-name
git worktree remove $PWD/.claude/worktrees/feature-name   # once merged
```

Work stays isolated in its own directory, so several sessions can run at once.

Fall back to a plain branch only when specifically asked.

## Commit trailers

Default: `Co-Authored-By: Claude <model>`.

**Exception — `nodejs/node`** (any clone): use the project's DCO convention, not `Co-Authored-By`:

```
Assisted-by: Claude <model>
Signed-off-by: Ethan Arrowood <ethan@arrowood.dev>
```

`<model>` is the human-readable name, e.g. "Claude Opus 5". See the `nodejs-contribute` skill for the full build/test/commit/land workflow.

## Commenting on my behalf

When posting under my identity — GitHub PRs and issues, Slack, code review:

- Write in the **first person as me**. Never refer to me in the third person, or as though a separate author wrote it (not "after digging into this with @Ethan-Arrowood").
- Add a footer on its own line at the very bottom: `<sub>sent with Claude <model></sub>`.
- No footer in release or publish text, such as GitHub Release notes.

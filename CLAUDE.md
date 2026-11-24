# CLAUDE.md

@AGENTS.md

## Claude Code specifics

- **Start of a session.** Read `docs/STATUS.md`, then check the board:
  `gh project item-list 3 --owner Chandrashekhar-Hegde --format json`. Continue the In Progress or
  Todo item, or the next milestone named in "Where we are and what's next" above.
- **Skills.** The project ships a Rust skill (`.claude/skills/rust`, mirrored in `.agents/skills`).
  Railway work goes through the `use-railway` skill.
- **Long commands.** Run `scripts/check.sh` in the foreground with a long timeout; a full run takes
  about 3–5 minutes. Run deploy polling in the background and wait for the notification instead of
  sleeping in loops.
- **Scratch files** (screenshots, reference venvs, probe scripts) go in the session scratchpad,
  never in the repo. The reference venv can also be `.refenv/`, which is gitignored.
- **Approval.** Ask before anything outward-facing that isn't routine for this repo (new services,
  paid resources, deleting resources, changing secrets).
- **Before reporting work as done,** check it at every layer you touched (tests, gate, and a real
  run or screenshot for UI), and say plainly what wasn't verified.

# CLAUDE.md

@AGENTS.md

## Claude Code specifics

- **Start of a session.** Read `docs/STATUS.md` and `docs/ROADMAP.md`. Continue an item that is
  already claimed and in progress; otherwise take the first `agent-ready` ROADMAP row whose
  blockers are closed. With the owner's access, also keep the private board in step:
  `gh project item-list 3 --owner Chandrashekhar-Hegde --format json`.
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

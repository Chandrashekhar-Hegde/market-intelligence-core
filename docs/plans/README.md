# Plans

Design plans for the project, kept in the repo so humans and AI agents work from the same
thinking. **The rule:** a plan is committed before significant work starts, and is marked
**Done** or **Superseded** when it ships. GitHub issues hold the executable work packages; plans
explain the *why* and the *order*.

| Plan | Date | Status | Issues / PRs |
|---|---|---|---|
| [Phase 0 roadmap: from fixture console to live quant analysis](2026-11-22-phase-0-roadmap.md) | 2026-11-22 | Shipped (0.1–0.5) | PRs #72–#78 |
| [Make it live: real data in the UI, local gate, board](2026-11-24-live-data-and-gate.md) | 2026-11-24 | Done | PRs #72, #76 |
| [Collaboration: plans, roadmap and work packages on GitHub](2026-11-25-collaboration.md) | 2026-11-25 | Done | #95, #97 |
| [Phase 0.6: live data and validated advice](phase-0.6-advice.md) | 2026-11-26 | Done | #102–#106 |
| [Phase 0.7: find any listing, understand the advice, real forecasts, news](phase-0.7.md) | 2026-11-27 | In progress | #110–#114, #108 |
| [Phase 1: real events and fundamentals](phase-1-events-and-fundamentals.md) | 2026-11-25 | Planned | #64, #67, #80–#88, #13, #17, #65, #66 |
| [Phase 2: portfolio, accounts, broker data](phase-2-portfolio.md) | 2026-11-25 | Planned | #91–#93, #89, #90, #18, #68, #69, #70 |

Decision documents (`decision-*.md`) are added here by the research issues (#83, #85, #89, #65).

## Writing a new plan

1. Copy the shape of an existing plan: goal, a picture of the dependencies, steps in order (each
   one an issue), cross-cutting rules, and "done when".
2. Open one issue per step from the **Work package** template, then link the issues in the plan.
3. Add a row to this table and to [ROADMAP.md](../ROADMAP.md) in the same PR.

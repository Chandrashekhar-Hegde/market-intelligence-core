# Contributing

Market Intelligence Core is source-available under a noncommercial license (PolyForm
Noncommercial 1.0.0). Contributions are welcome from people and from AI agents. Both follow the
same rules below.

**Start here:**
1. [AGENTS.md](AGENTS.md): setup, commands, architecture, and gotchas we already paid for.
2. [docs/ROADMAP.md](docs/ROADMAP.md): what's done and the ordered queue of what's next.
3. [docs/STATUS.md](docs/STATUS.md): what works today, in plain terms.

Questions, ideas and design debates go to
[Discussions](https://github.com/Chandrashekhar-Hegde/market-intelligence-core/discussions).
Issues are for concrete, ready work.

## Local setup

Needs Rust 1.96+, Node 24 (`.nvmrc`) and `cargo install cargo-audit --locked`.

```bash
git config core.hooksPath .githooks          # pre-push gate: nothing is pushed unless it passes
(cd apps/web && npm ci && npx playwright install chromium)
scripts/check.sh                             # must end with "All checks passed."
```

## How to pick up work

1. **Choose:** take the first `agent-ready` item in
   [ROADMAP.md → Next](docs/ROADMAP.md#next-in-order) whose blockers are closed. ROADMAP order
   wins: `gh issue list --label agent-ready` shows the same set, but sorted by date. Small,
   isolated items are also labelled `good first issue`.
2. **Claim it:** comment "taking this" on the issue. If you have write access, also assign
   yourself; otherwise a maintainer assigns you. If an item has been claimed but shows no activity
   for 7 days, ask in the issue before taking it over.
3. **Branch** from `main` as `<milestone>-<short-slug>`, for example `0.1-bhavcopy` or
   `phase1-event-study`. One issue per PR.
4. **Follow the work package.** Every issue has *Why*, *Scope*, a *Suggested approach* (files and
   code to reuse), *Acceptance criteria*, *Verification* and *Dependencies*. If the approach turns
   out wrong, say so in the issue before diverging.
5. **Open a PR** using the template, with `Closes #N`. CI must be green. `main` is protected: no
   direct pushes, no force-pushes.
6. **Items labelled `needs-owner-input`** wait for the owner (keys, money, product decisions).
   Research that helps the decision is welcome; implementation waits.

## Definition of Done

An issue is done when **all** of these are true:

- [ ] Every acceptance criterion in the issue is checked.
- [ ] `scripts/check.sh` passes locally (the pre-push hook runs it) and CI is green.
- [ ] There are tests at every layer you touched:
  - domain maths against an independent reference (`scripts/gen_reference.py`), plus a
    behavioural test where the truth is known;
  - providers on recorded fixtures;
  - API on in-process stubs;
  - UI with Vitest, plus Playwright with axe.
- [ ] Docs are updated in the same PR:
  - [docs/ROADMAP.md](docs/ROADMAP.md): move the item to *Done*;
  - [docs/STATUS.md](docs/STATUS.md);
  - [docs/METHODOLOGY.md](docs/METHODOLOGY.md) for any new statistic;
  - [docs/API.md](docs/API.md) and `/openapi.json` for any route;
  - the in-app **How it works** page, if what users see or what is stored changes.
- [ ] The PR says `Closes #N`.
- [ ] After merge, Railway deploys `main` automatically. The production smoke test passes:
  `BASE_URL=… GM_ACCESS_TOKEN=… scripts/smoke_api.sh`. **Maintainer step:** only maintainers hold
  production access, so outside contributors list any production-only checks (smoke, Railway
  spikes) in the PR for a maintainer to run.

## Rules that don't bend

- **Distributions, not calls.** No buy/sell outputs. Forecasts are ranges with a horizon.
- **A model earns its place.** It must beat the baseline out of sample in the validation harness
  before the UI presents it as useful.
- **Cite or calibrate.** Every constant has a source in METHODOLOGY or is estimated from data.
- **Pure domain.** `gm-domain` has no network, clock or unseeded randomness.
- **No live order placement.** `ExecutionMode::Live` stays disabled.
- **No secrets in the repo, issues or logs:** credentials, tokens, account ids, broker or payment
  secrets, personal emails. EDGAR-style "contact" headers use a project contact, never a person.
- **Data-source terms are checked before code.** Record them in `docs/INTEGRATIONS.md`. Sources
  whose terms forbid automated access (for example the NSE website JSON) are not used.

## Working with AI agents

- Agents read [AGENTS.md](AGENTS.md) (Claude Code reads it through `CLAUDE.md`) and follow this
  file exactly like a human contributor: claim, branch, work package, Definition of Done, PR.
- Plans for significant work are committed to [docs/plans/](docs/plans/README.md) *before*
  implementation, so the reasoning lives in the repo, not in a chat log.
- A human reviews and merges. CI and branch protection apply to everyone.
- Commit messages and PRs carry **no AI co-author trailers or "generated with" lines**.
- Agents must say plainly what they did *not* verify.

## Proposing new work

- **Something concrete:** open an issue from the **Work package** template. Include why, scope,
  approach, acceptance criteria and verification. Maintainers set the milestone and state label.
- **Something bigger:** start a Discussion or write a plan in `docs/plans/` first. It then becomes
  one issue per step.
- **Bugs:** use the **Bug report** template, with reproduction steps.

## Frontend specifics

The interface is a dense, evidence-first analysis tool, not a marketing page. Verify loading,
empty, error, stale and success states; desktop and phone widths (no horizontal overflow);
keyboard reachability; and axe with no serious violations.

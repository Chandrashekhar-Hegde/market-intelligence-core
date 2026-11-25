> **Status: Done** (#95; cold-start fixes in #97). The plan approved on 2026-11-25 for making the project workable by
> other developers and AI agents from GitHub alone.

# Put every plan in GitHub so any developer or AI agent knows exactly what's done and what's next

## Context

The owner wants the project to be workable by other developers and, later, other AI agents, from
GitHub alone. Today the knowledge is split:
- **Local only:** the design plans live in `~/.claude/plans/` (the Phase 0 plan and the live-data
  plan) and in chat.
- **Private:** the project board, which the owner wants kept private.
- **Too thin to start from cold:** the remaining 13 open issues. Four of them (#9, #13, #17, #18)
  even have broken bodies with literal `\n` sequences.
- **Unprotected `main`:** nothing stops someone pushing straight to it.
- **Out of date:** the PR template still says `make check`.

Already in place: `AGENTS.md`/`CLAUDE.md`, `docs/STATUS.md`, `docs/HISTORY.md`,
`docs/METHODOLOGY.md`, the milestones, and the pre-push gate.

Owner decisions (this session):
- Keep the board **private**, so the repo has to carry the complete public roadmap.
- **Protect `main` for everyone:** PR plus green CI, no force-push.
- **CODEOWNERS:** `@llMr-Sweetll` was the owner's old username, so switch to `@Chandrashekhar-Hegde`.
- **Enable Discussions** for ideas and design questions; issues stay concrete and ready to work.

Outcome: someone opening the repo can read, in order, what was built (with PR links), what is
next, and why. They can then pick the top "agent-ready" issue and finish it without asking anyone.

## 1. Plans committed to the repo (`docs/plans/`)

- `docs/plans/README.md`: index table (plan, date, status, issues, PRs) and the rule "a plan is
  committed before work starts; mark it Done/Superseded when it ships".
- `2026-11-22-phase-0-roadmap.md`: the original plan (copied from `~/.claude/plans/`), with a status
  header listing what shipped (0.1–0.5 via #72–#79), what changed (India-first became global, no
  Postgres bar cache, GET single-symbol analyze) and what is left.
- `2026-11-24-live-data-and-gate.md`: the live-data, Markets UI, gate and board plan, marked Done.
- **New:** `phase-1-events-and-fundamentals.md` and `phase-2-portfolio.md`. These are step-by-step
  design plans for the remaining work. Each step is one issue, with its order, dependencies,
  approach, files and verification, built on what exists (`gm-domain::analysis`, `multi`,
  `event_study.rs`, `gm-persistence`, `migrations/`).
- **Privacy pass** before committing: no tokens, emails or Railway IDs. Plans reference
  `AGENTS.md` for operational detail.

## 2. `docs/ROADMAP.md`, the public step-by-step source of truth

The board is private, so this file carries the roadmap:
- **Done:** milestone by milestone (0.1 → 0.4, plus the guide), one line per shipped item with its
  PR and issue links.
- **Next (ordered queue):** each row has an issue link, a one-line goal, `blocked by`, and a
  `needs owner input` marker.
- **A mermaid dependency graph** of the remaining work.
- **"How to pick the next item"** rule: the first `agent-ready` row whose blockers are closed.
- **Maintenance rule:** update it in the same PR that closes an issue. This is added to the
  Definition of Done.

The planned order (issue numbers marked *new* are created in step 3):
1. **#45, NSE bhavcopy cross-check and fallback.** A spike first: is
   `nsearchives.nseindia.com` reachable from Railway? The UDiFF zip format needs a `zip` crate.
2. **#71, replace chrono.** First a decision: jiff vs a maintained chrono, depending on what sqlx
   supports.
3. **#64, market-model event study** (estimation window, alpha + beta, CAR significance) in
   `gm-domain/src/event_study.rs`.
4. ***new*, persist data.** Attach Railway Postgres, needed first by events. Needs owner approval
   because it costs money.
5. ***new*, GDELT news ingestion** (open data) into the event store. Split out of #17.
6. ***new*, US filings (SEC EDGAR)** plus a **decision on an India announcements source** that the
   source's terms allow (the NSE site API is ruled out). Split out of #17.
7. **#13, event study views** (event markers, CAR chart).
8. **#65, calibrate or retire `/decide`.** A decision, made with #64's data.
9. **#66, fundamentals.** Split into: a decision on the free source, the provider, the metrics, and
   a Monte Carlo DCF.
10. **#67, HMM regimes** (optional; ships only if it beats the baselines in validation).
11. **#69, accounts.** Split into: an auth design decision, then sessions and migrating off the
    shared token.
12. **#70, portfolio analytics.** Split into: manual CSV holdings first, then risk (VaR/ES, Euler
    contributions, factors) reusing `analysis` and `multi`.
13. **#18 / #68, broker read-only adapters and live ticks.** Needs the owner's broker keys.
14. **#9 rewritten as "Operations status panel"** (provider health, cache age, stream status).
    Most of the original Command Center was replaced by Markets.

## 3. Issues: every open item becomes a work package an agent can finish

- **New issue template** `.github/ISSUE_TEMPLATE/work_package.yml` with these fields:
  - why;
  - scope, in and out;
  - suggested approach, with the files to touch and the existing code to reuse;
  - acceptance criteria (testable checkboxes);
  - verification commands;
  - dependencies;
  - needs owner input;
  - the Definition of Done, linked rather than copied.
- **Rewrite all 13 open issue bodies** in that format, fixing the broken `\n` bodies. Large items
  get **GitHub sub-issues** for each step (REST `sub_issues` endpoint), each also a full work
  package with a "Part of #X" link.
- **Labels:**
  - `agent-ready`: fully specified, no owner input needed;
  - `needs-owner-input`: keys, money or a product decision;
  - `blocked`: waiting on another issue.

  Apply them to every open issue. Keep `good first issue` for small, isolated ones.
- **Milestones:** close the finished 0.2, 0.3 and 0.4 milestones. Move #13 into Phase 1. Rewrite
  #9 as the ops panel in 0.5. Update every milestone description to its current status.
- **Board:** add the new issues; set Todo for queue items 1–2 and Backlog for the rest. The board
  remains the owner's private view.

## 4. Collaboration plumbing

- **`CONTRIBUTING.md`** gets three new sections:
  - *How to pick up work*: read AGENTS.md, take the top `agent-ready` ROADMAP item, self-assign,
    comment "taking this", branch as `<milestone>-<slug>`, one issue per PR.
  - *Definition of Done*: gate green; tests at every layer touched; STATUS, METHODOLOGY, API and
    ROADMAP updated; PR "Closes #N"; production smoke after deploy.
  - *Working with AI agents*: same rules; humans review; no AI co-author trailers.
- **PR template:** replace it with the gate, a docs checklist, `Closes #`, and a "verified in
  production" line.
- **`.github/CODEOWNERS`:** `* @Chandrashekhar-Hegde`. Point the security link in
  `ISSUE_TEMPLATE/config.yml` at the current repo, and add Discussions as a contact link.
- **Discussions:** enable them (`gh api -X PATCH repos/... -f has_discussions=true`), with a
  pinned welcome post that links ROADMAP, AGENTS and CONTRIBUTING.
- **README:** a "Roadmap & contributing" section linking ROADMAP, `docs/plans/`, CONTRIBUTING,
  AGENTS and Discussions.
- **Branch protection on `main`** (`gh api -X PUT .../branches/main/protection`):
  - require the `quality` check and an up-to-date branch;
  - require a PR, with 0 required approvals because the owner is the sole maintainer for now;
  - `enforce_admins: true`;
  - no force-push and no deletion.

  This lands **last**, after the docs PR merges.
- **`AGENTS.md`:** point "Where we are and what's next" at ROADMAP.md, so there is a single queue.

## Critical files

- `docs/plans/{README.md, 2026-11-22-phase-0-roadmap.md, 2026-11-24-live-data-and-gate.md, phase-1-events-and-fundamentals.md, phase-2-portfolio.md}` (new)
- `docs/ROADMAP.md` (rewrite), `CONTRIBUTING.md`, `README.md`, `AGENTS.md`
- `.github/ISSUE_TEMPLATE/work_package.yml` (new), `.github/ISSUE_TEMPLATE/config.yml`,
  `.github/pull_request_template.md`, `.github/CODEOWNERS`
- GitHub (via `gh`): 13 issue bodies, about 10 new sub-issues, 3 labels, milestones, Discussions,
  branch protection

## Verification

1. **Docs PR:** goes through the pre-push gate, then green CI, then merge.
2. **Public view as a newcomer:** fetch the README, ROADMAP and CONTRIBUTING via
   `gh api repos/.../contents` and follow the links. Every ROADMAP row resolves to an open issue
   with a full work-package body.
3. **Issues:**
   - `gh issue list --label agent-ready` shows the queue head (#45 or its spike);
   - no open issue body contains a literal `\n`;
   - every open issue has a milestone and exactly one of `agent-ready`, `needs-owner-input` or
     `blocked`.
4. **Sub-issues:** `gh api repos/.../issues/17/sub_issues` lists the split items.
5. **Protection:** `gh api .../branches/main/protection` shows the required `quality` check and
   `enforce_admins`. A test direct push of an empty commit is rejected (then dropped locally).
6. **Discussions:** enabled, and the welcome post is visible.
7. **Cold-start test:** give a fresh subagent only the repo URL, and ask what to work on next and
   how to verify it. It should answer with the queue head and its commands, using only repo
   content.

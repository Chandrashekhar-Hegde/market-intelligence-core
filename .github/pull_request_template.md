## What and why

Closes #

<!-- One issue per PR. Say what changed and why, in a few lines. -->

## Verification

- [ ] `scripts/check.sh` passes (the pre-push hook ran it)
- [ ] Tests added or updated at every layer touched (domain reference, provider fixtures, API stubs, Vitest, Playwright + axe)
- [ ] For UI changes: screenshot or description at desktop and phone width
- [ ] What was **not** verified is stated here:

## Docs (Definition of Done)

- [ ] `docs/ROADMAP.md`: item moved to Done
- [ ] `docs/STATUS.md` updated if what works changed
- [ ] `docs/METHODOLOGY.md` updated for any new statistic (with citation)
- [ ] `docs/API.md` and `/openapi.json` updated for any route change
- [ ] In-app "How it works" updated if what users see or what is stored changed

## Safety

- [ ] No credentials, tokens, account ids or personal emails committed
- [ ] Live order placement remains disabled
- [ ] Any new data source's terms of use are checked and recorded in `docs/INTEGRATIONS.md`

## After merge

- [ ] Railway deploy of `main` reached SUCCESS and `scripts/smoke_api.sh` passed against production

# Roadmap

The roadmap lives on GitHub so it stays current:

- [Project board](https://github.com/users/Chandrashekhar-Hegde/projects/3): Backlog → Todo → In Progress → In Review → Done
- [Milestones](https://github.com/Chandrashekhar-Hegde/market-intelligence-core/milestones): 0.1 Data foundation, 0.2 Analysis engine, 0.3 Validation harness, 0.4 Multi-instrument, 0.5 Analyze web app, Phase 1 Events & fundamentals, Phase 2 Portfolio & live data
- [Status](STATUS.md): what works today, what runs on fixtures, and known limits

Principles for every model added:

1. Distributions, not calls: forecasts are quantiles or fan charts with a horizon.
2. A model earns its place by beating a random-walk or historical-simulation baseline
   out of sample; otherwise the app says "no evidence of predictability".
3. Every constant is estimated from data or cites a published source.
4. Data provenance (source, fetch time, quality checks) is visible on every result.
5. The domain layer stays pure: providers fetch, `gm-domain` computes.

## Operating Rule

Do not add live network calls, clocks, random identifiers, or database writes to
the domain scoring path. External inputs must be collected upstream and passed in
as explicit facts.

/** Plain-language explanation of where the data comes from, what is stored, and how to read
 * every number. Formulas and citations live in docs/METHODOLOGY.md (linked at the end). */

const METHODOLOGY_URL =
  'https://github.com/Chandrashekhar-Hegde/market-intelligence-core/blob/main/docs/METHODOLOGY.md'

type Term = [string, string]

const GLOSSARY: Term[] = [
  ['Return', 'How much the price changed, as a percentage. We use log returns internally because they add up over time.'],
  ['Volatility', 'How much the price typically moves in a year, as a percentage. 20% means the yearly return stays within about ±20% of its average in roughly two years out of three.'],
  ['Drawdown', 'How far the price fell from its previous high. The worst drawdown is the deepest such fall in the period.'],
  ['VaR (Value at Risk)', '“On 1 day in 100 the loss exceeded X%.” It is a threshold, not the worst case.'],
  ['Expected shortfall', 'The average loss on those bad days beyond the VaR threshold: how bad the bad days are.'],
  ['Sharpe ratio', 'Return per unit of risk, per year. Above 1 is strong, around 0.5 is typical for stocks. Shown with its uncertainty (±).'],
  ['Beta', 'How much the instrument moves when its home market moves 1%. Beta 1.2 means about 1.2% on average.'],
  ['Alpha', 'Return not explained by the market. Usually indistinguishable from zero once you look at its ± range.'],
  ['p-value', 'How surprising the data would be if nothing interesting were going on. Below 0.05 we call the effect “significant”.'],
  ['Fat tails', 'Extreme days happen far more often than a bell curve predicts. Almost every real market has them.'],
  ['Correlation', 'Whether two instruments move together day to day: 1 together, 0 unrelated, −1 opposite.'],
  ['Cointegration', 'Whether two prices stay tied over the long run, so the gap between them keeps closing. Different from correlation.'],
  ['Half-life', 'For a gap that closes over time: how many trading days it takes to close halfway.'],
]

export function Guide() {
  return (
    <>
      <header className="topbar">
        <div>
          <p className="eyebrow">How it works</p>
          <h2>What you are looking at, and how far to trust it</h2>
        </div>
      </header>

      <section className="panel chart-panel guide" aria-labelledby="guide-what">
        <h3 id="guide-what">What this app does</h3>
        <p>
          Pick any stock, index, ETF, fund, cryptocurrency or currency pair, from any exchange. The app fetches its price history
          and live price, checks the data, and analyses it: how it moves, how risky it is, how it relates to its home market, and
          what range of prices is plausible in the coming weeks. It also tests whether any forecasting model actually works on
          that instrument. Usually none beats a simple baseline, and the app says so.
        </p>
        <p>
          <strong>It gives a call only when the evidence supports one.</strong> Six well-known trading rules (trend,
          momentum, short-term bounce, RSI, unusual volume, buying pressure) are tested on that instrument’s own past, the
          fair way: each day’s signal uses only what was known that day. A rule may say Buy or Sell only if it beat
          trading costs, held up in both halves of the history, and passed a test that allows for having tried many
          rules. Otherwise the app says Hold and shows why. On most stocks, most of the time, the honest answer is Hold.
          It cannot place orders, and it is not personal financial advice.
        </p>
      </section>

      <section className="panel guide" aria-labelledby="guide-data">
        <h3 id="guide-data">Where the data comes from</h3>
        <ul className="plain-list">
          <li>
            <strong>Prices and history:</strong> Yahoo Finance (free, unofficial). Daily closing prices are adjusted for
            splits and dividends, so a dividend payout doesn’t look like a crash.
          </li>
          <li>
            <strong>Live prices:</strong> streamed from Yahoo. On 25 Nov 2026 we measured NSE stocks and NIFTY at about 2
            seconds behind the exchange. BSE is delayed 15 minutes and is labelled <em>DELAYED 15 MIN</em>. Crypto and
            currencies arrive within 1–3 seconds. Other exchanges follow Yahoo’s terms; many are delayed.
          </li>
          <li>
            <strong>The status pill</strong> on the quote says what you’re seeing: LIVE (a fresh tick), DELAYED (the feed or
            quote lags), or CLOSED (weekend, holiday or after hours, showing the last trade).
          </li>
          <li>
            <strong>The TradingView chart</strong> is TradingView’s own widget, streamed to your browser. We only display it;
            its numbers are not used in any analysis.
          </li>
          <li>
            <strong>Data checks</strong> run on every price series: impossible prices, missing trading days, stale data and
            suspicious one-day jumps are listed so you can judge the data before trusting the analysis.
          </li>
        </ul>
      </section>

      <section className="panel guide" aria-labelledby="guide-storage" data-testid="guide-storage">
        <h3 id="guide-storage">What is stored (the database question)</h3>
        <ul className="plain-list">
          <li>
            <strong>About you: nothing but a sign-in cookie.</strong> The private beta uses one shared access code. Entering it
            sets a cookie in your browser for 30 days. There are no accounts and no names or emails, and the app keeps no
            record of what you search. The hosting provider keeps standard short-lived request logs (which pages were
            requested), as every web host does.
          </li>
          <li>
            <strong>Market data is not kept in a database.</strong> It is fetched from Yahoo when you ask, kept in the server’s
            memory for 1 to 15 minutes so repeated views are fast, and gone when the server restarts. We do this on purpose:
            adjusted prices are rewritten every time a company pays a dividend, so a stored copy would silently go out of date.
          </li>
          <li>
            <strong>Analyses are recomputed every time</strong> from fresh data. Forecasts use a fixed random seed, so the same
            data always gives the same numbers.
          </li>
          <li>
            <strong>The hosted beta runs without a database</strong>, so nothing you look at is written anywhere.
          </li>
          <li>
            <strong>Your browser</strong> keeps recent answers in memory while the page is open and forgets them when you close
            it.
          </li>
        </ul>
      </section>

      <section className="panel chart-panel guide" aria-labelledby="guide-read">
        <h3 id="guide-read">How to read the analysis</h3>
        <dl className="guide-list">
          <dt>What to do</dt>
          <dd>
            Buy, Sell or Hold, from the rules that have worked on this instrument. With a call you also see how often the
            rule was right in the past (and a likely range for that), the time frame, the day to look again, and the price
            at which the call changes. Below it, each rule says what it sees today and whether it is proven here. If a live
            price would flip a rule, it says so as it happens.
          </dd>
          <dt>How much to put in</dt>
          <dd>
            Enter the most you are willing to lose on a bad day. The app divides it by the average fall on this
            instrument’s worst 1 day in 20, which gives the largest amount that keeps a typical bad day within your limit.
          </dd>
          <dt>Likely close today and tomorrow</dt>
          <dd>
            Next to the live price: where 9 in 10 simulated outcomes end by today’s close, and by the next one. It is
            recomputed with every price tick, and narrows as the session runs out.
          </dd>
          <dt>What this means for your money</dt>
          <dd>
            Cards that turn the statistics into rupees for the amount you enter: a typical day, a bad day, the worst fall,
            where it could be in a week, a month and three months, and whether it moves with the market.
          </dd>
          <dt>Show all the maths</dt>
          <dd>Every test, model score and data check behind the cards, for anyone who wants to check the working.</dd>
          <dt>Range of outcomes</dt>
          <dd>
            2,000 simulated futures for the price over the next 63 trading days (about 3 months). The dotted lines enclose half
            of them, the dashed lines nine tenths, and the solid line is the middle. Read it as “how wide could this go”, not “where will it go”: the width is reliable, the centre is
            not, because the average return is very uncertain (its ± is printed next to it).
          </dd>
          <dt>Value at risk and expected shortfall</dt>
          <dd>The size of a bad day, measured three ways: from history, from a fat-tailed curve, and from today’s volatility.</dd>
          <dt>How much it moves</dt>
          <dd>
            Volatility now and in the long run. Markets have calm and stormy periods; the GARCH model measures how long a storm
            usually lasts (the half-life).
          </dd>
          <dt>Shape and predictability</dt>
          <dd>
            Statistical tests of whether returns are bell-shaped (they rarely are) and whether yesterday predicts today (for
            most liquid instruments it doesn’t).
          </dd>
          <dt>Market exposure</dt>
          <dd>
            Beta against the instrument’s home index, chosen automatically: NIFTY 50 for NSE, Nikkei 225 for Tokyo, DAX for
            Germany, the S&amp;P 500 for the US, and a world index (MSCI ACWI) where we don’t know the exchange. Indices,
            currencies and crypto need no reference and get none.
          </dd>
        </dl>
      </section>

      <section className="panel chart-panel guide" aria-labelledby="guide-trust">
        <h3 id="guide-trust">How a model earns trust</h3>
        <p>
          For every instrument with enough history (about two and a half years), the app replays the past one day at a time.
          Each day, each model forecasts the next day using only what it could have known then. The forecasts are scored against
          what actually happened and compared with the simplest baseline: “the next day will look like the last two years.”
        </p>
        <p>
          A model <strong>earns its place</strong> only if it beats that baseline by more than luck would explain <em>and</em>{' '}
          its bad-day estimates (VaR) hold up: losses beyond the threshold happen about as often as promised, and not in
          clusters. Many instruments end up with no winner. That is a real finding, not a failure of the app.
        </p>
      </section>

      <section className="panel chart-panel guide" aria-labelledby="guide-compare">
        <h3 id="guide-compare">Comparing instruments</h3>
        <p>
          The Compare tab lines up 2 to 10 instruments on the days they all traded. Correlation shows whether they move together
          (holding things that move together diversifies little). The shared factor shows how much of everything is one common
          “market” move. Cointegration finds pairs whose prices stay tied in the long run. Markets in different time zones close
          hours apart, which makes daily correlation look too low, so the weekly figure is shown first for them. And with many
          pairs, a few will look tied by pure chance; the app tells you how many to expect.
        </p>
      </section>

      <section className="panel chart-panel guide" aria-labelledby="guide-limits">
        <h3 id="guide-limits">Limits to keep in mind</h3>
        <ul className="plain-list">
          <li>Past behaviour describes risk; it doesn’t promise future returns.</li>
          <li>Yahoo is free and unofficial: occasionally slow, delayed or briefly unavailable. Errors say so plainly.</li>
          <li>Returns are in each instrument’s own currency; comparisons across currencies include exchange-rate moves.</li>
          <li>Events, news and company fundamentals are not part of the analysis yet.</li>
        </ul>
      </section>

      <section className="panel chart-panel guide" aria-labelledby="guide-glossary">
        <h3 id="guide-glossary">Glossary</h3>
        <dl className="guide-list" data-testid="glossary">
          {GLOSSARY.map(([term, meaning]) => (
            <div key={term}>
              <dt>{term}</dt>
              <dd>{meaning}</dd>
            </div>
          ))}
        </dl>
        <p className="quiet-copy">
          Every formula, with its source and how we verified it, is in the{' '}
          <a href={METHODOLOGY_URL} target="_blank" rel="noopener noreferrer">
            methodology document
          </a>
          .
        </p>
      </section>
    </>
  )
}

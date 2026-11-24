//! Exchange metadata keyed by Yahoo symbol suffix: which index is the local market benchmark
//! and how TradingView names the listing. Every benchmark symbol here was checked against
//! Yahoo's chart endpoint (2026-11-24).

/// MSCI ACWI (iShares ETF, USD): the fallback "world market" benchmark for equities whose
/// exchange isn't in the table.
pub const GLOBAL_BENCHMARK: (&str, &str) = ("ACWI", "MSCI ACWI (global)");
/// US listings have no suffix on Yahoo.
pub const US_BENCHMARK: (&str, &str) = ("^GSPC", "S&P 500");

pub struct Market {
    pub suffix: &'static str,
    pub benchmark: &'static str,
    pub benchmark_name: &'static str,
    pub tradingview_prefix: &'static str,
}

const fn m(
    suffix: &'static str,
    benchmark: &'static str,
    benchmark_name: &'static str,
    tradingview_prefix: &'static str,
) -> Market {
    Market {
        suffix,
        benchmark,
        benchmark_name,
        tradingview_prefix,
    }
}

pub const MARKETS: &[Market] = &[
    m("NS", "^NSEI", "NIFTY 50", "NSE"),
    m("BO", "^BSESN", "S&P BSE SENSEX", "BSE"),
    m("L", "^FTSE", "FTSE 100", "LSE"),
    m("DE", "^GDAXI", "DAX", "XETR"),
    m("F", "^GDAXI", "DAX", "FWB"),
    m("PA", "^FCHI", "CAC 40", "EURONEXT"),
    m("AS", "^AEX", "AEX", "EURONEXT"),
    m("BR", "^BFX", "BEL 20", "EURONEXT"),
    m("MI", "FTSEMIB.MI", "FTSE MIB", "MIL"),
    m("MC", "^IBEX", "IBEX 35", "BME"),
    m("SW", "^SSMI", "SMI", "SIX"),
    m("ST", "^OMX", "OMX Stockholm 30", "OMXSTO"),
    m("CO", "^OMXC25", "OMX Copenhagen 25", "OMXCOP"),
    m("OL", "OSEBX.OL", "Oslo Børs Benchmark", "OSL"),
    m("T", "^N225", "Nikkei 225", "TSE"),
    m("HK", "^HSI", "Hang Seng", "HKEX"),
    m("SS", "000001.SS", "SSE Composite", "SSE"),
    m("SZ", "399001.SZ", "SZSE Component", "SZSE"),
    m("KS", "^KS11", "KOSPI", "KRX"),
    m("KQ", "^KQ11", "KOSDAQ", "KRX"),
    m("TW", "^TWII", "TAIEX", "TWSE"),
    m("AX", "^AXJO", "S&P/ASX 200", "ASX"),
    m("NZ", "^NZ50", "S&P/NZX 50", "NZX"),
    m("TO", "^GSPTSE", "S&P/TSX Composite", "TSX"),
    m("SA", "^BVSP", "Ibovespa", "BMFBOVESPA"),
    m("MX", "^MXX", "IPC Mexico", "BMV"),
    m("SI", "^STI", "Straits Times", "SGX"),
    m("JK", "^JKSE", "IDX Composite", "IDX"),
    m("KL", "^KLSE", "FTSE Bursa Malaysia KLCI", "MYX"),
    m("BK", "^SET.BK", "SET", "SET"),
    m("TA", "TA35.TA", "TA-35", "TASE"),
    m("IS", "XU100.IS", "BIST 100", "BIST"),
    m("SR", "^TASI.SR", "Tadawul All Share", "TADAWUL"),
    m("JO", "^J203.JO", "JSE All Share", "JSE"),
];

/// Indices TradingView knows under a different name.
const TRADINGVIEW_INDICES: &[(&str, &str)] = &[
    ("^GSPC", "SP:SPX"),
    ("^IXIC", "NASDAQ:IXIC"),
    ("^DJI", "DJ:DJI"),
    ("^NSEI", "NSE:NIFTY"),
    ("^NSEBANK", "NSE:BANKNIFTY"),
    ("^BSESN", "BSE:SENSEX"),
    ("^N225", "TVC:NI225"),
    ("^FTSE", "TVC:UKX"),
    ("^GDAXI", "XETR:DAX"),
    ("^HSI", "TVC:HSI"),
];

fn split(symbol: &str) -> (&str, Option<&str>) {
    match symbol.rsplit_once('.') {
        Some((code, suffix))
            if !symbol.starts_with('^') || suffix.chars().all(|c| c.is_ascii_uppercase()) =>
        {
            (code, Some(suffix))
        }
        _ => (symbol, None),
    }
}

pub fn market(symbol: &str) -> Option<&'static Market> {
    let (_, suffix) = split(symbol);
    MARKETS.iter().find(|m| Some(m.suffix) == suffix)
}

/// Non-equity Yahoo symbols: indices (^), FX (=X), futures (=F), crypto pairs (BTC-USD).
fn is_market_instrument(symbol: &str, instrument_type: Option<&str>) -> bool {
    symbol.starts_with('^')
        || symbol.contains('=')
        || matches!(
            instrument_type,
            Some("INDEX" | "CRYPTOCURRENCY" | "CURRENCY" | "FUTURE")
        )
}

/// The index an equity is measured against: its home market's main index, the S&P 500 for
/// US listings, otherwise MSCI ACWI. None for indices, FX, futures and crypto: they *are*
/// the market, and the analysis needs no benchmark to run.
pub fn default_benchmark(
    symbol: &str,
    instrument_type: Option<&str>,
    currency: Option<&str>,
) -> Option<(&'static str, &'static str)> {
    if is_market_instrument(symbol, instrument_type) {
        return None;
    }
    if let Some(m) = market(symbol) {
        return Some((m.benchmark, m.benchmark_name));
    }
    let (_, suffix) = split(symbol);
    if suffix.is_none() && currency.is_none_or(|c| c == "USD") {
        return Some(US_BENCHMARK);
    }
    Some(GLOBAL_BENCHMARK)
}

/// True when the benchmark trades on the instrument's own exchange, so its trading days are a
/// valid calendar for gap checks.
pub fn is_local_benchmark(symbol: &str, benchmark: &str) -> bool {
    market(symbol).is_some_and(|m| m.benchmark == benchmark)
        || (split(symbol).1.is_none() && benchmark == US_BENCHMARK.0)
}

/// TradingView's name for a Yahoo symbol, when we know it.
pub fn tradingview_symbol(symbol: &str) -> Option<String> {
    if let Some((_, tv)) = TRADINGVIEW_INDICES.iter().find(|(y, _)| *y == symbol) {
        return Some((*tv).to_string());
    }
    if symbol.starts_with('^') || symbol.contains('=') {
        return None;
    }
    // TradingView spells M&M and BAJAJ-AUTO as M_M and BAJAJ_AUTO.
    let clean = |code: &str| code.replace(['&', '-'], "_");
    match split(symbol) {
        (code, None) if !code.contains('-') => Some(code.to_string()),
        (code, Some(_)) => market(symbol).map(|m| {
            // Hong Kong: Yahoo 0700.HK is TradingView HKEX:700.
            let code = if m.suffix == "HK" {
                code.trim_start_matches('0')
            } else {
                code
            };
            format!("{}:{}", m.tradingview_prefix, clean(code))
        }),
        _ => None, // crypto pairs like BTC-USD: no single TradingView venue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmarks_by_market() {
        assert_eq!(
            default_benchmark("RELIANCE.NS", Some("EQUITY"), Some("INR")),
            Some(("^NSEI", "NIFTY 50"))
        );
        assert_eq!(
            default_benchmark("AAPL", Some("EQUITY"), Some("USD")),
            Some(US_BENCHMARK)
        );
        assert_eq!(
            default_benchmark("7203.T", Some("EQUITY"), Some("JPY"))
                .unwrap()
                .0,
            "^N225"
        );
        assert_eq!(
            default_benchmark("SAP.DE", Some("EQUITY"), Some("EUR"))
                .unwrap()
                .0,
            "^GDAXI"
        );
        assert_eq!(
            default_benchmark("0700.HK", Some("EQUITY"), Some("HKD"))
                .unwrap()
                .0,
            "^HSI"
        );
        // Unknown exchange: global benchmark rather than none.
        assert_eq!(
            default_benchmark("NOKIA.HE", Some("EQUITY"), Some("EUR")),
            Some(GLOBAL_BENCHMARK)
        );
        // Market instruments need no benchmark.
        for s in ["^NSEI", "^GSPC", "EURUSD=X", "GC=F"] {
            assert_eq!(default_benchmark(s, None, None), None, "{s}");
        }
        assert_eq!(
            default_benchmark("BTC-USD", Some("CRYPTOCURRENCY"), Some("USD")),
            None
        );
    }

    #[test]
    fn index_suffixes_are_not_mistaken_for_exchanges() {
        // ^SET.BK is an index with a suffix; it still needs no benchmark.
        assert_eq!(
            default_benchmark("^SET.BK", Some("INDEX"), Some("THB")),
            None
        );
        assert!(market("^SET.BK").is_some_and(|m| m.suffix == "BK"));
    }

    #[test]
    fn local_benchmark_detection() {
        assert!(is_local_benchmark("TCS.NS", "^NSEI"));
        assert!(is_local_benchmark("MSFT", "^GSPC"));
        assert!(!is_local_benchmark("NOKIA.HE", "ACWI"));
        assert!(!is_local_benchmark("TCS.NS", "^GSPC"));
    }

    #[test]
    fn tradingview_names() {
        let cases = [
            ("RELIANCE.NS", Some("NSE:RELIANCE")),
            ("M&M.NS", Some("NSE:M_M")),
            ("BAJAJ-AUTO.NS", Some("NSE:BAJAJ_AUTO")),
            ("AAPL", Some("AAPL")),
            ("BRK-B", None),
            ("0700.HK", Some("HKEX:700")),
            ("7203.T", Some("TSE:7203")),
            ("SAP.DE", Some("XETR:SAP")),
            ("^NSEI", Some("NSE:NIFTY")),
            ("^GSPC", Some("SP:SPX")),
            ("^XYZ", None),
            ("EURUSD=X", None),
            ("BTC-USD", None),
            ("NOKIA.HE", None),
        ];
        for (yahoo, tv) in cases {
            assert_eq!(tradingview_symbol(yahoo).as_deref(), tv, "{yahoo}");
        }
    }
}

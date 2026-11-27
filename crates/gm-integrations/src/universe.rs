//! The instruments the daily list scans and the news recorder tags: the NIFTY 50 (membership
//! as of late 2025, every ticker checked against Yahoo on 27 Nov 2026), with the names
//! headlines use for each company.
//! ponytail: a hand-kept list. NSE reshuffles the index twice a year; update it then.

/// (Yahoo symbol, names used in headlines separated by `|`).
pub const NIFTY50: [(&str, &str); 50] = [
    ("ADANIENT.NS", "Adani Enterprises"),
    ("ADANIPORTS.NS", "Adani Ports"),
    ("APOLLOHOSP.NS", "Apollo Hospitals"),
    ("ASIANPAINT.NS", "Asian Paints"),
    ("AXISBANK.NS", "Axis Bank"),
    ("BAJAJ-AUTO.NS", "Bajaj Auto"),
    ("BAJFINANCE.NS", "Bajaj Finance"),
    ("BAJAJFINSV.NS", "Bajaj Finserv"),
    ("BEL.NS", "Bharat Electronics|BEL"),
    ("BHARTIARTL.NS", "Bharti Airtel|Airtel"),
    ("CIPLA.NS", "Cipla"),
    ("COALINDIA.NS", "Coal India"),
    ("DRREDDY.NS", "Dr Reddy"),
    ("EICHERMOT.NS", "Eicher Motors|Royal Enfield"),
    ("ETERNAL.NS", "Eternal|Zomato|Blinkit"),
    ("GRASIM.NS", "Grasim"),
    ("HCLTECH.NS", "HCLTech|HCL Tech|HCL Technologies"),
    ("HDFCBANK.NS", "HDFC Bank"),
    ("HDFCLIFE.NS", "HDFC Life"),
    ("HINDALCO.NS", "Hindalco"),
    ("HINDUNILVR.NS", "Hindustan Unilever|HUL"),
    ("ICICIBANK.NS", "ICICI Bank"),
    ("INDIGO.NS", "IndiGo|InterGlobe"),
    ("INFY.NS", "Infosys"),
    ("ITC.NS", "ITC"),
    ("JIOFIN.NS", "Jio Financial"),
    ("JSWSTEEL.NS", "JSW Steel"),
    ("KOTAKBANK.NS", "Kotak Mahindra Bank|Kotak Bank"),
    ("LT.NS", "Larsen|L&T"),
    ("M&M.NS", "Mahindra & Mahindra|M&M"),
    ("MARUTI.NS", "Maruti"),
    ("MAXHEALTH.NS", "Max Healthcare"),
    ("NESTLEIND.NS", "Nestle India|Nestlé India"),
    ("NTPC.NS", "NTPC"),
    ("ONGC.NS", "ONGC"),
    ("POWERGRID.NS", "Power Grid"),
    ("RELIANCE.NS", "Reliance Industries|RIL"),
    ("SBILIFE.NS", "SBI Life"),
    ("SHRIRAMFIN.NS", "Shriram Finance"),
    ("SBIN.NS", "State Bank of India|SBI"),
    ("SUNPHARMA.NS", "Sun Pharma"),
    ("TATACONSUM.NS", "Tata Consumer"),
    ("TMPV.NS", "Tata Motors"),
    ("TATASTEEL.NS", "Tata Steel"),
    ("TCS.NS", "TCS|Tata Consultancy"),
    ("TECHM.NS", "Tech Mahindra"),
    ("TITAN.NS", "Titan"),
    ("TRENT.NS", "Trent"),
    ("ULTRACEMCO.NS", "UltraTech"),
    ("WIPRO.NS", "Wipro"),
];

fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '&')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect()
}

/// NIFTY 50 symbols a headline names, matching whole words ("ITC" never matches "switch").
/// "SBI Life" names only SBI Life, not State Bank of India too.
pub fn companies_named(title: &str) -> Vec<&'static str> {
    let ws = tokens(&title.replace('é', "e"));
    let contains = |phrase: &str| {
        let p = tokens(&phrase.replace('é', "e"));
        !p.is_empty() && ws.windows(p.len()).any(|w| w == p.as_slice())
    };
    let mut found: Vec<&'static str> = NIFTY50
        .iter()
        .filter(|(_, names)| names.split('|').any(contains))
        .map(|(symbol, _)| *symbol)
        .collect();
    if found.contains(&"SBILIFE.NS") && !contains("State Bank of India") {
        found.retain(|s| *s != "SBIN.NS");
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_whole_words_only() {
        assert_eq!(companies_named("ITC shares rise after Q2"), ["ITC.NS"]);
        assert!(companies_named("WhatsApp's rupee switch is a marketing problem").is_empty());
        assert_eq!(
            companies_named("Dr. Reddy's gets USFDA nod; HCLTech wins deal"),
            ["DRREDDY.NS", "HCLTECH.NS"]
        );
        assert_eq!(
            companies_named("SBI Life premiums jump 20%"),
            ["SBILIFE.NS"]
        );
        assert_eq!(
            companies_named("L&T bags order; M&M sales up"),
            ["LT.NS", "M&M.NS"]
        );
        assert_eq!(companies_named("Nestlé India Q2 profit"), ["NESTLEIND.NS"]);
        assert!(companies_named("Reliance Power shares hit upper circuit").is_empty());
    }
}

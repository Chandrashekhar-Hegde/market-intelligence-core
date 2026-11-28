//! Numbers the way an Indian reader writes them.

/// ₹1,23,456 (lakh grouping) for INR; 1,234,567 with the currency code otherwise.
pub fn money(v: f64, currency: &str) -> String {
    let n = v.round().abs() as u64;
    let digits = n.to_string();
    let grouped = if currency == "INR" || currency.is_empty() {
        if digits.len() <= 3 {
            digits
        } else {
            let (head, tail) = digits.split_at(digits.len() - 3);
            let mut parts: Vec<String> = Vec::new();
            let mut h = head.to_string();
            while h.len() > 2 {
                parts.insert(0, h.split_off(h.len() - 2));
            }
            if !h.is_empty() {
                parts.insert(0, h);
            }
            format!("{},{tail}", parts.join(","))
        }
    } else {
        let b = digits.as_bytes();
        b.rchunks(3)
            .rev()
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect::<Vec<_>>()
            .join(",")
    };
    let sign = if v < 0.0 { "−" } else { "" };
    match currency {
        "INR" | "" => format!("{sign}₹{grouped}"),
        c => format!("{sign}{grouped} {c}"),
    }
}

pub fn price(v: f64) -> String {
    let s = format!("{:.2}", v.abs());
    let (whole, cents) = s.split_once('.').unwrap_or((&s, "00"));
    let grouped = money(whole.parse().unwrap_or(0.0), "INR")
        .trim_start_matches('₹')
        .to_string();
    format!("{}{grouped}.{cents}", if v < 0.0 { "−" } else { "" })
}

pub fn pct(x: f64) -> String {
    format!(
        "{}{:.2}%",
        if x >= 0.0 { "+" } else { "−" },
        (x * 100.0).abs()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lakh_grouping() {
        assert_eq!(money(10_000.0, "INR"), "₹10,000");
        assert_eq!(money(123_456.0, "INR"), "₹1,23,456");
        assert_eq!(money(12_345_678.0, "INR"), "₹1,23,45,678");
        assert_eq!(money(999.4, "INR"), "₹999");
        assert_eq!(money(1_234_567.0, "USD"), "1,234,567 USD");
        assert_eq!(price(25607.2), "25,607.20");
        assert_eq!(pct(-0.0123), "−1.23%");
    }
}

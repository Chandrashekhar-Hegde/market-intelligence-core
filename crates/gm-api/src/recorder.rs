//! The calibration run (owner decision, 27 Nov 2026): every headline the desk sources carry is
//! appended once to `news.jsonl` in `GM_DATA_DIR` (a Railway volume), with the companies it
//! names and how the built-in readers read it. What prices did afterwards is worked out from
//! price history when the calibration is asked for, so nothing but headlines is stored.
//! ponytail: one append-only JSONL file and an in-memory set of seen titles; fine for a few
//! hundred headlines a day for years (~60 MB/year). Move to Postgres (#80) if it grows past that.

use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use gm_domain::analysis::{
    macro_news::read_macro,
    news_reader::{Reading, read_headline},
};
use gm_integrations::{
    news::{Story, dedupe_key},
    universe::companies_named,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub recorded_at: DateTime<Utc>,
    pub seen_at: DateTime<Utc>,
    pub title: String,
    pub url: String,
    pub publisher: String,
    pub source: String,
    /// NIFTY 50 symbols the headline names.
    pub symbols: Vec<String>,
    /// The company reader's reading (built-in).
    pub reading: Reading,
    /// The market-news reader's theme and (asset, direction) effects.
    pub theme: String,
    pub impacts: Vec<(String, i8)>,
}

pub const BACKFILL_FILE: &str = "news_backfill.jsonl";
/// The model's company readings of recorded headlines, one `{title, reading}` per line.
pub const MODEL_FILE: &str = "model_readings.jsonl";

#[derive(Serialize, Deserialize)]
struct ModelRow {
    title: String,
    reading: Reading,
}

pub struct Recorder {
    path: PathBuf,
    seen: Mutex<HashSet<String>>,
}

impl Recorder {
    /// Opens (creating) `dir/news.jsonl` and remembers every title already in it.
    pub fn open(dir: &std::path::Path) -> std::io::Result<Self> {
        Self::open_file(dir, "news.jsonl")
    }

    /// The historical backfill (`backfill_news` binary) writes here, beside the live file.
    pub fn open_backfill(dir: &std::path::Path) -> std::io::Result<Self> {
        Self::open_file(dir, BACKFILL_FILE)
    }

    fn open_file(dir: &std::path::Path, name: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(name);
        let mut seen = HashSet::new();
        if path.exists() {
            for line in BufReader::new(std::fs::File::open(&path)?).lines() {
                if let Ok(r) = serde_json::from_str::<Record>(&line?) {
                    seen.insert(dedupe_key(&r.title));
                }
            }
        }
        Ok(Self {
            path,
            seen: Mutex::new(seen),
        })
    }

    /// Appends the stories not recorded before; returns how many were new.
    pub fn record(&self, stories: &[Story]) -> std::io::Result<usize> {
        let mut seen = self.seen.lock().expect("recorder lock");
        let now = Utc::now();
        let mut lines = String::new();
        let mut new = 0;
        for story in stories {
            if !seen.insert(dedupe_key(&story.title)) {
                continue;
            }
            let reading = read_macro(&story.title);
            let record = Record {
                recorded_at: now,
                seen_at: story.seen_at,
                title: story.title.clone(),
                url: story.url.clone(),
                publisher: story.publisher.clone(),
                source: story.source.clone(),
                symbols: companies_named(&story.title)
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                reading: read_headline(&story.title),
                theme: reading.theme.to_string(),
                impacts: reading
                    .impacts
                    .iter()
                    .map(|i| (i.asset.to_string(), i.direction))
                    .collect(),
            };
            lines.push_str(&serde_json::to_string(&record).expect("record serialises"));
            lines.push('\n');
            new += 1;
        }
        if new > 0 {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?
                .write_all(lines.as_bytes())?;
        }
        Ok(new)
    }

    /// The model's stored readings, by title.
    pub fn model_readings(&self) -> std::collections::HashMap<String, Reading> {
        let path = self.path.with_file_name(MODEL_FILE);
        let Ok(file) = std::fs::File::open(path) else {
            return Default::default();
        };
        BufReader::new(file)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| serde_json::from_str::<ModelRow>(&l).ok())
            .map(|r| (r.title, r.reading))
            .collect()
    }

    pub fn append_model_readings(&self, rows: &[(String, Reading)]) -> std::io::Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let _guard = self.seen.lock().expect("recorder lock");
        let mut text = String::new();
        for (title, reading) in rows {
            text.push_str(
                &serde_json::to_string(&ModelRow {
                    title: title.clone(),
                    reading: reading.clone(),
                })
                .expect("serialises"),
            );
            text.push('\n');
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path.with_file_name(MODEL_FILE))?
            .write_all(text.as_bytes())
    }

    /// Every record: the live file and, beside it, the historical backfill.
    pub fn read_all(&self) -> std::io::Result<Vec<Record>> {
        let _guard = self.seen.lock().expect("recorder lock");
        let mut out = Vec::new();
        let backfill = self.path.with_file_name(BACKFILL_FILE);
        for path in [&self.path, &backfill] {
            if path.exists() {
                out.extend(
                    BufReader::new(std::fs::File::open(path)?)
                        .lines()
                        .map_while(Result::ok)
                        .filter_map(|l| serde_json::from_str::<Record>(&l).ok()),
                );
            }
        }
        // The live file and the backfill may share a headline: keep the first.
        let mut seen = HashSet::new();
        out.retain(|r| seen.insert(dedupe_key(&r.title)));
        Ok(out)
    }
}

/// The first trading day a story can move the price on an Indian exchange: its own date if it
/// appeared before the 15:30 IST close, otherwise the next day (weekends and holidays fall to
/// the next bar when the window is taken).
pub fn event_date(seen_at: DateTime<Utc>) -> chrono::NaiveDate {
    let ist = seen_at + chrono::Duration::minutes(330);
    let date = ist.date_naive();
    if ist.time() >= chrono::NaiveTime::from_hms_opt(15, 30, 0).expect("valid time") {
        date.succ_opt().unwrap_or(date)
    } else {
        date
    }
}

/// Log return from the close before the first bar on or after `day` to the close `h` sessions
/// later (that bar included). None until the window has completed.
pub fn window_return(
    bars: &[(chrono::NaiveDate, f64)],
    day: chrono::NaiveDate,
    h: usize,
) -> Option<f64> {
    let start = bars.iter().position(|(d, _)| *d >= day)?;
    let before = bars.get(start.checked_sub(1)?)?.1;
    let end = bars.get(start + h.checked_sub(1)?)?.1;
    Some((end / before).ln())
}

/// Log return you could still have earned after the news: from the close of the event day (the
/// first bar on or after `day`, when someone who read it could act) to `h` sessions later.
pub fn window_after(
    bars: &[(chrono::NaiveDate, f64)],
    day: chrono::NaiveDate,
    h: usize,
) -> Option<f64> {
    let start = bars.iter().position(|(d, _)| *d >= day)?;
    let entry = bars.get(start)?.1;
    let exit = bars.get(start + h)?.1;
    Some((exit / entry).ln())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_dates_follow_the_nse_close_and_windows_need_full_data() {
        let at = |s: &str| s.parse::<DateTime<Utc>>().unwrap();
        let d = |s: &str| s.parse::<chrono::NaiveDate>().unwrap();
        // 09:59 UTC = 15:29 IST: same day; 10:00 UTC = 15:30 IST: next day.
        assert_eq!(event_date(at("2026-11-26T09:59:00Z")), d("2026-11-26"));
        assert_eq!(event_date(at("2026-11-26T10:00:00Z")), d("2026-11-27"));
        let bars = [
            (d("2026-11-24"), 100.0),
            (d("2026-11-25"), 101.0),
            (d("2026-11-27"), 103.0),
            (d("2026-11-30"), 99.0),
        ];
        // Story on the 26th (a holiday): the window starts on the 27th, from the 25th's close.
        assert!(
            (window_return(&bars, d("2026-11-26"), 1).unwrap() - (103.0f64 / 101.0).ln()).abs()
                < 1e-12
        );
        assert!(
            (window_return(&bars, d("2026-11-26"), 2).unwrap() - (99.0f64 / 101.0).ln()).abs()
                < 1e-12
        );
        assert_eq!(window_return(&bars, d("2026-11-26"), 3), None);
        assert_eq!(window_return(&bars, d("2026-11-24"), 1), None);
        // After the news: from the event day's close (27th) one session on (30th).
        assert!(
            (window_after(&bars, d("2026-11-26"), 1).unwrap() - (99.0f64 / 103.0).ln()).abs()
                < 1e-12
        );
        assert_eq!(window_after(&bars, d("2026-11-26"), 2), None);
    }

    fn story(title: &str) -> Story {
        Story {
            title: title.into(),
            url: "https://example.com".into(),
            publisher: "Mint".into(),
            source: "Mint".into(),
            seen_at: "2026-11-27T05:00:00Z".parse().unwrap(),
            summary: None,
        }
    }

    #[test]
    fn records_each_headline_once_across_restarts() {
        let dir = std::env::temp_dir().join(format!("gm-recorder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let r = Recorder::open(&dir).unwrap();
        let batch = [
            story("Infosys wins $1.5 billion deal"),
            story("Crude oil prices rise for a third day"),
            story("INFOSYS wins $1.5 billion deal!"),
        ];
        assert_eq!(r.record(&batch).unwrap(), 2);
        assert_eq!(r.record(&batch).unwrap(), 0);
        // A restart remembers what is already on disk.
        let again = Recorder::open(&dir).unwrap();
        assert_eq!(again.record(&batch).unwrap(), 0);
        let all = again.read_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].symbols, ["INFY.NS"]);
        assert_eq!(
            all[0].reading,
            read_headline("Infosys wins $1.5 billion deal")
        );
        assert_eq!(all[1].impacts[0], ("crude".to_string(), 1));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

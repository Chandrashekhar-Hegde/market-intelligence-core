//! One-off historical news backfill for the calibration run (owner decision, 27 Nov 2026):
//! Google News, month by month, for each NIFTY 50 company, written in the recorder's format to
//! `news_backfill.jsonl` in the data directory. Run from a workstation, not the server:
//!
//!     cargo run --release -p gm-api --bin backfill_news -- <data-dir> [months=36]
//!     cargo run --release -p gm-api --bin backfill_news -- read <data-dir>
//!
//! `read` has the configured model (`GM_LLM_*`) read every recorded headline that names a
//! company and isn't read yet, writing `model_readings.jsonl`; paced for a free tier and
//! resumable (stops at the first refusal, e.g. the daily quota; rerun the next day).
//!
//! Paced (one search every 3 s, a minute's pause when Google refuses) and resumable: finished
//! company-months are listed in `backfill_progress.txt` and skipped on the next run.

use std::{collections::HashSet, io::Write, path::PathBuf, time::Duration};

use chrono::{Datelike, Months, NaiveDate, Utc};
use gm_api::recorder::Recorder;
use gm_integrations::{news::NewsClient, universe::NIFTY50};

const PACE: Duration = Duration::from_secs(3);
const BACKOFF: Duration = Duration::from_secs(60);

async fn read_with_model(dir: PathBuf) -> anyhow::Result<()> {
    let config = gm_integrations::llm::LlmConfig::from_env()
        .ok_or_else(|| anyhow::anyhow!("set GM_LLM_BASE_URL, GM_LLM_MODEL and GM_LLM_API_KEY"))?;
    let pace = config.pace;
    let llm = gm_integrations::llm::LlmReader::new(config);
    let recorder = Recorder::open(&dir)?;
    let known = recorder.model_readings();
    let mut titles: Vec<String> = recorder
        .read_all()?
        .into_iter()
        .filter(|r| !r.symbols.is_empty() && !known.contains_key(&r.title))
        .map(|r| r.title)
        .collect();
    titles.dedup();
    println!(
        "{} headlines to read ({} already read)",
        titles.len(),
        known.len()
    );
    for (i, chunk) in titles.chunks(gm_integrations::llm::BATCH).enumerate() {
        // Transient network errors and busy periods: wait a minute and try again, three times.
        let mut attempt = 0;
        let readings = loop {
            match llm.read(chunk).await {
                Ok(r) => break r,
                Err(e) if attempt < 3 => {
                    attempt += 1;
                    eprintln!("batch {i}: {e}; retrying in a minute");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                Err(e) => anyhow::bail!("stopped after {i} batches: {e}"),
            }
        };
        recorder.append_model_readings(&chunk.iter().cloned().zip(readings).collect::<Vec<_>>())?;
        if (i + 1) % 20 == 0 {
            println!("{} read", (i + 1) * gm_integrations::llm::BATCH);
        }
        tokio::time::sleep(pace).await;
    }
    println!("done");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().map(String::as_str) == Some("read") {
        args.next();
        let dir = args
            .next()
            .ok_or_else(|| anyhow::anyhow!("usage: backfill_news read <data-dir>"))?;
        return read_with_model(PathBuf::from(dir)).await;
    }
    let dir = PathBuf::from(
        args.next()
            .ok_or_else(|| anyhow::anyhow!("usage: backfill_news <data-dir> [months]"))?,
    );
    let months: u32 = args.next().map(|m| m.parse()).transpose()?.unwrap_or(36);
    let recorder = Recorder::open_backfill(&dir)?;
    let progress_path = dir.join("backfill_progress.txt");
    let done: HashSet<String> = std::fs::read_to_string(&progress_path)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let mut progress = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&progress_path)?;
    let news = NewsClient::default();
    let this_month = Utc::now().date_naive().with_day(1).expect("day 1 exists");
    let (mut fetched, mut new_total, mut failures) = (0usize, 0usize, 0usize);
    for (symbol, names) in NIFTY50 {
        // The first name is the one headlines use most ("Reliance Industries", "HDFC Bank").
        let name = names.split('|').next().unwrap_or(symbol);
        for back in 1..=months {
            let start: NaiveDate = this_month - Months::new(back);
            let end = start + Months::new(1);
            let key = format!("{symbol} {}", start.format("%Y-%m"));
            if done.contains(&key) {
                continue;
            }
            let query = format!("\"{name}\" after:{start} before:{end}");
            let mut result = news.google_query(&query).await;
            if result.is_err() {
                eprintln!("{key}: refused, waiting a minute");
                tokio::time::sleep(BACKOFF).await;
                result = news.google_query(&query).await;
            }
            match result {
                Ok(stories) => {
                    let new = recorder.record(&stories)?;
                    writeln!(progress, "{key}")?;
                    fetched += 1;
                    new_total += new;
                    if fetched % 25 == 0 {
                        println!("{fetched} company-months, {new_total} new headlines");
                    }
                }
                Err(e) => {
                    failures += 1;
                    eprintln!("{key}: {e}");
                    if failures >= 10 {
                        anyhow::bail!("Google refused 10 times; stopping (rerun later to resume)");
                    }
                }
            }
            tokio::time::sleep(PACE).await;
        }
    }
    println!(
        "done: {fetched} company-months fetched, {new_total} new headlines, {failures} failures"
    );
    Ok(())
}

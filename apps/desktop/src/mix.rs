//! Your mix: pick holdings, say how much, get whole shares with equal risk from each
//! (`gm_domain::analysis::mix`).
use gm_domain::analysis::mix::{Allocation, allocate};
use gpui_kit::component::{
    ActiveTheme,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, Window, div, px,
};

use gpui_kit::prelude::FluentBuilder as _;

use crate::{api, fmt};

/// Up to this many holdings: the comparison behind the correlations takes at most 10.
const MAX_PICKS: usize = 10;

pub struct MixView {
    items: Option<Result<Vec<api::TodayItem>, String>>,
    picked: Vec<String>,
    amount: Entity<InputState>,
    result: Option<Result<Allocation, String>>,
    busy: bool,
    _tasks: Vec<Task<()>>,
}

impl MixView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let amount = cx.new(|cx| InputState::new(window, cx).default_value("100000"));
        let load = cx.spawn(async move |this, cx| {
            let items = cx.background_executor().spawn(async { api::today() }).await;
            if let Err(e) = &items {
                eprintln!("mix list failed: {e}");
            }
            this.update(cx, |v, cx| {
                v.items = Some(items);
                if std::env::var("GM_DESKTOP_SCREEN").as_deref() == Ok("mix") {
                    v.build(cx);
                }
                cx.notify();
            })
            .ok();
        });
        let picked = [
            "RELIANCE.NS",
            "HDFCBANK.NS",
            "INFY.NS",
            "ITC.NS",
            "GOLDBEES.NS",
        ]
        .map(String::from)
        .to_vec();
        Self {
            items: None,
            picked,
            amount,
            result: None,
            busy: false,
            _tasks: vec![load],
        }
    }

    fn buyable(&self) -> Vec<api::TodayItem> {
        let Some(Ok(items)) = &self.items else {
            return Vec::new();
        };
        items
            .iter()
            .filter(|i| i.error.is_none() && i.last_price.is_some())
            .filter(|i| !matches!(i.instrument_type.as_deref(), Some("INDEX" | "CURRENCY")))
            .cloned()
            .collect()
    }

    fn build(&mut self, cx: &mut Context<Self>) {
        let items = self.buyable();
        let budget: f64 = self
            .amount
            .read(cx)
            .value()
            .replace([',', '₹', ' '], "")
            .parse()
            .unwrap_or(0.0);
        if self.picked.len() < 2 || budget <= 0.0 {
            self.result = Some(Err("Pick at least two holdings and an amount.".into()));
            return cx.notify();
        }
        // Prices in rupees: dollar listings at today's USD/INR.
        let usd_inr = self
            .items
            .as_ref()
            .and_then(|r| r.as_ref().ok())
            .and_then(|all| all.iter().find(|i| i.symbol == "INR=X"))
            .and_then(|i| i.last_price);
        let picked = self.picked.clone();
        let prices: Vec<f64> = picked
            .iter()
            .map(|s| {
                let i = items.iter().find(|i| &i.symbol == s);
                let p = i.and_then(|i| i.last_price).unwrap_or(0.0);
                match i.and_then(|i| i.currency.as_deref()) {
                    Some("USD") => usd_inr.map_or(0.0, |fx| p * fx),
                    _ => p,
                }
            })
            .collect();
        self.busy = true;
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    api::covariance(&picked).map(|cov| allocate(&picked, &prices, &cov, budget))
                })
                .await;
            this.update(cx, |v, cx| {
                v.result = Some(result);
                v.busy = false;
                cx.notify();
            })
            .ok();
        });
        self._tasks.push(task);
        cx.notify();
    }
}

impl Render for MixView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let header = v_flex()
            .child(div().text_2xl().font_weight(FontWeight::BOLD).child("Your mix"))
            .child(div().text_sm().text_color(muted).child(
                "Pick up to 10 holdings and the amount. Each is sized so it adds the same risk, allowing for how they move together, and bought in whole shares. With no proven edge between them, this is the evidence-based default, not a forecast.",
            ));
        let items = self.buyable();
        if let Some(Err(e)) = &self.items {
            return v_flex().max_w(px(1100.)).child(header).child(
                div()
                    .mt_6()
                    .text_color(cx.theme().danger)
                    .child(SharedString::from(e.clone())),
            );
        }
        if items.is_empty() {
            return v_flex()
                .max_w(px(1100.))
                .child(header)
                .child(div().mt_6().child("Loading the list…"));
        }
        let names = |s: &str| {
            items
                .iter()
                .find(|i| i.symbol == s)
                .and_then(|i| i.name.clone())
                .unwrap_or_else(|| s.to_string())
        };
        let picker = h_flex()
            .flex_wrap()
            .gap_1()
            .mt_4()
            .children(items.iter().map(|i| {
                let symbol = i.symbol.clone();
                let on = self.picked.contains(&symbol);
                Button::new(SharedString::from(format!("pick-{symbol}")))
                    .label(i.name.clone().unwrap_or_else(|| symbol.clone()))
                    .compact()
                    .when(on, |b| b.primary())
                    .when(!on, |b| b.outline())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(k) = this.picked.iter().position(|s| s == &symbol) {
                            this.picked.remove(k);
                        } else if this.picked.len() < MAX_PICKS {
                            this.picked.push(symbol.clone());
                        }
                        cx.notify();
                    }))
            }));
        let controls = h_flex()
            .mt_4()
            .gap_3()
            .items_center()
            .child(div().child("Amount ₹"))
            .child(div().w(px(200.)).child(Input::new(&self.amount)))
            .child(
                Button::new("build")
                    .label(if self.busy {
                        "Working…"
                    } else {
                        "Build the mix"
                    })
                    .primary()
                    .on_click(cx.listener(|this, _, _, cx| this.build(cx))),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(format!("{} picked", self.picked.len())),
            );
        let result = match &self.result {
            None => div(),
            Some(Err(e)) => {
                crate::ready(window, cx, "mix");
                div()
                    .mt_4()
                    .text_color(cx.theme().danger)
                    .child(SharedString::from(e.clone()))
            }
            Some(Ok(a)) => {
                crate::ready(window, cx, "mix");
                let bad_day = 1.645 * a.vol_annual / 252f64.sqrt() * a.invested;
                let mut out = v_flex()
                    .mt_6()
                    .child(div().text_lg().font_weight(FontWeight::BOLD).child(format!(
                        "Buy these: {} invested, {} left in cash",
                        fmt::money(a.invested, "INR"),
                        fmt::money(a.cash_left, "INR")
                    )))
                    .child(
                        h_flex()
                            .mt_2()
                            .py_1()
                            .border_b_1()
                            .border_color(border)
                            .text_sm()
                            .text_color(muted)
                            .child(div().w(px(300.)).child("Holding"))
                            .child(div().w(px(90.)).text_right().child("Shares"))
                            .child(div().w(px(120.)).text_right().child("Price"))
                            .child(div().w(px(130.)).text_right().child("Amount"))
                            .child(div().w(px(90.)).text_right().child("Share")),
                    )
                    .children(a.lines.iter().map(|l| {
                        h_flex()
                            .py_1()
                            .border_b_1()
                            .border_color(border)
                            .child(div().w(px(300.)).truncate().child(names(&l.symbol)))
                            .child(div().w(px(90.)).text_right().child(l.shares.to_string()))
                            .child(div().w(px(120.)).text_right().child(fmt::money(l.price, "INR")))
                            .child(div().w(px(130.)).text_right().child(fmt::money(l.amount, "INR")))
                            .child(div().w(px(90.)).text_right().child(format!("{:.0}%", l.weight * 100.0)))
                    }))
                    .child(div().mt_3().child(format!(
                        "Together they swing about {:.0}% a year, against {:.0}% for the same money in each alone, a cut of {:.0}%. On a bad day (1 in 20) the mix has typically lost about {}.",
                        a.vol_annual * 100.0,
                        a.vol_alone_avg * 100.0,
                        (1.0 - a.vol_annual / a.vol_alone_avg.max(1e-9)) * 100.0,
                        fmt::money(bad_day, "INR")
                    )));
                if !a.too_dear.is_empty() {
                    out = out.child(div().mt_2().text_sm().text_color(muted).child(format!(
                        "Left out because one share costs more than its part of this amount: {}. A larger amount includes them.",
                        a.too_dear.iter().map(|(s, p)| format!("{} ({})", names(s), fmt::money(*p, "INR"))).collect::<Vec<_>>().join(", ")
                    )));
                }
                out
            }
        };
        v_flex()
            .max_w(px(1100.))
            .child(header)
            .child(picker)
            .child(controls)
            .child(result)
    }
}

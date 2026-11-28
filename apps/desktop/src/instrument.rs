//! One instrument: what to do (from `gm_domain::analysis::plain`), where ₹10,000 could be, the
//! candles, and the maths behind it on request.
use gm_domain::analysis::{DEPOSIT_RATE, plain::RangeRow};
use gpui_kit::component::{
    ActiveTheme, Selectable as _,
    button::{Button, ButtonVariants as _},
    chart::CandlestickChart,
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, Window, div, px, relative,
};

use crate::{api, fmt};

const AMOUNT: f64 = 10_000.0;

enum Load {
    Loading(String),
    Failed(String),
    Ready(Box<api::Instrument>),
}

pub struct InstrumentView {
    search: Entity<InputState>,
    range: &'static str,
    state: Load,
    show_maths: bool,
    _task: Option<Task<()>>,
}

impl InstrumentView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Name or symbol, then Enter (e.g. Reliance, ^NSEI, GOLDBEES.NS)")
        });
        cx.subscribe_in(&search, window, |this, state, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = state.read(cx).value().to_string();
                this.load(text, cx);
            }
        })
        .detach();
        let symbol = std::env::var("GM_DESKTOP_SYMBOL").unwrap_or_else(|_| "^NSEI".into());
        let mut view = Self {
            search,
            range: "1y",
            state: Load::Loading(symbol.clone()),
            show_maths: false,
            _task: None,
        };
        view.load(symbol, cx);
        view
    }

    pub fn load(&mut self, text: String, cx: &mut Context<Self>) {
        self.state = Load::Loading(text.clone());
        let range = self.range;
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    api::resolve(&text).and_then(|s| api::instrument(&s, range, DEPOSIT_RATE))
                })
                .await;
            this.update(cx, |view, cx| {
                view.state = match result {
                    Ok(i) => Load::Ready(Box::new(i)),
                    Err(e) => Load::Failed(e),
                };
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    fn range_button(&self, range: &'static str, cx: &mut Context<Self>) -> Button {
        Button::new(range)
            .label(range)
            .ghost()
            .compact()
            .selected(self.range == range)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.range = range;
                if let Load::Ready(i) = &this.state {
                    let symbol = i.symbol.clone();
                    this.load(symbol, cx);
                }
            }))
    }
}

fn heading(text: &'static str) -> gpui_kit::Div {
    div()
        .mt_6()
        .mb_2()
        .text_lg()
        .font_weight(FontWeight::BOLD)
        .child(text)
}

/// Where ₹10,000 could be: one row per horizon, with a bar on a shared scale.
fn range_rows(rows: &[RangeRow], cx: &App) -> gpui_kit::Div {
    let t = cx.theme();
    let lo = rows.iter().map(|r| r.returns[0]).fold(0.0_f64, f64::min);
    let hi = rows.iter().map(|r| r.returns[4]).fold(0.0_f64, f64::max);
    let x = |r: f64| (((r - lo) / (hi - lo).max(1e-9)) as f32).clamp(0.0, 1.0);
    let money = |r: f64| fmt::money(AMOUNT * (1.0 + r), "INR");
    v_flex().gap_3().children(rows.iter().map(|row| {
        let [p5, p25, _, p75, p95] = row.returns;
        h_flex()
            .gap_4()
            .items_center()
            .child(
                div()
                    .w(px(90.))
                    .font_weight(FontWeight::BOLD)
                    .child(row.label.clone()),
            )
            .child(
                div()
                    .relative()
                    .w(px(320.))
                    .h(px(16.))
                    .rounded_sm()
                    .bg(t.muted)
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(relative(x(p5)))
                            .w(relative(x(p95) - x(p5)))
                            .bg(t.blue_light),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .left(relative(x(p25)))
                            .w(relative(x(p75) - x(p25)))
                            .bg(t.blue),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(-3.))
                            .bottom(px(-3.))
                            .left(relative(x(0.0)))
                            .w(px(2.))
                            .bg(t.foreground),
                    ),
            )
            .child(
                v_flex()
                    .child(div().child(format!(
                        "{} – {} in 9 of 10 past cases",
                        money(p5),
                        money(p95)
                    )))
                    .child(
                        div()
                            .text_sm()
                            .text_color(t.muted_foreground)
                            .child(format!(
                                "middle half {} – {} · tested here: held {}",
                                money(p25),
                                money(p75),
                                gm_domain::analysis::plain::in_100(row.held)
                            )),
                    ),
            )
    }))
}

impl Render for InstrumentView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let top = h_flex()
            .gap_2()
            .items_center()
            .child(div().w(px(520.)).child(Input::new(&self.search)))
            .child(self.range_button("1y", cx))
            .child(self.range_button("5y", cx))
            .child(self.range_button("10y", cx));
        let body = match &self.state {
            Load::Loading(what) => div().mt_6().child(SharedString::from(format!(
                "Loading {what}… (ten years of prices, tested)"
            ))),
            Load::Failed(e) => {
                eprintln!("instrument failed: {e}");
                crate::ready(window, "instrument");
                div()
                    .mt_6()
                    .text_color(cx.theme().danger)
                    .child(SharedString::from(e.clone()))
            }
            Load::Ready(i) => {
                crate::ready(window, "instrument");
                let p = &i.plain;
                let change = i
                    .change_pct
                    .map(|c| fmt::pct(c / 100.0))
                    .unwrap_or_default();
                let mut body = v_flex()
                    .child(
                        h_flex()
                            .mt_4()
                            .gap_3()
                            .items_baseline()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::BOLD)
                                    .child(i.name.clone()),
                            )
                            .child(div().text_color(muted).child(i.symbol.clone()))
                            .child(div().text_2xl().child(format!(
                                "{} {}",
                                fmt::price(i.price),
                                i.currency
                            )))
                            .child(div().text_color(muted).child(change)),
                    )
                    .child(
                        h_flex()
                            .mt_4()
                            .gap_3()
                            .items_center()
                            .child(crate::action_chip(p.action, cx))
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(p.headline.clone()),
                            ),
                    )
                    .child(v_flex().mt_3().gap_3().children(p.answers.iter().map(|a| {
                        v_flex()
                            .child(div().text_sm().text_color(muted).child(a.question.clone()))
                            .child(div().font_weight(FontWeight::BOLD).child(a.verdict.clone()))
                            .child(div().text_sm().child(a.why.clone()))
                    })));
                if let Some(tr) = &p.track_record {
                    body = body
                        .child(heading("Has this advice paid off here?"))
                        .child(div().child(tr.clone()));
                }
                if !p.ranges.is_empty() {
                    body = body
                        .child(heading("Where ₹10,000 could be"))
                        .child(range_rows(&p.ranges, cx));
                }
                if let Some(d) = &p.direction {
                    body = body.child(
                        div()
                            .mt_3()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(d.clone()),
                    );
                }
                if let Some(h) = &p.how {
                    body = body.child(div().mt_1().text_sm().text_color(muted).child(h.clone()));
                }
                let n = i.bars.len();
                body = body.child(heading("Price history")).child(
                    div().h(px(320.)).child(
                        CandlestickChart::new(i.bars.clone())
                            .x(|b| b.date.clone())
                            .open(|b| b.open)
                            .high(|b| b.high)
                            .low(|b| b.low)
                            .close(|b| b.close)
                            .tick_margin((n / 6).max(1))
                            .appear(false)
                            .tooltip_value(|_, _, v| fmt::price(v).into()),
                    ),
                );
                body.child(
                    Button::new("maths")
                        .label(if self.show_maths {
                            "Hide the maths"
                        } else {
                            "Show the maths"
                        })
                        .outline()
                        .mt_6()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_maths = !this.show_maths;
                            cx.notify();
                        })),
                )
                .when(self.show_maths, |b| {
                    b.child(
                        v_flex()
                            .mt_2()
                            .gap_2()
                            .children(i.findings.iter().map(|f| div().text_sm().child(f.clone()))),
                    )
                })
            }
        };
        v_flex().max_w(px(1100.)).child(top).child(body)
    }
}

use gpui_kit::prelude::FluentBuilder as _;

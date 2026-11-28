//! Today: every instrument in the list with one action and one line.
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{
    Context, EventEmitter, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task, Window, div, px,
};

use gm_domain::analysis::plain::Action;

use crate::{api, fmt};

pub struct Open(pub String);

pub struct TodayView {
    items: Option<Result<Vec<api::TodayItem>, String>>,
    brief: Option<Result<api::Brief, String>>,
    _tasks: [Task<()>; 2],
}

impl EventEmitter<Open> for TodayView {}

const GROUPS: [&str; 6] = [
    "India index",
    "NIFTY 50",
    "Gold, silver, rupee",
    "US from India",
    "AI",
    "Your watchlist",
];

impl TodayView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let task = cx.spawn(async move |this, cx| {
            let items = cx.background_executor().spawn(async { api::today() }).await;
            this.update(cx, |v, cx| {
                v.items = Some(items);
                cx.notify();
            })
            .ok();
        });
        let brief = cx.spawn(async move |this, cx| {
            let brief = cx.background_executor().spawn(async { api::brief() }).await;
            this.update(cx, |v, cx| {
                v.brief = Some(brief);
                cx.notify();
            })
            .ok();
        });
        Self {
            items: None,
            brief: None,
            _tasks: [task, brief],
        }
    }
}

impl TodayView {
    fn brief_card(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let t = cx.theme();
        let (muted, border, blue) = (t.muted_foreground, t.border, t.blue);
        let card = v_flex()
            .mt_4()
            .p_4()
            .gap_2()
            .rounded_md()
            .border_1()
            .border_color(border);
        match &self.brief {
            None => card.child(div().text_color(muted).child("Writing the morning brief…")),
            Some(Err(e)) => card.child(
                div()
                    .text_color(muted)
                    .child(SharedString::from(format!("No brief: {e}"))),
            ),
            Some(Ok(b)) => {
                let by = if b.writer == "template" {
                    "Written from the app's numbers".to_string()
                } else {
                    format!("Worded by {} from the app's numbers", b.writer)
                };
                let mut card = card.child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::BOLD)
                                .child("Morning brief"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .bg(blue)
                                .text_color(gpui_kit::white())
                                .text_xs()
                                .child(by),
                        ),
                );
                if let Some(note) = &b.note {
                    card = card.child(div().text_xs().text_color(muted).child(note.clone()));
                }
                card.children(b.sections.iter().map(|s| {
                    v_flex()
                        .mt_1()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(s.title.clone()),
                        )
                        .children(
                            s.bullets
                                .iter()
                                .map(|b| div().text_sm().child(format!("• {b}"))),
                        )
                }))
            }
        }
    }
}

impl Render for TodayView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let hover = cx.theme().list_hover;
        let header = v_flex()
            .child(div().text_2xl().font_weight(FontWeight::BOLD).child("Today"))
            .child(div().text_sm().text_color(muted).child(
                "One action for each holding, from what has been tested on its own past. Click a row for the full answer.",
            ));
        let items = match &self.items {
            None => {
                return v_flex()
                    .child(header)
                    .child(div().mt_6().child("Scanning the list…"));
            }
            Some(Err(e)) => {
                return v_flex().child(header).child(
                    div()
                        .mt_6()
                        .text_color(cx.theme().danger)
                        .child(SharedString::from(e.clone())),
                );
            }
            Some(Ok(items)) => items.clone(),
        };
        if self.brief.is_some() {
            crate::ready(window, cx, "today");
        }
        let ok: Vec<&api::TodayItem> = items.iter().filter(|i| i.error.is_none()).collect();
        let pick = |wanted: &[Action]| {
            ok.iter()
                .copied()
                .filter(|i| i.action.is_some_and(|a| wanted.contains(&a)))
                .collect::<Vec<_>>()
        };
        let index_only = pick(&[Action::IndexInstead]);
        let mut act = pick(&[Action::Buy, Action::Sell, Action::Trim]);
        // Tested calls first, then the jumpiest holdings (the biggest trims).
        act.sort_by(|a, b| {
            (a.action == Some(Action::Trim))
                .cmp(&(b.action == Some(Action::Trim)))
                .then(
                    b.vol_ratio
                        .unwrap_or(0.0)
                        .total_cmp(&a.vol_ratio.unwrap_or(0.0)),
                )
        });
        let mut sections: Vec<(String, Vec<&api::TodayItem>)> = vec![
            ("Act today: tested calls and holdings to trim".into(), act),
            ("New money for 3+ years".into(), pick(&[Action::Sip])),
        ];
        sections.retain(|(_, rows)| !rows.is_empty());
        let mut list = v_flex()
            .max_w(px(1100.))
            .child(header)
            .child(self.brief_card(cx));
        if !index_only.is_empty() {
            list = list.child(div().mt_4().p_3().rounded_md().bg(cx.theme().muted).child(format!(
                "{} single stocks are calm and say the same thing: for new money, their market's index fund (NIFTY 50 or S&P 500) has the same expected growth with less risk; if you own them, keep them.",
                index_only.len()
            )));
        }
        for group in GROUPS {
            let rows: Vec<_> = ok
                .iter()
                .copied()
                .filter(|i| i.group == group && i.action != Some(Action::IndexInstead))
                .collect();
            sections.push((format!("Everything else: {group}"), rows));
        }
        for (k, (title, rows)) in sections.into_iter().enumerate() {
            if rows.is_empty() {
                continue;
            }
            list = list.child(
                div()
                    .mt_6()
                    .mb_1()
                    .font_weight(FontWeight::BOLD)
                    .child(title),
            );
            for item in rows {
                let symbol = item.symbol.clone();
                let id = SharedString::from(format!("row-{k}-{symbol}"));
                let price = item.last_price.map(fmt::price).unwrap_or_default();
                let change = item
                    .change_pct
                    .map(|c| fmt::pct(c / 100.0))
                    .unwrap_or_default();
                let mut row = h_flex()
                    .id(id)
                    .gap_3()
                    .py_1p5()
                    .px_2()
                    .items_center()
                    .border_b_1()
                    .border_color(border)
                    .hover(move |s| s.bg(hover))
                    .cursor_pointer()
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(Open(symbol.clone()))))
                    .child(
                        div()
                            .w(px(200.))
                            .truncate()
                            .child(item.name.clone().unwrap_or_else(|| item.symbol.clone())),
                    )
                    .child(div().w(px(110.)).text_right().child(price))
                    .child(
                        div()
                            .w(px(70.))
                            .text_right()
                            .text_sm()
                            .text_color(muted)
                            .child(change),
                    );
                if let Some(a) = item.action {
                    row = row.child(div().w(px(90.)).child(crate::action_chip(a, cx)));
                }
                row = row.child(
                    div()
                        .flex_1()
                        .text_sm()
                        .truncate()
                        .child(item.headline.clone().unwrap_or_default()),
                );
                list = list.child(row);
            }
        }
        list
    }
}

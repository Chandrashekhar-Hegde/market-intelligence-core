//! The news desk: what today's headlines lean towards, and who read them.
use gpui_kit::component::{ActiveTheme, h_flex, v_flex};
use gpui_kit::{
    Context, FontWeight, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Task,
    Window, div, px,
};

use crate::api;

pub struct NewsView {
    desk: Option<Result<api::Desk, String>>,
    _task: Task<()>,
}

impl NewsView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let task = cx.spawn(async move |this, cx| {
            let desk = cx.background_executor().spawn(async { api::desk() }).await;
            this.update(cx, |v, cx| {
                v.desk = Some(desk);
                cx.notify();
            })
            .ok();
        });
        Self {
            desk: None,
            _task: task,
        }
    }
}

fn arrow(d: i32) -> &'static str {
    match d.signum() {
        1 => "↑",
        -1 => "↓",
        _ => "→",
    }
}

impl Render for NewsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (muted, blue, border) = (t.muted_foreground, t.blue, t.border);
        let header = div()
            .text_2xl()
            .font_weight(FontWeight::BOLD)
            .child("News desk");
        let desk = match &self.desk {
            None => {
                return v_flex()
                    .child(header)
                    .child(div().mt_6().child("Reading today's headlines…"));
            }
            Some(Err(e)) => {
                return v_flex()
                    .child(header)
                    .child(div().mt_6().child(SharedString::from(e.clone())));
            }
            Some(Ok(d)) => d.clone(),
        };
        crate::ready(window, "news");
        let reader = if desk.reader.starts_with("built-in") {
            "Read by the built-in reader".to_string()
        } else {
            format!("Read by {}", desk.reader)
        };
        let leaning: Vec<_> = desk.leans.iter().filter(|l| l.lean != 0).collect();
        v_flex()
            .max_w(px(1100.))
            .child(h_flex().gap_3().items_center().child(header).child(div().px_2().py_0p5().rounded_md().bg(blue).text_color(gpui_kit::white()).text_xs().child(reader)))
            .child(div().mt_4().font_weight(FontWeight::BOLD).child("Where today's news leans"))
            .child(if leaning.is_empty() {
                div().text_sm().text_color(muted).child("No clear lean on any market today.")
            } else {
                div().child(h_flex().flex_wrap().gap_3().children(leaning.iter().map(|l| div().child(format!("{} {}", arrow(l.lean), l.label)))))
            })
            .child(div().text_sm().text_color(muted).child(
                "Tested on 3 years of headlines: news moves prices on the day it comes out, not after, so it explains the day but doesn't make calls.",
            ))
            .child(div().mt_4().font_weight(FontWeight::BOLD).child(format!("{} headlines", desk.stories.len())))
            .children(desk.stories.iter().take(60).map(|s| {
                let effects = s.impacts.iter().map(|i| format!("{} {}", arrow(i.direction), i.asset)).collect::<Vec<_>>().join("  ");
                v_flex()
                    .py_1p5()
                    .border_b_1()
                    .border_color(border)
                    .child(div().child(s.title.clone()))
                    .child(div().text_xs().text_color(muted).child(format!("{} · {}{}", s.publisher, s.theme, if effects.is_empty() { String::new() } else { format!(" · {effects}") })))
            }))
    }
}

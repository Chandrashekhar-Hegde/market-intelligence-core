//! Market Intelligence, native: Today's actions, one instrument in plain words, the news desk.
//! Talks to the same API as the web app; every word on screen comes from `gm_domain`.
mod api;
mod fmt;
mod instrument;
mod mix;
mod news;
mod today;

use gpui_kit::component::{
    ActiveTheme, Selectable as _,
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::{
    AppContext as _, Bounds, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window, WindowBounds,
    WindowOptions, div, px, size,
};

use gm_domain::analysis::plain::Action;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Today,
    Instrument,
    Mix,
    News,
}

struct Shell {
    screen: Screen,
    today: Entity<today::TodayView>,
    instrument: Entity<instrument::InstrumentView>,
    mix: Entity<mix::MixView>,
    news: Entity<news::NewsView>,
}

impl Shell {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let today = cx.new(today::TodayView::new);
        let instrument = cx.new(|cx| instrument::InstrumentView::new(window, cx));
        let mix = cx.new(|cx| mix::MixView::new(window, cx));
        let news = cx.new(news::NewsView::new);
        cx.subscribe(&today, |this, _, open: &today::Open, cx| {
            this.screen = Screen::Instrument;
            let symbol = open.0.clone();
            this.instrument.update(cx, |view, cx| view.load(symbol, cx));
            cx.notify();
        })
        .detach();
        let screen = match std::env::var("GM_DESKTOP_SCREEN").as_deref() {
            Ok("instrument") => Screen::Instrument,
            Ok("news") => Screen::News,
            Ok("mix") => Screen::Mix,
            _ => Screen::Today,
        };
        Self {
            screen,
            today,
            instrument,
            mix,
            news,
        }
    }

    fn nav(
        &self,
        id: &'static str,
        label: &'static str,
        screen: Screen,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .label(label)
            .ghost()
            .selected(self.screen == screen)
            .w_full()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.screen = screen;
                cx.notify();
            }))
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, sidebar, border) = (
            theme.background,
            theme.foreground,
            theme.sidebar,
            theme.sidebar_border,
        );
        h_flex()
            .size_full()
            .bg(bg)
            .text_color(fg)
            .child(
                v_flex()
                    .w(px(190.))
                    .h_full()
                    .p_3()
                    .gap_1()
                    .bg(sidebar)
                    .border_r_1()
                    .border_color(border)
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Internal"),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .mb_3()
                            .child("Market Intelligence"),
                    )
                    .child(self.nav("nav-today", "Today", Screen::Today, cx))
                    .child(self.nav("nav-instrument", "Instrument", Screen::Instrument, cx))
                    .child(self.nav("nav-mix", "Your mix", Screen::Mix, cx))
                    .child(self.nav("nav-news", "News desk", Screen::News, cx)),
            )
            .child(
                div()
                    .id("content")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .p_6()
                    .child(match self.screen {
                        Screen::Today => self.today.clone().into_any_element(),
                        Screen::Instrument => self.instrument.clone().into_any_element(),
                        Screen::Mix => self.mix.clone().into_any_element(),
                        Screen::News => self.news.clone().into_any_element(),
                    }),
            )
    }
}

/// A coloured label for an action.
pub fn action_chip(action: Action, cx: &gpui_kit::App) -> gpui_kit::Div {
    let t = cx.theme();
    let (bg, label) = match action {
        Action::Buy => (t.green, "Buy"),
        Action::Sip => (t.green, "SIP"),
        Action::Keep => (t.green, "Keep"),
        Action::Sell => (t.red, "Sell"),
        Action::Trim => (t.yellow, "Trim"),
        Action::IndexInstead => (t.blue, "Index fund"),
        Action::Fd => (t.muted_foreground, "FD as good"),
        Action::Watch => (t.muted_foreground, "Watch"),
    };
    div()
        .px_2()
        .py_0p5()
        .rounded_md()
        .bg(bg)
        .text_color(gpui_kit::white())
        .text_xs()
        .font_weight(FontWeight::BOLD)
        .child(label)
}

/// Screenshot mode: once the chosen screen has its data, render it to a PNG and quit. GPUI draws
/// only when something changed, so a timer forces the redraw that the capture reads.
#[cfg(feature = "screenshot")]
pub fn ready(window: &mut Window, cx: &mut gpui_kit::App, screen: &str) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    static DONE: AtomicBool = AtomicBool::new(false);
    let wanted = std::env::var("GM_DESKTOP_SCREEN").unwrap_or_else(|_| "today".into());
    let Ok(path) = std::env::var("GM_DESKTOP_SCREENSHOT") else {
        return;
    };
    if wanted != screen || DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(400))
            .await;
        cx.update(|cx| handle.update(cx, |_, window, _| window.refresh()))
            .ok();
        cx.background_executor()
            .timer(Duration::from_millis(400))
            .await;
        cx.update(|cx| {
            handle
                .update(cx, |_, window, _| match window.render_to_image() {
                    Ok(img) => img
                        .save(&path)
                        .map_or_else(|e| eprintln!("save: {e}"), |_| eprintln!("saved {path}")),
                    Err(e) => eprintln!("render_to_image: {e}"),
                })
                .ok();
            cx.quit();
        });
    })
    .detach();
}

#[cfg(not(feature = "screenshot"))]
pub fn ready(_: &mut Window, _: &mut gpui_kit::App, _: &str) {}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            let height = std::env::var("GM_DESKTOP_HEIGHT")
                .ok()
                .and_then(|h| h.parse().ok())
                .unwrap_or(900.0);
            let bounds = Bounds::centered(None, size(px(1360.), px(height)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| Shell::new(window, cx))
            })
            .expect("failed to open window");
            // Come to the front: a window behind others gets no redraws on macOS.
            cx.activate(true);
        });
}

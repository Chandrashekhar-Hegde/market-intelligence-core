//! Spike: one window with NIFTY 50's candles and its analysis, from the same API the web app uses.
use gpui_kit::component::{ActiveTheme, chart::CandlestickChart, h_flex, v_flex};
use gpui_kit::{
    AppContext as _, Context, FontWeight, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, WindowOptions, div, px,
};
use serde::Deserialize;

const DEFAULT_API: &str = "https://gm-api-production-911a7e90.up.railway.app";

#[derive(Clone, Deserialize)]
struct Bar {
    date: String,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
}

struct Loaded {
    symbol: String,
    bars: Vec<Bar>,
    findings: Vec<String>,
}

fn get(path: &str) -> Result<serde_json::Value, String> {
    let base = std::env::var("GM_API_URL").unwrap_or_else(|_| DEFAULT_API.into());
    let mut req = ureq::get(format!("{base}{path}"));
    if let Ok(token) = std::env::var("GM_ACCESS_TOKEN") {
        req = req.header("Authorization", format!("Bearer {token}"));
    }
    req.call()
        .map_err(|e| e.to_string())?
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())
}

fn load(symbol: &str) -> Result<Loaded, String> {
    let s = symbol.replace('^', "%5E");
    let bars = get(&format!("/api/v1/bars?symbol={s}&range={}", std::env::var("GM_RANGE").unwrap_or_else(|_| "1y".into())))?;
    let bars: Vec<Bar> = serde_json::from_value(bars["chart"]["bars"].clone()).map_err(|e| e.to_string())?;
    let analysis = get(&format!("/api/v1/analyze?symbol={s}&range=10y"))?;
    let findings = serde_json::from_value(analysis["report"]["findings"].clone()).unwrap_or_default();
    Ok(Loaded { symbol: symbol.into(), bars, findings })
}

struct Spike {
    state: Option<Result<Loaded, String>>,
}

impl Spike {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let loaded = cx.background_executor().spawn(async { load("^NSEI") }).await;
            this.update(cx, |view, cx| {
                view.state = Some(loaded);
                cx.notify();
            })
            .ok();
        })
        .detach();
        Self { state: None }
    }
}

impl Render for Spike {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.state {
            None => div().child("Loading NIFTY 50…").into_any_element(),
            Some(Err(e)) => div().child(SharedString::from(format!("Could not load: {e}"))).into_any_element(),
            Some(Ok(data)) => {
                #[cfg(feature = "screenshot")]
                screenshot_once(window);
                let last = data.bars.last().map(|b| b.close).unwrap_or_default();
                v_flex()
                    .gap_3()
                    .size_full()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(div().text_xl().font_weight(FontWeight::BOLD).child(data.symbol.clone()))
                            .child(div().text_xl().child(format!("{last:.2}"))),
                    )
                    .child(
                        div().h(px(360.)).child(
                            CandlestickChart::new(data.bars.clone())
                                .x(|b| b.date.clone())
                                .open(|b| b.open)
                                .high(|b| b.high)
                                .low(|b| b.low)
                                .close(|b| b.close)
                                .tick_margin((data.bars.len() / 6).max(1))
                                .tooltip_value(|_, _, v| format!("{v:.2}").into())
                                .appear(false)
                        ),
                    )
                    .child(div().font_weight(FontWeight::BOLD).child("What the data says"))
                    .children(data.findings.iter().take(6).map(|f| div().text_sm().child(f.clone())))
                    .into_any_element()
            }
        };
        let _ = window;
        div()
            .size_full()
            .p_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(body)
    }
}

#[cfg(feature = "screenshot")]
fn screenshot_once(window: &mut Window) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static DONE: AtomicBool = AtomicBool::new(false);
    let Ok(path) = std::env::var("GM_DESKTOP_SCREENSHOT") else { return };
    if DONE.swap(true, Ordering::SeqCst) {
        return;
    }
    // Two frames on: the chart's draw-in animation has settled by then.
    window.on_next_frame(move |window, _| {
        window.on_next_frame(move |window, cx| {
            match window.render_to_image() {
                Ok(img) => img.save(&path).map_or_else(|e| eprintln!("save: {e}"), |_| eprintln!("saved {path}")),
                Err(e) => eprintln!("render_to_image: {e}"),
            }
            cx.quit();
        });
    });
}

fn main() {
    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(|cx| {
        gpui_kit::init(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| cx.new(Spike::new))
            .expect("failed to open window");
    });
}

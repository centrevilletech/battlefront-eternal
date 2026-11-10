//! Page server-browser presentation. Discovery, filters and joins use the
//! existing ServerBrowserService and ShellApp; no network state lives here.
use super::Page;
use bfe_online::browser::{
    BrowserState, Filters, GameInfo, ServerBrowserService, SortSpec, Source,
};

#[derive(Default)]
pub struct BrowserView {
    pub source: Option<Source>,
    pub filters: Filters,
    pub sort: SortSpec,
    pub state: BrowserState,
    pub selected: Option<String>,
    pub rows: Vec<[String; 7]>,
    pub info: Option<GameInfo>,
    pub focus: usize,
}
impl BrowserView {
    pub fn sync(&mut self, browser: &dyn ServerBrowserService) {
        self.source = Some(browser.source());
        self.filters = browser.filters().clone();
        self.sort = browser.sort_spec();
        self.state = browser.state().clone();
        let selected = browser.selected();
        self.selected = selected.map(|s| s.id.clone());
        self.info = selected.map(|s| browser.describe(s, &[]));
        self.rows = browser
            .rows()
            .into_iter()
            .map(|s| {
                let b = s.battle.as_ref();
                [
                    s.name.clone(),
                    format!("{}/{}", s.players, s.max_players),
                    b.map(|v| {
                        if v.map_name.is_empty() {
                            v.map.clone()
                        } else {
                            v.map_name.clone()
                        }
                    })
                    .unwrap_or_default(),
                    b.map(|v| v.mode.to_uppercase()).unwrap_or_default(),
                    b.map(|v| {
                        if v.era == "cw" {
                            "CLONE WARS"
                        } else {
                            "GALACTIC CIVIL WAR"
                        }
                        .into()
                    })
                    .unwrap_or_default(),
                    s.latency_ms
                        .map(|v| format!("{v} MS"))
                        .unwrap_or_else(|| "\u{2014}".into()),
                    s.location.clone(),
                ]
            })
            .collect();
    }
}
impl Page {
    pub(super) fn text_width(&self, text: &str, size: f64, spacing: f64) -> f64 {
        text.encode_utf16()
            .map(|ch| {
                self.body
                    .glyphs
                    .get(&ch)
                    .map(|g| g.advance * size / self.body.height + spacing)
                    .unwrap_or(0.)
            })
            .sum::<f64>()
            - spacing
    }
}

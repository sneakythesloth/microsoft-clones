use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, vec2, Color32, Context, Id, Pos2, Rect, RichText, Sense, Ui};

use crate::dialogs::Dialogs;
use crate::fonts::FontStore;
use crate::layout::*;
use crate::model::*;
use crate::render::{self, TexCache, View};

// ---------------------------------------------------------------- small types

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Sel {
    pub anchor: Pos,
    pub head: Pos,
}

impl Sel {
    pub fn caret(p: Pos) -> Sel {
        Sel { anchor: p, head: p }
    }
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
    pub fn ordered(&self) -> (Pos, Pos) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

pub struct Snapshot {
    pub doc: Document,
    pub sel: Sel,
    pub kind: StoryKind,
}

#[derive(Default)]
pub struct History {
    pub undo: Vec<Snapshot>,
    pub redo: Vec<Snapshot>,
    pub last: Option<((u8, usize), Instant)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Home,
    Insert,
    Layout,
    Review,
    View,
    Table,
    Picture,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Units {
    Inch,
    Cm,
    Pt,
}

impl Units {
    pub fn from_pt(&self, pt: f32) -> f32 {
        match self {
            Units::Inch => pt / 72.0,
            Units::Cm => pt / 72.0 * 2.54,
            Units::Pt => pt,
        }
    }
    pub fn to_pt(&self, v: f32) -> f32 {
        match self {
            Units::Inch => v * 72.0,
            Units::Cm => v / 2.54 * 72.0,
            Units::Pt => v,
        }
    }
    pub fn suffix(&self) -> &'static str {
        match self {
            Units::Inch => " in",
            Units::Cm => " cm",
            Units::Pt => " pt",
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Units::Inch => "Inches",
            Units::Cm => "Centimeters",
            Units::Pt => "Points",
        }
    }
}

pub struct Settings {
    pub units: Units,
    pub smart_quotes: bool,
    pub auto_download: bool,
    pub api_key: String,
    pub recent: Vec<String>,
    pub dark_ui: bool,
}

fn config_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("wordrs").join("config.json")
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings { units: Units::Inch, smart_quotes: true, auto_download: true, api_key: String::new(), recent: vec![], dark_ui: false };
        if let Ok(data) = std::fs::read(config_path()) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data) {
                s.units = match v.get("units").and_then(|u| u.as_str()) {
                    Some("cm") => Units::Cm,
                    Some("pt") => Units::Pt,
                    _ => Units::Inch,
                };
                s.smart_quotes = v.get("smart_quotes").and_then(|b| b.as_bool()).unwrap_or(true);
                s.auto_download = v.get("auto_download").and_then(|b| b.as_bool()).unwrap_or(true);
                s.dark_ui = v.get("dark_ui").and_then(|b| b.as_bool()).unwrap_or(false);
                s.api_key = v.get("api_key").and_then(|b| b.as_str()).unwrap_or("").to_string();
                s.recent = v.get("recent").and_then(|r| r.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            }
        } else if let Some(loc) = sys_locale_metric() {
            if loc {
                s.units = Units::Cm;
            }
        }
        s
    }
    pub fn save(&self) {
        let v = serde_json::json!({
            "units": match self.units { Units::Inch => "in", Units::Cm => "cm", Units::Pt => "pt" },
            "smart_quotes": self.smart_quotes,
            "auto_download": self.auto_download,
            "dark_ui": self.dark_ui,
            "api_key": self.api_key,
            "recent": self.recent,
        });
        let p = config_path();
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::write(p, serde_json::to_vec_pretty(&v).unwrap_or_default());
    }
    pub fn add_recent(&mut self, p: &std::path::Path) {
        let s = p.to_string_lossy().to_string();
        self.recent.retain(|r| *r != s);
        self.recent.insert(0, s);
        self.recent.truncate(10);
        self.save();
    }
}

/// Very rough: most of the world except the US/Liberia/Myanmar uses metric.
fn sys_locale_metric() -> Option<bool> {
    let l = std::env::var("LC_MEASUREMENT").or_else(|_| std::env::var("LANG")).ok()?;
    let l = l.to_lowercase();
    Some(!(l.starts_with("en_us") || l.starts_with("my_") || l.starts_with("en_lr") || l.is_empty()) && l.contains('_') && !l.starts_with("c.") && l != "c")
}

#[derive(PartialEq, Clone, Copy)]
pub struct PagKey {
    vers: [u64; 5],
    styles: u64,
    fonts: u64,
    page: [u32; 9],
}

pub struct ImgDrag {
    pub para: usize,
    pub off: usize,
    pub w: f32,
    pub h: f32,
    pub start: Pos2,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ZoomMode {
    PageWidth,
    WholePage,
}

pub enum Confirm {
    Quit,
    New,
    Open(Option<PathBuf>),
}

// ---------------------------------------------------------------- the app

pub struct App {
    pub doc: Document,
    pub kind: StoryKind,
    pub sel: Sel,
    pub pending: CharFmt,
    pub hist: History,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    pub fonts: FontStore,
    pub engine: LayoutEngine,
    pub sc: StyleCache,
    pub pag: Option<Pagination>,
    pub pag_key: Option<PagKey>,
    pub tex: TexCache,
    pub zoom: f32,
    pub zoom_mode: Option<ZoomMode>,
    pub show_marks: bool,
    pub show_nav: bool,
    pub show_ruler: bool,
    pub tab: Tab,
    pub scroll_to_caret: bool,
    pub goal_x: Option<f32>,
    pub active_page: usize,
    pub dragging: bool,
    pub img_drag: Option<ImgDrag>,
    pub clip: Option<(String, Vec<Paragraph>)>,
    pub format_painter: Option<CharFmt>,
    pub status: Option<(String, Instant)>,
    pub settings: Settings,
    pub dlg: Dialogs,
    pub next_list_id: u32,
    pub text_focus: bool,
    pub editor_id: Id,
    pub page_left_px: f32,
    pub view_scale: f32,
    pub view_w: f32,
    pub view_h: f32,
    pub last_title: String,
    pub counts: (u64, usize, usize),
    pub nav: (u64, Vec<(usize, u8, String)>),
    pub confirm: Option<Confirm>,
    pub allow_close: bool,
    pub ruler_drag_ck: bool,
    pub last_color: Rgb,
    pub last_highlight: Rgb,
    pub font_filter: String,
    pub screenshot_path: Option<PathBuf>,
    pub frame_no: u64,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> App {
        let settings = Settings::load();
        let mut fonts = FontStore::new();
        fonts.attach_ctx(&cc.egui_ctx);
        fonts.auto_download = settings.auto_download;
        if fonts.google.api_key.is_empty() {
            fonts.google.api_key = settings.api_key.clone();
        }
        let doc = Document::new();
        for (b, i) in [(false, false), (true, false), (false, true), (true, true)] {
            fonts.resolve("Calibri", b, i);
        }
        let ui_face = fonts.resolve("Carlito", false, false).face;
        fonts.use_as_ui_font(ui_face);
        fonts.install_now(&cc.egui_ctx);
        // refresh the Google Fonts catalog if the cache is missing or old
        let stale = std::fs::metadata(crate::google::catalog_cache_path()).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|e| e > Duration::from_secs(14 * 86400)).unwrap_or(true);
        if stale && std::env::var("WORDRS_OFFLINE").is_err() {
            fonts.refresh_catalog();
        }
        let sc = StyleCache::build(&doc.styles, doc.styles_ver);
        let mut app = App {
            doc,
            kind: StoryKind::Body,
            sel: Sel::default(),
            pending: CharFmt::default(),
            hist: History::default(),
            path: None,
            dirty: false,
            fonts,
            engine: LayoutEngine::default(),
            sc,
            pag: None,
            pag_key: None,
            tex: TexCache::default(),
            zoom: 100.0,
            zoom_mode: None,
            show_marks: false,
            show_nav: false,
            show_ruler: true,
            tab: Tab::Home,
            scroll_to_caret: false,
            goal_x: None,
            active_page: 0,
            dragging: false,
            img_drag: None,
            clip: None,
            format_painter: None,
            status: None,
            settings,
            dlg: Dialogs::default(),
            next_list_id: 1,
            text_focus: false,
            editor_id: Id::new("wordrs-editor"),
            page_left_px: 0.0,
            view_scale: 96.0 / 72.0,
            view_w: 800.0,
            view_h: 600.0,
            last_title: String::new(),
            counts: (0, 0, 0),
            nav: (0, vec![]),
            confirm: None,
            allow_close: false,
            ruler_drag_ck: false,
            last_color: [0xC0, 0x00, 0x00],
            last_highlight: [0xFF, 0xFF, 0x00],
            font_filter: String::new(),
            screenshot_path: std::env::var("WORDRS_SCREENSHOT").ok().map(PathBuf::from),
            frame_no: 0,
        };
        app.apply_theme(&cc.egui_ctx);
        if let Some(p) = initial {
            app.open_path(p);
        }
        app
    }

    pub fn apply_theme(&self, ctx: &Context) {
        let mut v = if self.settings.dark_ui { egui::Visuals::dark() } else { egui::Visuals::light() };
        if !self.settings.dark_ui {
            v.panel_fill = Color32::from_rgb(243, 243, 243);
            v.widgets.noninteractive.bg_fill = Color32::from_rgb(243, 243, 243);
            v.selection.bg_fill = Color32::from_rgb(190, 215, 245);
        }
        ctx.set_visuals(v);
        ctx.style_mut(|s| {
            s.spacing.item_spacing = vec2(4.0, 3.0);
            s.spacing.button_padding = vec2(5.0, 2.0);
            s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(11.5));
        });
    }

    // ------------------------------------------------------------ small accessors

    pub fn story(&self) -> &crate::story::Story {
        self.doc.story(self.kind)
    }

    pub fn story_mut(&mut self) -> &mut crate::story::Story {
        let k = self.kind;
        self.doc.story_mut(k)
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), Instant::now()));
    }

    pub fn note_focus(&mut self, r: &egui::Response) {
        if r.has_focus() {
            self.text_focus = true;
        }
    }

    pub fn title(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Document1".into());
        format!("{}{} - Word RS", name, if self.dirty { " •" } else { "" })
    }

    // ------------------------------------------------------------ layout

    fn pag_key_now(&self) -> PagKey {
        let p = &self.doc.page;
        PagKey {
            vers: [self.doc.body.ver, self.doc.header.ver, self.doc.footer.ver, self.doc.first_header.ver, self.doc.first_footer.ver],
            styles: self.doc.styles_ver,
            fonts: self.fonts.generation,
            page: [p.w.to_bits(), p.h.to_bits(), p.top.to_bits(), p.bottom.to_bits(), p.left.to_bits(), p.right.to_bits(), p.header_dist.to_bits(), p.footer_dist.to_bits(), p.diff_first as u32],
        }
    }

    pub fn ensure_layout(&mut self) {
        if self.sc.ver != self.doc.styles_ver {
            self.sc = StyleCache::build(&self.doc.styles, self.doc.styles_ver);
        }
        let key = self.pag_key_now();
        if self.pag.is_none() || self.pag_key != Some(key) {
            self.pag = Some(self.engine.paginate(&mut self.fonts, &self.doc, &self.sc));
            // faces may have been loaded lazily during layout
            self.pag_key = Some(self.pag_key_now());
        }
    }

    pub fn total_pages(&self) -> usize {
        self.pag.as_ref().map(|p| p.pages.len()).unwrap_or(1)
    }

    pub fn counts(&mut self) -> (usize, usize) {
        if self.counts.0 != self.doc.body.ver {
            let st = &self.doc.body;
            let mut words = 0;
            let mut chars = 0;
            for i in 0..st.len() {
                let t = st.para(i).plain();
                words += t.split_whitespace().count();
                chars += t.chars().count();
            }
            self.counts = (self.doc.body.ver, words, chars);
        }
        (self.counts.1, self.counts.2)
    }

    pub fn zoom_to(&mut self, mode: ZoomMode) {
        self.zoom_mode = Some(mode);
    }

    // ------------------------------------------------------------ document view

    fn page_rects(&self, pag: &Pagination, rect: Rect, content_w: f32, scale: f32, gap: f32) -> Vec<Rect> {
        let mut rects = Vec::with_capacity(pag.pages.len());
        let mut y = rect.top() + gap;
        for p in &pag.pages {
            let (pw, ph) = (p.setup.w * scale, p.setup.h * scale);
            let x = rect.left() + (content_w - pw) / 2.0;
            rects.push(Rect::from_min_size(Pos2::new(x, y), vec2(pw, ph)));
            y += ph + gap;
        }
        rects
    }

    fn doc_ui(&mut self, ui: &mut Ui, ctx: &Context) {
        self.ensure_layout();
        let view_w = ui.available_width();
        self.view_w = view_w;
        self.view_h = ui.available_height();
        // zoom modes and ctrl+wheel
        if let Some(m) = self.zoom_mode.take() {
            let page = self.doc.page;
            let k = 96.0 / 72.0;
            self.zoom = match m {
                ZoomMode::PageWidth => ((view_w - 70.0) / (page.w * k) * 100.0).clamp(25.0, 400.0),
                ZoomMode::WholePage => ((ui.available_height() - 40.0) / (page.h * k) * 100.0).min((view_w - 70.0) / (page.w * k) * 100.0).clamp(25.0, 400.0),
            };
        }
        let zd = ctx.input(|i| i.zoom_delta());
        if (zd - 1.0).abs() > 0.001 && ui.rect_contains_pointer(ui.max_rect()) {
            self.zoom = (self.zoom * zd).clamp(25.0, 400.0);
        }
        self.zoom = (self.zoom / 1.0).round().clamp(25.0, 400.0);
        let scale = self.zoom / 100.0 * 96.0 / 72.0;
        self.view_scale = scale;

        // keyboard
        let focused = ctx.memory(|m| m.has_focus(self.editor_id));
        if focused && !self.text_focus {
            self.handle_events(ctx);
        }
        self.handle_dropped_files(ctx);
        self.ensure_layout();

        let (gap, margin) = (20.0, 28.0);
        let pag_ref = self.pag.as_ref().unwrap();
        let max_pw = pag_ref.pages.iter().map(|p| p.setup.w).fold(0.0, f32::max) * scale;
        let content_w = (max_pw + 2.0 * margin).max(view_w);
        let total_h: f32 = pag_ref.pages.iter().map(|p| p.setup.h * scale + gap).sum::<f32>() + gap;

        egui::ScrollArea::both().id_salt("doc_scroll").auto_shrink([false, false]).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(content_w, total_h), Sense::hover());
            let resp = ui.interact(rect.intersect(ui.clip_rect()), self.editor_id, Sense::click_and_drag());
            let rects = self.page_rects(self.pag.as_ref().unwrap(), rect, content_w, scale, gap);
            self.page_left_px = rects.first().map(|r| r.left()).unwrap_or(0.0);
            self.pointer_input(ctx, &resp, &rects, scale, ui.clip_rect());
            if resp.hovered() {
                ctx.set_cursor_icon(egui::CursorIcon::Text);
            }
            self.ensure_layout();
            let rects = self.page_rects(self.pag.as_ref().unwrap(), rect, content_w, scale, gap);
            let pag = self.pag.take().unwrap();
            self.paint_pages(ui, ctx, &pag, &rects, scale, focused);
            self.pag = Some(pag);
        });
        // keep keyboard focus on the editor unless a text field wants it
        ctx.memory_mut(|m| {
            m.set_focus_lock_filter(self.editor_id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false });
        });
    }

    fn pointer_input(&mut self, ctx: &Context, resp: &egui::Response, rects: &[Rect], scale: f32, viewport: Rect) {
        let (pressed, down, hover, mods, clicks) = ctx.input(|i| {
            let clicks = if i.pointer.button_triple_clicked(egui::PointerButton::Primary) {
                3
            } else if i.pointer.button_double_clicked(egui::PointerButton::Primary) {
                2
            } else {
                1
            };
            (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.latest_pos(), i.modifiers, clicks)
        });
        if !down {
            self.dragging = false;
            self.img_drag = None;
        }
        let Some(p) = hover else { return };
        // map the pointer to (page, x_pt, y_pt)
        let locate = |p: Pos2| -> Option<(usize, f32, f32)> {
            if rects.is_empty() {
                return None;
            }
            let mut best = 0;
            let mut bd = f32::MAX;
            for (i, r) in rects.iter().enumerate() {
                let d = if p.y < r.top() { r.top() - p.y } else if p.y > r.bottom() { p.y - r.bottom() } else { 0.0 };
                if d < bd {
                    bd = d;
                    best = i;
                }
            }
            let r = rects[best];
            Some((best, (p.x - r.left()) / scale, (p.y - r.top()) / scale))
        };
        // image resize drag
        if let Some(d) = &self.img_drag {
            if down {
                let (para, off, w0, h0, start) = (d.para, d.off, d.w, d.h, d.start);
                let dx = (p.x - start.x) / scale;
                let nw = (w0 + dx).max(12.0);
                let nh = h0 * nw / w0;
                self.set_image_size(para, off, nw, nh);
            }
            return;
        }
        if pressed && resp.hovered() && viewport.contains(p) {
            let Some((page, x, y)) = locate(p) else { return };
            self.on_press(page, x, y, mods, clicks, ctx, p, scale, rects);
        } else if self.dragging && down {
            if let Some((page, x, y)) = locate(p) {
                self.drag_to(page, x, y);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_press(&mut self, page: usize, x: f32, y: f32, mods: egui::Modifiers, clicks: u8, ctx: &Context, ptr: Pos2, scale: f32, rects: &[Rect]) {
        if std::env::var("WORDRS_DEBUG").is_ok() {
        }
        let Some(pag) = self.pag.as_ref() else { return };
        let page_idx = page.min(pag.pages.len() - 1);
        let setup = pag.pages[page_idx].setup;
        let top = pag.pages[page_idx].top;
        let in_header = y < top.min(setup.top);
        let in_footer = y > setup.h - setup.bottom;
        // resize handle of a selected image?
        if let Some((para, off)) = self.selected_image() {
            if self.kind == StoryKind::Body {
                if let Some((r, _)) = render::image_rect(&pag.pages[page_idx].body, para, off) {
                    let hr = Rect::from_center_size(Pos2::new(rects[page_idx].left() + r.right() * scale, rects[page_idx].top() + r.bottom() * scale), vec2(10.0, 10.0));
                    if hr.contains(ptr) {
                        self.checkpoint(None);
                        self.img_drag = Some(ImgDrag { para, off, w: r.width(), h: r.height(), start: ptr });
                        return;
                    }
                }
            }
        }
        // ctrl+click follows links
        if mods.command {
            let lines = if self.kind == StoryKind::Body { &pag.pages[page_idx].body } else { &pag.pages[page_idx].body };
            if let Some(url) = render::link_at(lines, x, y) {
                crate::io::open_url(&url);
                return;
            }
        }
        // switching between body and header/footer
        if self.kind == StoryKind::Body && clicks >= 2 && (in_header || in_footer) {
            let pg = &pag.pages[page_idx];
            self.kind = if in_header { pg.header_kind } else { pg.footer_kind };
            self.active_page = page_idx;
            self.ensure_layout();
            let pag = self.pag.as_ref().unwrap();
            if let Some(pos) = pag.hit(self.kind, page_idx, x, y) {
                self.sel = Sel::caret(pos);
            }
            return;
        }
        if self.kind != StoryKind::Body && !in_header && !in_footer {
            self.kind = StoryKind::Body;
            self.sel = Sel::caret(Pos::default());
        }
        if self.kind != StoryKind::Body {
            self.active_page = page_idx;
        }
        let kind = self.kind;
        let pag = self.pag.as_ref().unwrap();
        // images
        if kind == StoryKind::Body {
            if let Some((para, off)) = render::image_at(&pag.pages[page_idx].body, x, y) {
                self.sel = Sel { anchor: Pos::new(para, off), head: Pos::new(para, off + 1) };
                self.pending = CharFmt::default();
                return;
            }
        }
        let Some(pos) = pag.hit(kind, page_idx, x, y) else { return };
        let pos = self.story().clamp(pos);
        self.pending = CharFmt::default();
        self.goal_x = None;
        match clicks {
            3 => {
                let len = self.story().plen(pos.p);
                self.sel = Sel { anchor: Pos::new(pos.p, 0), head: Pos::new(pos.p, len) };
            }
            2 => {
                let (a, b) = self.story().word_range(pos);
                self.sel = Sel { anchor: a, head: b };
            }
            _ => {
                if mods.shift {
                    self.sel.head = pos;
                } else {
                    self.sel = Sel::caret(pos);
                }
                self.dragging = true;
            }
        }
        // format painter: apply on selection end handled at release; simple: apply immediately on word select
        if self.format_painter.is_some() && clicks == 2 {
            self.apply_format_painter();
        }
        if self.kind != StoryKind::Body {
            self.active_page = page_idx;
        }
        if self.kind == StoryKind::Body {
            self.active_page = page_idx;
        }
        ctx.request_repaint();
    }

    fn drag_to(&mut self, page: usize, x: f32, y: f32) {
        let Some(pag) = self.pag.as_ref() else { return };
        if let Some(pos) = pag.hit(self.kind, page.min(pag.pages.len() - 1), x, y) {
            let pos = self.story().clamp(pos);
            self.sel.head = pos;
            self.pending = CharFmt::default();
        }
    }

    pub fn selected_image(&self) -> Option<(usize, usize)> {
        let (a, b) = self.sel.ordered();
        if a.p == b.p && b.o == a.o + 1 && self.kind == StoryKind::Body {
            let p = self.story().para(a.p);
            let mut acc = 0;
            for r in &p.runs {
                let n = r.text.chars().count();
                if a.o >= acc && a.o < acc + n {
                    return if matches!(r.obj, Some(Obj::Image(_))) { Some((a.p, a.o)) } else { None };
                }
                acc += n;
            }
        }
        None
    }

    fn paint_pages(&mut self, ui: &mut Ui, ctx: &Context, pag: &Pagination, rects: &[Rect], scale: f32, focused: bool) {
        let clip = ui.clip_rect();
        let (a, b) = self.sel.ordered();
        let cell_sel = if !self.sel.is_empty() && self.kind == StoryKind::Body { self.story().cell_range(a.p, b.p).filter(|_| !self.story().same_container(a.p, b.p)) } else { None };
        let sel_color = Color32::from_rgba_unmultiplied(51, 120, 220, 85);
        let find_color = Color32::from_rgba_unmultiplied(255, 220, 0, 130);
        let find_cur = Color32::from_rgba_unmultiplied(255, 140, 0, 170);
        let blink = (ctx.input(|i| i.time) * 2.0) as i64 % 2 == 0;
        let caret = if self.kind == StoryKind::Body || self.active_page < pag.pages.len() { pag.caret_rect(&self.fonts, self.kind, self.sel.head, self.active_page) } else { None };
        let matches = if self.dlg.find_open { self.dlg.find.matches.clone() } else { vec![] };
        let cur_match = self.dlg.find.cur;
        let spell = self.spell_spans_for_view(pag);
        let sel_img = self.selected_image();
        let marks = self.show_marks;
        let kind = self.kind;
        for (i, page) in pag.pages.iter().enumerate() {
            let r = rects[i];
            if !r.intersects(clip) {
                continue;
            }
            let painter = ui.painter().with_clip_rect(r.intersect(clip));
            render::paint_page_frame(&ui.painter().with_clip_rect(clip.expand(30.0)), r);
            let mut view = View { painter: &painter, ctx, fonts: &self.fonts, tex: &mut self.tex, origin: r.min, scale, show_marks: marks };
            render::paint_fills(&view, page);
            if kind == StoryKind::Body {
                for (mi, (ma, mb)) in matches.iter().enumerate() {
                    render::paint_range(&view, &page.body, *ma, *mb, if Some(mi) == cur_match { find_cur } else { find_color });
                }
                if let Some((blk, r0, c0, r1, c1)) = cell_sel {
                    render::paint_cells(&view, page, blk, r0, c0, r1, c1, sel_color);
                } else {
                    render::paint_range(&view, &page.body, a, b, sel_color);
                }
            } else {
                let lines = if matches!(kind, StoryKind::Header | StoryKind::FirstHeader) { &page.header } else { &page.footer };
                if self.active_page == i {
                    render::paint_range(&view, lines, a, b, sel_color);
                }
            }
            render::paint_lines(&mut view, &page.body, kind != StoryKind::Body, page.setup.w, page.setup.left);
            render::paint_squiggles(&view, &page.body, &spell);
            render::paint_rules(&view, page);
            render::paint_lines(&mut view, &page.header, kind == StoryKind::Body, page.setup.w, page.setup.left);
            render::paint_lines(&mut view, &page.footer, kind == StoryKind::Body, page.setup.w, page.setup.left);
            if kind != StoryKind::Body && self.active_page == i {
                // header/footer editing frame
                let (y0, y1) = if matches!(kind, StoryKind::Header | StoryKind::FirstHeader) { (0.0, page.top) } else { (page.top + page.content_h, page.setup.h) };
                let rr = Rect::from_min_max(view.pt(0.0, y0), view.pt(page.setup.w, y1));
                painter.rect_stroke(rr.shrink(1.0), 0.0, egui::Stroke::new(1.0, Color32::from_rgb(70, 130, 200)), egui::StrokeKind::Inside);
                let label = if matches!(kind, StoryKind::Header | StoryKind::FirstHeader) { "Header" } else { "Footer" };
                painter.text(rr.left_top() + vec2(4.0, 2.0), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(11.0), Color32::from_rgb(70, 130, 200));
            }
            // selected image frame + handle
            if let (Some((para, off)), true) = (sel_img, kind == StoryKind::Body) {
                if let Some((ir, _)) = render::image_rect(&page.body, para, off) {
                    let rr = Rect::from_min_max(view.pt(ir.left(), ir.top()), view.pt(ir.right(), ir.bottom()));
                    painter.rect_stroke(rr, 0.0, egui::Stroke::new(1.5, Color32::from_rgb(40, 110, 220)), egui::StrokeKind::Outside);
                    for c in [rr.left_top(), rr.right_top(), rr.left_bottom(), rr.right_bottom()] {
                        painter.rect_filled(Rect::from_center_size(c, vec2(7.0, 7.0)), 0.0, Color32::WHITE);
                        painter.rect_stroke(Rect::from_center_size(c, vec2(7.0, 7.0)), 0.0, egui::Stroke::new(1.0, Color32::from_rgb(40, 110, 220)), egui::StrokeKind::Inside);
                    }
                }
            }
            // caret
            if let Some((cp, cx, cy, ch)) = caret {
                if cp == i && self.sel.is_empty() && focused && blink && !self.text_focus {
                    render::paint_caret(&view, cx, cy, ch, Color32::BLACK);
                }
            }
        }
        // scroll caret into view; report IME position
        if let Some((cp, cx, cy, ch)) = caret {
            if cp < rects.len() {
                let r = rects[cp];
                let caret_rect = Rect::from_min_size(Pos2::new(r.left() + cx * scale, r.top() + cy * scale), vec2(2.0, (ch * scale).max(10.0)));
                if self.scroll_to_caret {
                    ui.scroll_to_rect(caret_rect.expand2(vec2(30.0, 40.0)), None);
                    self.scroll_to_caret = false;
                }
                ctx.output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput { rect: caret_rect, cursor_rect: caret_rect });
                });
            }
        }
        if focused && self.sel.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    // ------------------------------------------------------------ ruler

    fn ruler_ui(&mut self, ui: &mut Ui) {
        let h = 20.0;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(228));
        let Some(pag) = self.pag.as_ref() else { return };
        let page = &pag.pages[self.active_page.min(pag.pages.len() - 1)];
        let setup = page.setup;
        let s = self.view_scale;
        let px0 = self.page_left_px;
        let x_of = |pt: f32| px0 + pt * s;
        let white = Rect::from_min_max(Pos2::new(x_of(setup.left).max(rect.left()), rect.top() + 3.0), Pos2::new(x_of(setup.w - setup.right).min(rect.right()), rect.bottom() - 3.0));
        if white.width() > 0.0 {
            painter.rect_filled(white, 0.0, Color32::WHITE);
        }
        // ticks every 1/8 inch or 1/2 cm measured from the left margin
        let units = self.settings.units;
        let (step, per_label): (f32, i32) = match units {
            Units::Inch => (72.0 / 8.0, 8),
            Units::Cm => (72.0 / 2.54 / 2.0, 2),
            Units::Pt => (72.0 / 8.0, 8),
        };
        let n = ((setup.w) / step) as i32 + 1;
        for i in -(setup.left / step) as i32..n {
            let pt = setup.left + i as f32 * step;
            let x = x_of(pt);
            if x < rect.left() || x > rect.right() || pt < 0.0 || pt > setup.w {
                continue;
            }
            let major = i % per_label == 0;
            let half = per_label % 2 == 0 && i % (per_label / 2) == 0;
            let len = if major { 7.0 } else if half { 5.0 } else { 3.0 };
            let cy = rect.center().y;
            painter.line_segment([Pos2::new(x, cy - len * 0.5), Pos2::new(x, cy + len * 0.5)], egui::Stroke::new(1.0, Color32::from_gray(110)));
            if major && i != 0 {
                let label = format!("{}", (i / per_label).abs());
                painter.text(Pos2::new(x, cy), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(9.0), Color32::from_gray(90));
            }
        }
        // indent markers for the caret paragraph (body only)
        if self.kind != StoryKind::Body {
            return;
        }
        let lines = pag.lines_of(self.sel.head.p);
        let Some(pl) = lines.first() else { return };
        let rp = pl.layout.rp.clone();
        let content_w = setup.content_w();
        let left_abs = rp.left;
        let first_abs = rp.left + rp.first;
        let right_abs = content_w - rp.right;
        let mk = |ui: &mut Ui, id: &str, x: f32, down: bool| -> (egui::Response, f32) {
            let r = Rect::from_center_size(Pos2::new(x, if down { rect.top() + 6.0 } else { rect.bottom() - 6.0 }), vec2(11.0, 11.0));
            let resp = ui.interact(r, Id::new(("ruler", id)), Sense::drag());
            let col = if resp.hovered() || resp.dragged() { Color32::from_rgb(40, 110, 220) } else { Color32::from_gray(70) };
            let pts = if down { vec![Pos2::new(x - 4.5, r.top() + 1.0), Pos2::new(x + 4.5, r.top() + 1.0), Pos2::new(x, r.top() + 7.0)] } else { vec![Pos2::new(x - 4.5, r.bottom() - 1.0), Pos2::new(x + 4.5, r.bottom() - 1.0), Pos2::new(x, r.bottom() - 7.0)] };
            painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
            let px = resp.interact_pointer_pos().map(|p| (p.x - px0) / s - setup.left).unwrap_or(0.0);
            (resp, px)
        };
        let (r_first, x_first) = mk(ui, "first", x_of(setup.left + first_abs), true);
        let (r_left, x_left) = mk(ui, "left", x_of(setup.left + left_abs), false);
        let (r_right, x_right) = mk(ui, "right", x_of(setup.left + right_abs), false);
        let snap = |v: f32| (v / 4.5).round() * 4.5;
        let sel = self.sel;
        for (resp, which) in [(&r_first, 0), (&r_left, 1), (&r_right, 2)] {
            if resp.drag_started() {
                self.checkpoint(None);
            }
            if resp.dragged() {
                let (a, b) = sel.ordered();
                let (px_first, px_left, px_right) = (snap(x_first).max(0.0), snap(x_left).max(0.0), snap(x_right));
                self.story_mut().modify_paras(a.p, b.p, &move |p| match which {
                    0 => p.fmt.first = Some(px_first - p.fmt.left.unwrap_or(left_abs)),
                    1 => {
                        p.fmt.first = Some(first_abs - px_left);
                        p.fmt.left = Some(px_left);
                    }
                    _ => p.fmt.right = Some((content_w - px_right).max(0.0)),
                });
            }
        }
    }

    // ------------------------------------------------------------ nav pane and status bar

    fn nav_ui(&mut self, ui: &mut Ui) {
        ui.heading("Navigation");
        ui.separator();
        if self.nav.0 != self.doc.body.ver {
            let st = &self.doc.body;
            let mut v = vec![];
            for i in 0..st.len() {
                let p = st.para(i);
                if let Some(l) = self.doc.styles.outline_level(&p.style) {
                    let t = p.plain();
                    if !t.trim().is_empty() {
                        v.push((i, l, t));
                    }
                }
            }
            self.nav = (self.doc.body.ver, v);
        }
        if self.nav.1.is_empty() {
            ui.label(RichText::new("Apply a heading style (Home → Styles) and it will appear here.").weak());
        }
        let items = self.nav.1.clone();
        let mut go = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (p, lvl, t) in items {
                ui.horizontal(|ui| {
                    ui.add_space((lvl.saturating_sub(1)) as f32 * 14.0);
                    let txt: String = t.chars().take(40).collect();
                    if ui.link(txt).clicked() {
                        go = Some(p);
                    }
                });
            }
        });
        if let Some(p) = go {
            self.kind = StoryKind::Body;
            self.sel = Sel::caret(Pos::new(p, 0));
            self.scroll_to_caret = true;
        }
    }

    fn status_ui(&mut self, ui: &mut Ui) {
        let (words, chars) = self.counts();
        let page = self.pag.as_ref().map(|p| p.page_of(self.sel.head)).unwrap_or(0);
        let total = self.total_pages();
        ui.horizontal(|ui| {
            ui.label(format!("Page {} of {}", if self.kind == StoryKind::Body { page + 1 } else { self.active_page + 1 }, total));
            ui.separator();
            if ui.link(format!("{} words", words)).clicked() {
                self.dlg.count_open = true;
            }
            ui.label(RichText::new(format!("{} characters", chars)).weak());
            ui.separator();
            if self.kind != StoryKind::Body {
                ui.label(RichText::new("Editing header/footer — double-click the page body to return").color(Color32::from_rgb(40, 100, 200)));
                ui.separator();
            }
            if let Some((m, t)) = &self.status {
                if t.elapsed() < Duration::from_secs(8) {
                    ui.label(m);
                }
            }
            if self.fonts.downloads_pending() {
                ui.spinner();
                ui.label(RichText::new(self.fonts.google.status.clone()).weak());
            } else if !self.fonts.google.status.is_empty() && self.status.is_none() {
                ui.label(RichText::new(self.fonts.google.status.clone()).weak());
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("+").clicked() {
                    self.zoom = (self.zoom + 10.0).min(400.0);
                }
                let mut z = self.zoom;
                if ui.add(egui::Slider::new(&mut z, 25.0..=300.0).show_value(false).step_by(5.0)).changed() {
                    self.zoom = z;
                }
                if ui.small_button("−").clicked() {
                    self.zoom = (self.zoom - 10.0).max(25.0);
                }
                if ui.link(format!("{}%", self.zoom as i32)).clicked() {
                    self.zoom = 100.0;
                }
                ui.separator();
                if ui.selectable_label(self.show_marks, "¶").on_hover_text("Show formatting marks (Ctrl+Shift+8)").clicked() {
                    self.show_marks = !self.show_marks;
                }
            });
        });
    }

    fn confirm_ui(&mut self, ctx: &Context) {
        let Some(c) = &self.confirm else { return };
        let what = match c {
            Confirm::Quit => "closing",
            Confirm::New => "creating a new document",
            Confirm::Open(_) => "opening another document",
        };
        let mut choice: Option<u8> = None;
        egui::Window::new("Word RS").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
            ui.label(format!("Do you want to save changes before {what}?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't Save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        if let Some(ch) = choice {
            let c = self.confirm.take().unwrap();
            if ch == 0 && !self.save() {
                return;
            }
            if ch <= 1 {
                self.dirty = false;
                match c {
                    Confirm::Quit => {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Confirm::New => self.new_doc(),
                    Confirm::Open(Some(p)) => self.open_path(p),
                    Confirm::Open(None) => self.open_dialog(),
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.frame_no += 1;
        self.text_focus = false;
        if self.fonts.poll() {
            self.pag_key = None;
        }
        self.fonts.sync_egui(ctx);
        if self.fonts.downloads_pending() {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
        // window title and close handling
        let t = self.title();
        if t != self.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(t.clone()));
            self.last_title = t;
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
        self.ensure_layout();

        egui::TopBottomPanel::top("ribbon").show(ctx, |ui| self.ribbon_ui(ui, ctx));
        if self.show_ruler {
            egui::TopBottomPanel::top("ruler").show_separator_line(false).frame(egui::Frame::NONE).show(ctx, |ui| self.ruler_ui(ui));
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_ui(ui));
        if self.show_nav {
            egui::SidePanel::left("nav").default_width(230.0).show(ctx, |ui| self.nav_ui(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(if self.settings.dark_ui { Color32::from_gray(60) } else { Color32::from_gray(208) })).show(ctx, |ui| self.doc_ui(ui, ctx));
        self.dialogs_ui(ctx);
        self.confirm_ui(ctx);

        // after the frame: give keyboard focus back to the editor unless a text field is active
        let modal = self.confirm.is_some();
        if !self.text_focus && !modal {
            ctx.memory_mut(|m| m.request_focus(self.editor_id));
        }
        self.fonts.sync_egui(ctx);

        // optional automated screenshot (used for testing)
        if let Some(path) = self.screenshot_path.clone() {
            if self.frame_no == 12 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            let shot = ctx.input(|i| i.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }));
            if let Some(img) = shot {
                let (w, h) = (img.size[0] as u32, img.size[1] as u32);
                let mut buf = image::RgbaImage::new(w, h);
                for (i, px) in img.pixels.iter().enumerate() {
                    buf.put_pixel((i as u32) % w, (i as u32) / w, image::Rgba([px.r(), px.g(), px.b(), px.a()]));
                }
                let _ = buf.save(&path);
                std::process::exit(0);
            }
            ctx.request_repaint();
        }
    }
}

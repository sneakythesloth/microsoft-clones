//! Font management. Fonts come from three places:
//!   * embedded (Carlito: metric-compatible with Calibri),
//!   * the operating system (via fontdb),
//!   * Google Fonts (downloaded on demand into the user's cache directory).
//!
//! Layout measures text with the font tables directly (zoom independent, identical on
//! screen and in PDF export); painting uses egui with one egui font family per face.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{mpsc, Arc};

use eframe::egui;

use crate::google::{self, GFont};

static CARLITO: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/Carlito-Regular.ttf"),
    include_bytes!("../assets/fonts/Carlito-Bold.ttf"),
    include_bytes!("../assets/fonts/Carlito-Italic.ttf"),
    include_bytes!("../assets/fonts/Carlito-BoldItalic.ttf"),
];

pub const DEFAULT_FAMILY: &str = "Carlito";

pub struct Face {
    pub family: String,
    pub data: Arc<Vec<u8>>,
    pub index: u32,
    pub upem: f32,
    pub asc: f32,
    pub desc: f32,
    pub gap: f32,
    pub ul_pos: f32,
    pub ul_thick: f32,
    pub cmap: HashMap<char, u16>,
    pub adv: Vec<u16>,
    pub key: String,
    pub bold: bool,
    pub italic: bool,
    pub glyf: bool,
    pub variable: bool,
    pub space: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub face: usize,
    pub synth_bold: bool,
    pub synth_italic: bool,
}

enum Src {
    Mem(&'static [u8]),
    Path(PathBuf, u32),
    Sys(fontdb::ID),
}

struct StyleSrc {
    bold: bool,
    italic: bool,
    weight: u16,
    src: Src,
}

struct Family {
    name: String,
    srcs: Vec<StyleSrc>,
    loaded: [Option<usize>; 4],
    tried: [bool; 4],
    google: bool,
}

pub enum FontEvent {
    Catalog(Vec<GFont>),
    CatalogErr(String),
    Installed { family: String, note: String },
    Failed { family: String, err: String },
}

pub struct GoogleState {
    pub catalog: Vec<GFont>,
    pub catalog_online: bool,
    pub loading_catalog: bool,
    pub in_flight: HashSet<String>,
    pub installed: HashSet<String>,
    pub status: String,
    pub api_key: String,
}

const ALIASES: &[(&str, &[&str])] = &[
    ("calibri", &["carlito"]),
    ("calibri light", &["carlito"]),
    ("aptos", &["carlito"]),
    ("aptos display", &["carlito"]),
    ("cambria", &["caladea"]),
    ("arial", &["liberation sans", "arimo", "helvetica", "dejavu sans"]),
    ("arial narrow", &["liberation sans narrow", "arimo"]),
    ("helvetica", &["arial", "liberation sans", "arimo"]),
    ("helvetica neue", &["arial", "liberation sans", "arimo"]),
    ("times new roman", &["liberation serif", "tinos", "times", "dejavu serif"]),
    ("times", &["times new roman", "liberation serif", "tinos"]),
    ("courier new", &["liberation mono", "cousine", "courier", "dejavu sans mono"]),
    ("courier", &["courier new", "liberation mono", "cousine"]),
    ("georgia", &["gelasio", "dejavu serif"]),
    ("verdana", &["dejavu sans", "open sans"]),
    ("tahoma", &["dejavu sans", "open sans"]),
    ("trebuchet ms", &["fira sans", "open sans"]),
    ("segoe ui", &["open sans", "noto sans", "dejavu sans"]),
    ("consolas", &["inconsolata", "dejavu sans mono", "liberation mono"]),
    ("lucida console", &["dejavu sans mono", "inconsolata"]),
    ("garamond", &["eb garamond"]),
    ("palatino linotype", &["tinos", "gelasio"]),
    ("book antiqua", &["tinos", "gelasio"]),
    ("century gothic", &["questrial", "didact gothic"]),
    ("impact", &["anton"]),
    ("comic sans ms", &["comic neue"]),
    ("sans-serif", &["carlito"]),
    ("serif", &["tinos", "liberation serif", "dejavu serif"]),
    ("monospace", &["liberation mono", "cousine", "dejavu sans mono"]),
];

fn alias_candidates(lname: &str) -> &'static [&'static str] {
    ALIASES.iter().find(|a| a.0 == lname).map(|a| a.1).unwrap_or(&[])
}

fn parse_face(data: Arc<Vec<u8>>, index: u32, family: &str, key: String) -> Option<Face> {
    let f = ttf_parser::Face::parse(&data, index).ok()?;
    let upem = f.units_per_em() as f32;
    if upem <= 0.0 {
        return None;
    }
    let mut asc = f.ascender() as f32;
    let mut desc = -(f.descender() as f32);
    let mut gap = f.line_gap() as f32;
    if asc == 0.0 && desc == 0.0 {
        asc = f.typographic_ascender().unwrap_or(0) as f32;
        desc = -(f.typographic_descender().unwrap_or(0) as f32);
        gap = f.typographic_line_gap().unwrap_or(0) as f32;
    }
    if asc + desc <= 0.0 {
        asc = upem * 0.8;
        desc = upem * 0.2;
    }
    let (ul_pos, ul_thick) = f.underline_metrics().map(|m| (-(m.position as f32), m.thickness as f32)).unwrap_or((upem * 0.1, upem * 0.05));
    let mut cmap = HashMap::new();
    if let Some(table) = f.tables().cmap {
        for st in table.subtables {
            if !st.is_unicode() {
                continue;
            }
            st.codepoints(|cp| {
                if let (Some(c), Some(g)) = (char::from_u32(cp), st.glyph_index(cp)) {
                    cmap.entry(c).or_insert(g.0);
                }
            });
        }
    }
    let n = f.number_of_glyphs();
    let adv: Vec<u16> = (0..n).map(|g| f.glyph_hor_advance(ttf_parser::GlyphId(g)).unwrap_or(0)).collect();
    let space = cmap.get(&' ').and_then(|g| adv.get(*g as usize)).copied().unwrap_or((upem * 0.3) as u16) as f32 / upem;
    Some(Face {
        family: family.to_string(),
        index,
        upem,
        asc: asc / upem,
        desc: desc / upem,
        gap: gap / upem,
        ul_pos: ul_pos / upem,
        ul_thick: (ul_thick / upem).max(0.02),
        cmap,
        adv,
        key,
        bold: f.is_bold(),
        italic: f.is_italic() || f.is_oblique(),
        glyf: f.tables().glyf.is_some(),
        variable: f.is_variable(),
        space,
        data,
    })
}

pub struct FontStore {
    pub faces: Vec<Face>,
    fams: Vec<Family>,
    fam_index: HashMap<String, usize>,
    fallback: Vec<usize>,
    fallback_names: Vec<String>,
    resolve_cache: HashMap<(String, bool, bool), Resolved>,
    pub defs: egui::FontDefinitions,
    pub egui_dirty: bool,
    check_active: bool,
    active: HashSet<usize>,
    /// Bumped whenever font availability changes (layout caches key on it).
    pub generation: u64,
    db: fontdb::Database,
    pub google: GoogleState,
    ctx: Option<egui::Context>,
    tx: mpsc::Sender<FontEvent>,
    rx: mpsc::Receiver<FontEvent>,
    pub auto_download: bool,
}

impl FontStore {
    pub fn new() -> FontStore {
        let (tx, rx) = mpsc::channel();
        let defs = egui::FontDefinitions::default();
        let mut store = FontStore {
            faces: vec![],
            fams: vec![],
            fam_index: HashMap::new(),
            fallback: vec![],
            fallback_names: vec![],
            resolve_cache: HashMap::new(),
            defs,
            egui_dirty: false,
            check_active: false,
            active: HashSet::new(),
            generation: 1,
            db: fontdb::Database::new(),
            google: GoogleState {
                catalog: google::load_cached_catalog().unwrap_or_else(google::bundled_catalog),
                catalog_online: false,
                loading_catalog: false,
                in_flight: HashSet::new(),
                installed: HashSet::new(),
                status: String::new(),
                api_key: std::env::var("GOOGLE_FONTS_API_KEY").unwrap_or_default(),
            },
            ctx: None,
            tx,
            rx,
            auto_download: true,
        };
        store.google.catalog_online = google::catalog_cache_path().exists();
        // fallback faces: egui's bundled fonts (for symbols and emoji)
        let names = store.defs.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
        for n in names {
            if let Some(fd) = store.defs.font_data.get(&n) {
                let data = Arc::new(fd.font.to_vec());
                if let Some(face) = parse_face(data, fd.index, "(fallback)", n.clone()) {
                    store.fallback.push(store.faces.len());
                    store.fallback_names.push(n.clone());
                    store.faces.push(face);
                }
            }
        }
        // embedded Carlito
        let mut srcs = vec![];
        for (i, bytes) in CARLITO.iter().enumerate() {
            srcs.push(StyleSrc { bold: i & 1 == 1, italic: i & 2 == 2, weight: if i & 1 == 1 { 700 } else { 400 }, src: Src::Mem(bytes) });
        }
        store.add_family("Carlito", srcs, false);
        store.scan_google_cache();
        store.scan_system();
        store
    }

    /// Put a face first in egui's proportional family so the UI uses it too.
    pub fn use_as_ui_font(&mut self, face: usize) {
        let key = self.faces[face].key.clone();
        if let Some(list) = self.defs.families.get_mut(&egui::FontFamily::Proportional) {
            list.retain(|k| *k != key);
            list.insert(0, key);
        }
        self.egui_dirty = true;
    }

    pub fn attach_ctx(&mut self, ctx: &egui::Context) {
        self.ctx = Some(ctx.clone());
    }

    fn add_family(&mut self, name: &str, srcs: Vec<StyleSrc>, google: bool) {
        let l = name.to_lowercase();
        if let Some(&fi) = self.fam_index.get(&l) {
            if google {
                // downloaded copy replaces whatever was there
                let f = &mut self.fams[fi];
                f.srcs = srcs;
                f.loaded = [None; 4];
                f.tried = [false; 4];
                f.google = true;
            } else {
                self.fams[fi].srcs.extend(srcs);
            }
            return;
        }
        self.fam_index.insert(l, self.fams.len());
        self.fams.push(Family { name: name.to_string(), srcs, loaded: [None; 4], tried: [false; 4], google });
    }

    fn scan_system(&mut self) {
        self.db.load_system_fonts();
        let mut found: Vec<(String, StyleSrc)> = vec![];
        for f in self.db.faces() {
            if let Some((name, _)) = f.families.first() {
                let italic = matches!(f.style, fontdb::Style::Italic | fontdb::Style::Oblique);
                found.push((name.clone(), StyleSrc { bold: f.weight.0 >= 600, italic, weight: f.weight.0, src: Src::Sys(f.id) }));
            }
        }
        for (name, s) in found {
            self.add_family(&name, vec![s], false);
        }
    }

    fn scan_google_dir(&mut self, dir: &std::path::Path) {
        let name = match dir.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => return,
        };
        let mut srcs = vec![];
        for (i, slot) in google::SLOTS.iter().enumerate() {
            let p = dir.join(format!("{slot}.ttf"));
            if p.exists() {
                srcs.push(StyleSrc { bold: i & 1 == 1, italic: i & 2 == 2, weight: if i & 1 == 1 { 700 } else { 400 }, src: Src::Path(p, 0) });
            }
        }
        if !srcs.is_empty() {
            self.add_family(&name, srcs, true);
            self.google.installed.insert(name);
        }
    }

    fn scan_google_cache(&mut self) {
        if let Ok(rd) = std::fs::read_dir(google::cache_root()) {
            let dirs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
            for d in dirs {
                self.scan_google_dir(&d);
            }
        }
    }

    pub fn family_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.fams.iter().map(|f| f.name.clone()).collect();
        v.sort_by_key(|s| s.to_lowercase());
        v.dedup();
        v
    }

    pub fn has_family(&self, name: &str) -> bool {
        self.fam_index.contains_key(&name.to_lowercase())
    }

    pub fn is_google_installed(&self, name: &str) -> bool {
        self.google.installed.iter().any(|n| n.eq_ignore_ascii_case(name))
    }

    pub fn catalog_entry(&self, name: &str) -> Option<&GFont> {
        self.google.catalog.iter().find(|g| g.family.eq_ignore_ascii_case(name))
    }

    fn find_family(&self, lname: &str) -> Option<usize> {
        if let Some(&fi) = self.fam_index.get(lname) {
            return Some(fi);
        }
        for cand in alias_candidates(lname) {
            if let Some(&fi) = self.fam_index.get(*cand) {
                return Some(fi);
            }
        }
        None
    }

    fn maybe_request_substitute(&mut self, name: &str, lname: &str) {
        if !self.auto_download {
            return;
        }
        let mut target: Option<String> = None;
        if let Some(g) = self.catalog_entry(name) {
            target = Some(g.family.clone());
        } else {
            for cand in alias_candidates(lname) {
                if let Some(g) = self.catalog_entry(cand) {
                    target = Some(g.family.clone());
                    break;
                }
            }
        }
        if let Some(t) = target {
            self.request_google(&t);
        }
    }

    pub fn resolve(&mut self, name: &str, bold: bool, italic: bool) -> Resolved {
        let lname = name.trim().to_lowercase();
        if let Some(r) = self.resolve_cache.get(&(lname.clone(), bold, italic)) {
            return *r;
        }
        let fi = match self.find_family(&lname) {
            Some(fi) => fi,
            None => {
                self.maybe_request_substitute(name, &lname);
                *self.fam_index.get("carlito").unwrap()
            }
        };
        let r = self.resolve_in(fi, bold, italic);
        self.resolve_cache.insert((lname, bold, italic), r);
        r
    }

    fn resolve_in(&mut self, fi: usize, bold: bool, italic: bool) -> Resolved {
        let want = (bold as usize) | ((italic as usize) << 1);
        let mut order = vec![want, want & !1, want & !2, 0];
        order.dedup();
        for slot in order {
            if let Some(face) = self.load_slot(fi, slot) {
                return Resolved { face, synth_bold: bold && slot & 1 == 0, synth_italic: italic && slot & 2 == 0 };
            }
        }
        // anything at all
        for slot in 0..4 {
            if let Some(face) = self.load_slot(fi, slot) {
                return Resolved { face, synth_bold: bold && slot & 1 == 0, synth_italic: italic && slot & 2 == 0 };
            }
        }
        let d = *self.fam_index.get("carlito").unwrap();
        let face = self.load_slot(d, 0).unwrap();
        Resolved { face, synth_bold: bold, synth_italic: italic }
    }

    fn load_slot(&mut self, fi: usize, slot: usize) -> Option<usize> {
        if let Some(f) = self.fams[fi].loaded[slot] {
            return Some(f);
        }
        if self.fams[fi].tried[slot] {
            return None;
        }
        self.fams[fi].tried[slot] = true;
        let (want_bold, want_italic) = (slot & 1 == 1, slot & 2 == 2);
        let target: i32 = if want_bold { 700 } else { 400 };
        let fam = &self.fams[fi];
        let mut cands: Vec<&StyleSrc> = fam.srcs.iter().filter(|s| s.italic == want_italic).collect();
        if cands.is_empty() && slot == 0 {
            cands = fam.srcs.iter().collect();
        }
        if want_bold {
            cands.retain(|s| s.weight >= 600);
        }
        let best = cands.into_iter().min_by_key(|s| (s.weight as i32 - target).abs())?;
        let (bytes, index): (Vec<u8>, u32) = match &best.src {
            Src::Mem(b) => (b.to_vec(), 0),
            Src::Path(p, i) => (std::fs::read(p).ok()?, *i),
            Src::Sys(id) => self.db.with_face_data(*id, |d, i| (d.to_vec(), i))?,
        };
        let fam_name = fam.name.clone();
        let id = self.faces.len();
        let key = format!("wf{id}");
        let face = parse_face(Arc::new(bytes), index, &fam_name, key.clone())?;
        self.defs.font_data.insert(
            key.clone(),
            Arc::new(egui::FontData { font: Cow::Owned(face.data.to_vec()), index, tweak: Default::default() }),
        );
        let mut list = vec![key.clone()];
        list.extend(self.fallback_names.iter().cloned());
        self.defs.families.insert(egui::FontFamily::Name(key.as_str().into()), list);
        self.faces.push(face);
        self.fams[fi].loaded[slot] = Some(id);
        self.egui_dirty = true;
        Some(id)
    }

    // ------------------------------------------------------------ metrics

    pub fn glyph_adv(&self, face: usize, ch: char) -> f32 {
        let f = &self.faces[face];
        if let Some(&g) = f.cmap.get(&ch) {
            return f.adv.get(g as usize).copied().unwrap_or(0) as f32 / f.upem;
        }
        for &fb in &self.fallback {
            let ff = &self.faces[fb];
            if let Some(&g) = ff.cmap.get(&ch) {
                return ff.adv.get(g as usize).copied().unwrap_or(0) as f32 / ff.upem;
            }
        }
        f.space.max(0.5)
    }

    /// The face and glyph id that will actually draw `ch` (primary face, then fallbacks).
    pub fn glyph_for(&self, face: usize, ch: char) -> Option<(usize, u16)> {
        if let Some(&g) = self.faces[face].cmap.get(&ch) {
            return Some((face, g));
        }
        for &fb in &self.fallback {
            if let Some(&g) = self.faces[fb].cmap.get(&ch) {
                return Some((fb, g));
            }
        }
        None
    }

    pub fn text_width_em(&self, face: usize, text: &str) -> f32 {
        text.chars().map(|c| self.glyph_adv(face, c)).sum()
    }

    pub fn has_glyph(&self, face: usize, ch: char) -> bool {
        self.faces[face].cmap.contains_key(&ch)
    }

    // ------------------------------------------------------------ egui sync

    /// Install fonts into egui right away (before the first frame).
    pub fn install_now(&mut self, ctx: &egui::Context) {
        ctx.set_fonts(self.defs.clone());
        self.egui_dirty = false;
        self.check_active = true;
        for i in 0..self.faces.len() {
            self.active.insert(i);
        }
    }

    pub fn sync_egui(&mut self, ctx: &egui::Context) {
        if self.egui_dirty {
            ctx.set_fonts(self.defs.clone());
            self.egui_dirty = false;
            self.check_active = true;
            ctx.request_repaint();
        } else if self.check_active {
            let fams: HashSet<String> = ctx
                .fonts(|f| f.families())
                .into_iter()
                .filter_map(|f| match f {
                    egui::FontFamily::Name(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect();
            let mut all = true;
            for (i, face) in self.faces.iter().enumerate() {
                if self.fallback.contains(&i) || self.active.contains(&i) {
                    continue;
                }
                if fams.contains(&face.key) {
                    self.active.insert(i);
                } else {
                    all = false;
                }
            }
            if all {
                self.check_active = false;
            } else {
                ctx.request_repaint();
            }
        }
    }

    pub fn egui_family(&self, face: usize) -> egui::FontFamily {
        if self.active.contains(&face) {
            egui::FontFamily::Name(self.faces[face].key.as_str().into())
        } else {
            egui::FontFamily::Proportional
        }
    }

    // ------------------------------------------------------------ Google Fonts

    pub fn canonical_google_name(&self, name: &str) -> String {
        self.catalog_entry(name).map(|g| g.family.clone()).unwrap_or_else(|| name.trim().to_string())
    }

    pub fn request_google(&mut self, family: &str) {
        let family = self.canonical_google_name(family);
        if self.google.in_flight.contains(&family) || self.is_google_installed(&family) {
            return;
        }
        self.google.in_flight.insert(family.clone());
        self.google.status = format!("Downloading {family}…");
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let key = if self.google.api_key.trim().is_empty() { None } else { Some(self.google.api_key.clone()) };
        let dir = google::cache_root().join(family.replace(['/', '\\', ':'], "_"));
        std::thread::spawn(move || {
            let res = google::download_family(&family, key.as_deref(), &dir);
            let ev = match res {
                Ok(note) => FontEvent::Installed { family, note },
                Err(err) => FontEvent::Failed { family, err },
            };
            let _ = tx.send(ev);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    pub fn refresh_catalog(&mut self) {
        if self.google.loading_catalog {
            return;
        }
        self.google.loading_catalog = true;
        self.google.status = "Fetching the Google Fonts catalog…".into();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let key = if self.google.api_key.trim().is_empty() { None } else { Some(self.google.api_key.clone()) };
        std::thread::spawn(move || {
            let ev = match google::fetch_catalog(key.as_deref()) {
                Ok(c) => {
                    google::save_catalog(&c);
                    FontEvent::Catalog(c)
                }
                Err(e) => FontEvent::CatalogErr(e),
            };
            let _ = tx.send(ev);
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Process background events. Returns true if layout-relevant state changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                FontEvent::Catalog(c) => {
                    self.google.status = format!("Google Fonts catalog updated: {} families", c.len());
                    self.google.catalog = c;
                    self.google.catalog_online = true;
                    self.google.loading_catalog = false;
                }
                FontEvent::CatalogErr(e) => {
                    self.google.status = format!("Could not fetch the full catalog ({e}). Using the built-in list.");
                    self.google.loading_catalog = false;
                }
                FontEvent::Installed { family, note } => {
                    self.google.in_flight.remove(&family);
                    let dir = google::cache_root().join(family.replace(['/', '\\', ':'], "_"));
                    self.scan_google_dir(&dir);
                    self.google.status = note;
                    self.resolve_cache.clear();
                    self.generation += 1;
                    changed = true;
                }
                FontEvent::Failed { family, err } => {
                    self.google.in_flight.remove(&family);
                    self.google.status = err;
                }
            }
        }
        changed
    }

    pub fn downloads_pending(&self) -> bool {
        !self.google.in_flight.is_empty() || self.google.loading_catalog
    }
}

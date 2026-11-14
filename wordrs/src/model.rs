//! Document model: formatting, paragraphs, runs, tables, styles, page setup.
//! All lengths are in points (1/72 inch).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::story::Story;

static REV: AtomicU64 = AtomicU64::new(1);
/// A process-unique token. Every mutation of a paragraph takes a new one, which
/// lets the layout cache detect changes (and survive undo/redo) cheaply.
pub fn next_rev() -> u64 {
    REV.fetch_add(1, Ordering::Relaxed)
}

pub type Rgb = [u8; 3];
pub const BLACK: Rgb = [0, 0, 0];
pub const OBJ_CHAR: char = '\u{FFFC}';
pub const PAGE_BREAK: char = '\u{000C}';

pub fn byte_idx(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

// ---------------------------------------------------------------- enums

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VertAlign {
    #[default]
    Baseline,
    Super,
    Sub,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineSpacing {
    Multiple(f32),
    Exact(f32),
    AtLeast(f32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKind {
    Bullet,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListInfo {
    pub id: u32,
    pub level: u8,
    pub kind: ListKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabKind {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leader {
    None,
    Dots,
    Dashes,
    Line,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabStop {
    pub pos: f32,
    pub kind: TabKind,
    pub leader: Leader,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub w: f32,
    pub color: Rgb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VAlign {
    #[default]
    Top,
    Center,
    Bottom,
}

// ---------------------------------------------------------------- formatting

/// Direct character formatting. `None` means "inherit from style".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharFmt {
    pub font: Option<String>,
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    pub color: Option<Rgb>,
    /// `Some(None)` = explicitly no highlight.
    pub highlight: Option<Option<Rgb>>,
    pub vert: Option<VertAlign>,
    pub small_caps: Option<bool>,
    pub all_caps: Option<bool>,
    pub spacing: Option<f32>,
}

impl CharFmt {
    /// Overlay `o` on top of `self` (fields set in `o` win).
    pub fn merge(&mut self, o: &CharFmt) {
        macro_rules! m {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        m!(font, size, bold, italic, underline, strike, color, highlight, vert, small_caps, all_caps, spacing);
    }
    pub fn is_empty(&self) -> bool {
        *self == CharFmt::default()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParaBorders {
    pub top: Option<Border>,
    pub bottom: Option<Border>,
    pub left: Option<Border>,
    pub right: Option<Border>,
}

/// Direct paragraph formatting. `None` means "inherit from style".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParaFmt {
    pub align: Option<Align>,
    pub left: Option<f32>,
    pub right: Option<f32>,
    /// First-line indent; negative = hanging.
    pub first: Option<f32>,
    pub before: Option<f32>,
    pub after: Option<f32>,
    pub line: Option<LineSpacing>,
    pub keep_next: Option<bool>,
    pub keep_lines: Option<bool>,
    pub page_break_before: Option<bool>,
    pub shade: Option<Option<Rgb>>,
    pub borders: Option<ParaBorders>,
    pub tabs: Option<Vec<TabStop>>,
}

impl ParaFmt {
    pub fn merge(&mut self, o: &ParaFmt) {
        macro_rules! m {
            ($($f:ident),*) => { $( if o.$f.is_some() { self.$f = o.$f.clone(); } )* };
        }
        m!(align, left, right, first, before, after, line, keep_next, keep_lines, page_break_before, shade, borders, tabs);
    }
}

/// Fully resolved character formatting.
#[derive(Clone, Debug, PartialEq)]
pub struct RChar {
    pub font: Arc<str>,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Rgb,
    pub highlight: Option<Rgb>,
    pub vert: VertAlign,
    pub small_caps: bool,
    pub all_caps: bool,
    pub spacing: f32,
}

impl RChar {
    pub fn base() -> RChar {
        RChar {
            font: Arc::from("Calibri"),
            size: 11.0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            color: BLACK,
            highlight: None,
            vert: VertAlign::Baseline,
            small_caps: false,
            all_caps: false,
            spacing: 0.0,
        }
    }
    pub fn apply(&mut self, f: &CharFmt) {
        if let Some(v) = &f.font {
            self.font = Arc::from(v.as_str());
        }
        if let Some(v) = f.size {
            self.size = v;
        }
        if let Some(v) = f.bold {
            self.bold = v;
        }
        if let Some(v) = f.italic {
            self.italic = v;
        }
        if let Some(v) = f.underline {
            self.underline = v;
        }
        if let Some(v) = f.strike {
            self.strike = v;
        }
        if let Some(v) = f.color {
            self.color = v;
        }
        if let Some(v) = f.highlight {
            self.highlight = v;
        }
        if let Some(v) = f.vert {
            self.vert = v;
        }
        if let Some(v) = f.small_caps {
            self.small_caps = v;
        }
        if let Some(v) = f.all_caps {
            self.all_caps = v;
        }
        if let Some(v) = f.spacing {
            self.spacing = v;
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RPara {
    pub align: Align,
    pub left: f32,
    pub right: f32,
    pub first: f32,
    pub before: f32,
    pub after: f32,
    pub line: LineSpacing,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub page_break_before: bool,
    pub shade: Option<Rgb>,
    pub borders: ParaBorders,
    pub tabs: Vec<TabStop>,
}

impl RPara {
    pub fn base() -> RPara {
        RPara {
            align: Align::Left,
            left: 0.0,
            right: 0.0,
            first: 0.0,
            before: 0.0,
            after: 0.0,
            line: LineSpacing::Multiple(1.0),
            keep_next: false,
            keep_lines: false,
            page_break_before: false,
            shade: None,
            borders: ParaBorders::default(),
            tabs: vec![],
        }
    }
    pub fn apply(&mut self, f: &ParaFmt) {
        if let Some(v) = f.align {
            self.align = v;
        }
        if let Some(v) = f.left {
            self.left = v;
        }
        if let Some(v) = f.right {
            self.right = v;
        }
        if let Some(v) = f.first {
            self.first = v;
        }
        if let Some(v) = f.before {
            self.before = v;
        }
        if let Some(v) = f.after {
            self.after = v;
        }
        if let Some(v) = f.line {
            self.line = v;
        }
        if let Some(v) = f.keep_next {
            self.keep_next = v;
        }
        if let Some(v) = f.keep_lines {
            self.keep_lines = v;
        }
        if let Some(v) = f.page_break_before {
            self.page_break_before = v;
        }
        if let Some(v) = f.shade {
            self.shade = v;
        }
        if let Some(v) = &f.borders {
            self.borders = v.clone();
        }
        if let Some(v) = &f.tabs {
            self.tabs = v.clone();
        }
    }
}

// ---------------------------------------------------------------- content

#[derive(Debug)]
pub struct ImageData {
    pub id: u64,
    pub bytes: Vec<u8>,
    pub px_w: u32,
    pub px_h: u32,
    pub mime: &'static str,
}

impl ImageData {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Arc<ImageData>, String> {
        let fmt = image::guess_format(&bytes).map_err(|e| e.to_string())?;
        let mime = match fmt {
            image::ImageFormat::Png => "image/png",
            image::ImageFormat::Jpeg => "image/jpeg",
            image::ImageFormat::Gif => "image/gif",
            image::ImageFormat::Bmp => "image/bmp",
            image::ImageFormat::WebP => "image/webp",
            _ => return Err("Unsupported image format".into()),
        };
        let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(&bytes[..]), fmt)
            .into_dimensions()
            .map_err(|e| e.to_string())?;
        let mut hash: u64 = 0xcbf29ce484222325;
        for b in bytes.iter() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        Ok(Arc::new(ImageData { id: hash, bytes, px_w: w, px_h: h, mime }))
    }
    pub fn decode(&self) -> Option<image::RgbaImage> {
        image::load_from_memory(&self.bytes).ok().map(|i| i.to_rgba8())
    }
    pub fn ext(&self) -> &'static str {
        match self.mime {
            "image/png" => "png",
            "image/jpeg" => "jpeg",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            _ => "webp",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ImageObj {
    pub data: Arc<ImageData>,
    pub w: f32,
    pub h: f32,
}

impl PartialEq for ImageObj {
    fn eq(&self, o: &Self) -> bool {
        self.data.id == o.data.id && self.w == o.w && self.h == o.h
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Page,
    NumPages,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Obj {
    Image(ImageObj),
    Field(Field),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub fmt: CharFmt,
    pub link: Option<String>,
    pub obj: Option<Obj>,
}

impl Run {
    pub fn text(t: &str, fmt: CharFmt) -> Run {
        Run { text: t.to_string(), fmt, link: None, obj: None }
    }
    fn empty(fmt: CharFmt) -> Run {
        Run { text: String::new(), fmt, link: None, obj: None }
    }
}

#[derive(Clone, Debug)]
pub struct Paragraph {
    pub rev: u64,
    pub style: String,
    pub fmt: ParaFmt,
    pub list: Option<ListInfo>,
    pub runs: Vec<Run>,
}

impl Default for Paragraph {
    fn default() -> Self {
        Paragraph::new()
    }
}

impl Paragraph {
    pub fn new() -> Self {
        Paragraph { rev: next_rev(), style: "Normal".into(), fmt: ParaFmt::default(), list: None, runs: vec![] }
    }
    pub fn with_text(text: &str) -> Self {
        let mut p = Paragraph::new();
        if !text.is_empty() {
            p.runs.push(Run::text(text, CharFmt::default()));
        }
        p
    }
    pub fn styled(style: &str, text: &str) -> Self {
        let mut p = Paragraph::with_text(text);
        p.style = style.to_string();
        p
    }
    pub fn len(&self) -> usize {
        self.runs.iter().map(|r| r.text.chars().count()).sum()
    }
    pub fn is_empty(&self) -> bool {
        self.runs.iter().all(|r| r.text.is_empty())
    }
    pub fn text(&self) -> String {
        let mut s = String::new();
        for r in &self.runs {
            s.push_str(&r.text);
        }
        s
    }
    pub fn touch(&mut self) {
        self.rev = next_rev();
    }

    /// Ensure a run boundary at char offset `off`; return the index of the first run starting at/after it.
    fn boundary(&mut self, off: usize) -> usize {
        let mut acc = 0;
        for i in 0..self.runs.len() {
            let n = self.runs[i].text.chars().count();
            if off == acc {
                return i;
            }
            if off < acc + n {
                let b = byte_idx(&self.runs[i].text, off - acc);
                let tail = self.runs[i].text.split_off(b);
                let mut r2 = self.runs[i].clone();
                r2.text = tail;
                self.runs.insert(i + 1, r2);
                return i + 1;
            }
            acc += n;
        }
        self.runs.len()
    }

    pub fn normalize(&mut self) {
        let mut out: Vec<Run> = Vec::with_capacity(self.runs.len());
        let mut empty: Option<Run> = None;
        for r in self.runs.drain(..) {
            if r.text.is_empty() && r.obj.is_none() {
                empty = Some(r);
                continue;
            }
            if let Some(last) = out.last_mut() {
                if last.obj.is_none() && r.obj.is_none() && last.fmt == r.fmt && last.link == r.link {
                    last.text.push_str(&r.text);
                    continue;
                }
            }
            out.push(r);
        }
        if out.is_empty() {
            if let Some(e) = empty {
                out.push(e);
            }
        }
        self.runs = out;
    }

    /// Format of the character before `off` (or the first character at 0).
    pub fn fmt_at(&self, off: usize) -> (CharFmt, Option<String>) {
        let target = if off > 0 { off - 1 } else { 0 };
        let mut acc = 0;
        for r in &self.runs {
            let n = r.text.chars().count();
            if target < acc + n || (n == 0) {
                return (r.fmt.clone(), r.link.clone());
            }
            acc += n;
        }
        match self.runs.last() {
            Some(r) => (r.fmt.clone(), None),
            None => (CharFmt::default(), None),
        }
    }

    pub fn insert(&mut self, off: usize, text: &str, fmt: Option<&CharFmt>, link: Option<Option<String>>) {
        if text.is_empty() {
            return;
        }
        let idx = self.boundary(off);
        let (base_fmt, base_link) = if idx > 0 {
            let r = &self.runs[idx - 1];
            let inside = idx < self.runs.len() && self.runs[idx].link == r.link && r.link.is_some();
            (r.fmt.clone(), if inside { r.link.clone() } else { None })
        } else if let Some(r) = self.runs.first() {
            (r.fmt.clone(), None)
        } else {
            (CharFmt::default(), None)
        };
        let run = Run {
            text: text.to_string(),
            fmt: fmt.cloned().unwrap_or(base_fmt),
            link: link.unwrap_or(base_link),
            obj: None,
        };
        self.runs.insert(idx, run);
        self.normalize();
    }

    pub fn insert_obj(&mut self, off: usize, obj: Obj, fmt: Option<&CharFmt>) {
        let idx = self.boundary(off);
        let base_fmt = if idx > 0 { self.runs[idx - 1].fmt.clone() } else { self.runs.first().map(|r| r.fmt.clone()).unwrap_or_default() };
        let run = Run { text: OBJ_CHAR.to_string(), fmt: fmt.cloned().unwrap_or(base_fmt), link: None, obj: Some(obj) };
        self.runs.insert(idx, run);
        self.normalize();
    }

    pub fn delete(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        let i0 = self.boundary(a);
        let i1 = self.boundary(b);
        let keep = self.runs.get(i0).map(|r| r.fmt.clone());
        self.runs.drain(i0..i1);
        if self.runs.is_empty() {
            if let Some(f) = keep {
                self.runs.push(Run::empty(f));
            }
        }
        self.normalize();
    }

    /// Split at `off`; `self` keeps the head, the returned paragraph is the tail.
    pub fn split_off(&mut self, off: usize) -> Paragraph {
        let idx = self.boundary(off);
        let tail_runs: Vec<Run> = self.runs.split_off(idx);
        let mut right = Paragraph { rev: next_rev(), style: self.style.clone(), fmt: self.fmt.clone(), list: self.list, runs: tail_runs };
        if right.runs.is_empty() {
            if let Some(l) = self.runs.last() {
                right.runs.push(Run::empty(l.fmt.clone()));
            }
        }
        if self.runs.is_empty() {
            if let Some(f) = right.runs.first() {
                self.runs.push(Run::empty(f.fmt.clone()));
            }
        }
        self.touch();
        right
    }

    pub fn append(&mut self, other: Paragraph) {
        self.runs.extend(other.runs);
        self.normalize();
        self.touch();
    }

    pub fn modify(&mut self, a: usize, b: usize, f: &dyn Fn(&mut Run)) {
        if a >= b {
            if a == b && self.len() == 0 {
                if self.runs.is_empty() {
                    self.runs.push(Run::empty(CharFmt::default()));
                }
                for r in self.runs.iter_mut() {
                    f(r);
                }
            }
            return;
        }
        let i0 = self.boundary(a);
        let i1 = self.boundary(b);
        for r in self.runs[i0..i1].iter_mut() {
            f(r);
        }
        self.normalize();
    }

    pub fn slice(&self, a: usize, b: usize) -> Paragraph {
        let mut out = Paragraph { rev: next_rev(), style: self.style.clone(), fmt: self.fmt.clone(), list: self.list, runs: vec![] };
        let mut acc = 0;
        for r in &self.runs {
            let n = r.text.chars().count();
            let (s, e) = (a.max(acc), b.min(acc + n));
            if s < e {
                let bs = byte_idx(&r.text, s - acc);
                let be = byte_idx(&r.text, e - acc);
                out.runs.push(Run { text: r.text[bs..be].to_string(), fmt: r.fmt.clone(), link: r.link.clone(), obj: r.obj.clone() });
            }
            acc += n;
        }
        if out.runs.is_empty() {
            if let Some(r) = self.runs.first() {
                out.runs.push(Run::empty(r.fmt.clone()));
            }
        }
        out
    }

    /// Text with object placeholders and page-break marks removed.
    pub fn plain(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect::<String>().replace([OBJ_CHAR, PAGE_BREAK], "")
    }
}

// ---------------------------------------------------------------- tables

#[derive(Clone, Debug)]
pub struct Cell {
    pub paras: Vec<Paragraph>,
    pub cs: usize,
    pub rs: usize,
    pub covered: bool,
    pub shade: Option<Rgb>,
    pub valign: VAlign,
}

impl Cell {
    pub fn new() -> Cell {
        Cell { paras: vec![Paragraph::new()], cs: 1, rs: 1, covered: false, shade: None, valign: VAlign::Top }
    }
    pub fn covered() -> Cell {
        Cell { covered: true, ..Cell::new() }
    }
}

impl Default for Cell {
    fn default() -> Self {
        Cell::new()
    }
}

#[derive(Clone, Debug)]
pub struct TRow {
    pub cells: Vec<Cell>,
    pub height: Option<f32>,
    pub header: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableBorders {
    pub outer: Option<Border>,
    pub inner_h: Option<Border>,
    pub inner_v: Option<Border>,
}

impl TableBorders {
    pub fn all(w: f32) -> TableBorders {
        let b = Some(Border { w, color: BLACK });
        TableBorders { outer: b, inner_h: b, inner_v: b }
    }
    pub fn none() -> TableBorders {
        TableBorders { outer: None, inner_h: None, inner_v: None }
    }
}

#[derive(Clone, Debug)]
pub struct Table {
    pub cols: Vec<f32>,
    pub rows: Vec<TRow>,
    pub borders: TableBorders,
    pub align: Align,
    pub pad_lr: f32,
}

impl Table {
    pub fn new(rows: usize, cols: usize, width: f32) -> Table {
        let w = width / cols.max(1) as f32;
        Table {
            cols: vec![w; cols],
            rows: (0..rows).map(|_| TRow { cells: (0..cols).map(|_| Cell::new()).collect(), height: None, header: false }).collect(),
            borders: TableBorders::all(0.5),
            align: Align::Left,
            pad_lr: 5.4,
        }
    }
    pub fn width(&self) -> f32 {
        self.cols.iter().sum()
    }
    pub fn ncols(&self) -> usize {
        self.cols.len()
    }
}

#[derive(Clone, Debug)]
pub enum Block {
    Para(Paragraph),
    Table(Table),
}

// ---------------------------------------------------------------- styles

#[derive(Clone, Debug)]
pub struct Style {
    pub name: String,
    pub based_on: Option<String>,
    pub next: Option<String>,
    pub ch: CharFmt,
    pub para: ParaFmt,
    pub outline: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct Styles {
    pub list: Vec<Style>,
    pub def_char: CharFmt,
    pub def_para: ParaFmt,
}

fn style(name: &str, based: Option<&str>, next: Option<&str>, ch: CharFmt, para: ParaFmt, outline: Option<u8>) -> Style {
    Style { name: name.into(), based_on: based.map(|s| s.into()), next: next.map(|s| s.into()), ch, para, outline }
}

impl Styles {
    pub fn word_defaults() -> Styles {
        let blue = [0x2F, 0x54, 0x96];
        let dblue = [0x1F, 0x37, 0x63];
        let heading_font = Some("Calibri Light".to_string());
        let hp = |before: f32| ParaFmt { before: Some(before), after: Some(0.0), keep_next: Some(true), keep_lines: Some(true), ..Default::default() };
        let list = vec![
            style("Normal", None, None, CharFmt::default(), ParaFmt::default(), None),
            style("No Spacing", Some("Normal"), None, CharFmt::default(), ParaFmt { after: Some(0.0), line: Some(LineSpacing::Multiple(1.0)), ..Default::default() }, None),
            style("Heading 1", Some("Normal"), Some("Normal"), CharFmt { font: heading_font.clone(), size: Some(16.0), color: Some(blue), ..Default::default() }, hp(12.0), Some(1)),
            style("Heading 2", Some("Normal"), Some("Normal"), CharFmt { font: heading_font.clone(), size: Some(13.0), color: Some(blue), ..Default::default() }, hp(2.0), Some(2)),
            style("Heading 3", Some("Normal"), Some("Normal"), CharFmt { font: heading_font.clone(), size: Some(12.0), color: Some(dblue), ..Default::default() }, hp(2.0), Some(3)),
            style("Heading 4", Some("Normal"), Some("Normal"), CharFmt { font: heading_font.clone(), italic: Some(true), color: Some(blue), ..Default::default() }, hp(2.0), Some(4)),
            style("Heading 5", Some("Normal"), Some("Normal"), CharFmt { font: heading_font.clone(), color: Some(blue), ..Default::default() }, hp(2.0), Some(5)),
            style("Heading 6", Some("Normal"), Some("Normal"), CharFmt { font: heading_font.clone(), color: Some(dblue), ..Default::default() }, hp(2.0), Some(6)),
            style("Title", Some("Normal"), Some("Normal"), CharFmt { font: heading_font.clone(), size: Some(28.0), spacing: Some(-0.5), ..Default::default() }, ParaFmt { after: Some(0.0), line: Some(LineSpacing::Multiple(1.0)), ..Default::default() }, None),
            style("Subtitle", Some("Normal"), Some("Normal"), CharFmt { color: Some([0x5A, 0x5A, 0x5A]), spacing: Some(0.75), ..Default::default() }, ParaFmt::default(), None),
            style("Quote", Some("Normal"), Some("Normal"), CharFmt { italic: Some(true), color: Some([0x40, 0x40, 0x40]), ..Default::default() }, ParaFmt { align: Some(Align::Center), left: Some(43.2), right: Some(43.2), before: Some(10.0), after: Some(8.0), ..Default::default() }, None),
            style("Intense Quote", Some("Normal"), Some("Normal"), CharFmt { italic: Some(true), color: Some(blue), ..Default::default() }, ParaFmt { align: Some(Align::Center), left: Some(43.2), right: Some(43.2), before: Some(18.0), after: Some(18.0), borders: Some(ParaBorders { top: Some(Border { w: 0.75, color: blue }), bottom: Some(Border { w: 0.75, color: blue }), ..Default::default() }), ..Default::default() }, None),
            style("List Paragraph", Some("Normal"), None, CharFmt::default(), ParaFmt { left: Some(36.0), ..Default::default() }, None),
            style("Caption", Some("Normal"), Some("Normal"), CharFmt { italic: Some(true), size: Some(9.0), color: Some([0x44, 0x54, 0x6A]), ..Default::default() }, ParaFmt { after: Some(10.0), line: Some(LineSpacing::Multiple(1.0)), ..Default::default() }, None),
            style("Code", Some("Normal"), Some("Code"), CharFmt { font: Some("Courier New".into()), size: Some(10.0), ..Default::default() }, ParaFmt { before: Some(0.0), after: Some(0.0), line: Some(LineSpacing::Multiple(1.0)), shade: Some(Some([0xF2, 0xF2, 0xF2])), ..Default::default() }, None),
            style("Header", Some("Normal"), None, CharFmt::default(), ParaFmt { after: Some(0.0), line: Some(LineSpacing::Multiple(1.0)), tabs: Some(vec![TabStop { pos: 234.0, kind: TabKind::Center, leader: Leader::None }, TabStop { pos: 468.0, kind: TabKind::Right, leader: Leader::None }]), ..Default::default() }, None),
            style("Footer", Some("Normal"), None, CharFmt::default(), ParaFmt { after: Some(0.0), line: Some(LineSpacing::Multiple(1.0)), tabs: Some(vec![TabStop { pos: 234.0, kind: TabKind::Center, leader: Leader::None }, TabStop { pos: 468.0, kind: TabKind::Right, leader: Leader::None }]), ..Default::default() }, None),
        ];
        Styles {
            list,
            def_char: CharFmt { font: Some("Calibri".into()), size: Some(11.0), ..Default::default() },
            def_para: ParaFmt { after: Some(8.0), line: Some(LineSpacing::Multiple(1.08)), ..Default::default() },
        }
    }

    pub fn get(&self, name: &str) -> Option<&Style> {
        self.list.iter().find(|s| s.name == name)
    }

    pub fn names(&self) -> Vec<String> {
        self.list.iter().map(|s| s.name.clone()).collect()
    }

    pub fn upsert(&mut self, s: Style) {
        if let Some(e) = self.list.iter_mut().find(|e| e.name == s.name) {
            *e = s;
        } else {
            self.list.push(s);
        }
    }

    pub fn resolve(&self, name: &str) -> (RChar, RPara) {
        let mut chain: Vec<&Style> = vec![];
        let mut cur = self.get(name);
        while let Some(s) = cur {
            if chain.len() > 12 || chain.iter().any(|c| c.name == s.name) {
                break;
            }
            chain.push(s);
            cur = s.based_on.as_deref().and_then(|b| self.get(b));
        }
        let mut c = RChar::base();
        let mut p = RPara::base();
        c.apply(&self.def_char);
        p.apply(&self.def_para);
        for s in chain.iter().rev() {
            c.apply(&s.ch);
            p.apply(&s.para);
        }
        (c, p)
    }

    pub fn outline_level(&self, name: &str) -> Option<u8> {
        let mut cur = self.get(name);
        let mut guard = 0;
        while let Some(s) = cur {
            if s.outline.is_some() {
                return s.outline;
            }
            guard += 1;
            if guard > 12 {
                break;
            }
            cur = s.based_on.as_deref().and_then(|b| self.get(b));
        }
        None
    }
}

// ---------------------------------------------------------------- document

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    pub w: f32,
    pub h: f32,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    pub header_dist: f32,
    pub footer_dist: f32,
    pub diff_first: bool,
}

impl Default for PageSetup {
    fn default() -> Self {
        PageSetup::letter()
    }
}

impl PageSetup {
    pub fn letter() -> PageSetup {
        PageSetup { w: 612.0, h: 792.0, top: 72.0, bottom: 72.0, left: 72.0, right: 72.0, header_dist: 36.0, footer_dist: 36.0, diff_first: false }
    }
    pub fn content_w(&self) -> f32 {
        (self.w - self.left - self.right).max(36.0)
    }
    pub fn content_h(&self) -> f32 {
        (self.h - self.top - self.bottom).max(36.0)
    }
    pub fn landscape(&self) -> bool {
        self.w > self.h
    }
}

pub const PAPER_SIZES: [(&str, f32, f32); 8] = [
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("A4", 595.3, 841.9),
    ("A3", 841.9, 1190.6),
    ("A5", 419.5, 595.3),
    ("B5", 498.9, 708.7),
    ("Executive", 522.0, 756.0),
    ("Tabloid", 792.0, 1224.0),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoryKind {
    Body,
    Header,
    Footer,
    FirstHeader,
    FirstFooter,
}

#[derive(Clone, Debug, Default)]
pub struct Meta {
    pub title: String,
    pub author: String,
}

#[derive(Clone)]
pub struct Document {
    pub body: Story,
    pub header: Story,
    pub footer: Story,
    pub first_header: Story,
    pub first_footer: Story,
    pub page: PageSetup,
    pub styles: Styles,
    pub meta: Meta,
    pub styles_ver: u64,
}

impl Document {
    pub fn new() -> Document {
        let hf = |style: &str| {
            let mut s = Story::new();
            s.para_mut(0).style = style.to_string();
            s
        };
        Document {
            body: Story::new(),
            header: hf("Header"),
            footer: hf("Footer"),
            first_header: hf("Header"),
            first_footer: hf("Footer"),
            page: PageSetup::letter(),
            styles: Styles::word_defaults(),
            meta: Meta::default(),
            styles_ver: next_rev(),
        }
    }
    pub fn story(&self, k: StoryKind) -> &Story {
        match k {
            StoryKind::Body => &self.body,
            StoryKind::Header => &self.header,
            StoryKind::Footer => &self.footer,
            StoryKind::FirstHeader => &self.first_header,
            StoryKind::FirstFooter => &self.first_footer,
        }
    }
    pub fn story_mut(&mut self, k: StoryKind) -> &mut Story {
        match k {
            StoryKind::Body => &mut self.body,
            StoryKind::Header => &mut self.header,
            StoryKind::Footer => &mut self.footer,
            StoryKind::FirstHeader => &mut self.first_header,
            StoryKind::FirstFooter => &mut self.first_footer,
        }
    }
    pub fn touch_styles(&mut self) {
        self.styles_ver = next_rev();
    }
}

impl Default for Document {
    fn default() -> Self {
        Document::new()
    }
}

/// Position in a story: flat paragraph index + char offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Pos {
    pub p: usize,
    pub o: usize,
}

impl Pos {
    pub fn new(p: usize, o: usize) -> Pos {
        Pos { p, o }
    }
}

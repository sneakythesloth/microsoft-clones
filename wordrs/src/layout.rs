//! Paragraph layout (line breaking, tabs, justification, inline objects) and pagination.
//! Everything is computed in points and is independent of zoom and of the GUI toolkit,
//! so the same layout drives the screen, PDF export and the tests.

use std::collections::HashMap;
use std::sync::Arc;

use unicode_linebreak::{linebreaks, BreakOpportunity};

use crate::fonts::{FontStore, Resolved};
use crate::model::*;
use crate::story::Story;

pub const LINK_COLOR: Rgb = [5, 99, 193];
const SCRIPT_SCALE: f32 = 0.65;
const SMALL_CAPS_SCALE: f32 = 0.8;
const CELL_VPAD: f32 = 1.0;

// ------------------------------------------------------------------ style cache

pub struct StyleCache {
    map: HashMap<String, (RChar, RPara)>,
    pub ver: u64,
}

impl StyleCache {
    pub fn build(styles: &Styles, ver: u64) -> StyleCache {
        let mut map = HashMap::new();
        for s in &styles.list {
            map.insert(s.name.clone(), styles.resolve(&s.name));
        }
        map.entry("Normal".to_string()).or_insert_with(|| styles.resolve("Normal"));
        StyleCache { map, ver }
    }
    pub fn get(&self, name: &str) -> &(RChar, RPara) {
        self.map.get(name).unwrap_or_else(|| &self.map["Normal"])
    }
}

// ------------------------------------------------------------------ layout types

pub struct FmtInfo {
    pub r: RChar,
    pub res: Resolved,
    pub link: Option<String>,
}

pub enum ItemKind {
    Text,
    Space,
    Tab(Leader),
    Image(ImageObj),
    Field(String),
    Break,
}

pub struct Item {
    pub kind: ItemKind,
    pub start: usize,
    pub end: usize,
    pub x: f32,
    pub w: f32,
    pub fi: u32,
    pub text: String,
    /// Effective font size in points (after super/subscript and small caps scaling).
    pub size: f32,
    /// Baseline shift in points (positive = up).
    pub rise: f32,
    /// Char x-offsets relative to `x` (len = chars + 1) for Text/Space items.
    pub cum: Vec<f32>,
}

pub struct Line {
    pub start: usize,
    pub end: usize,
    pub caret_end: usize,
    pub top: f32,
    pub h: f32,
    pub base: f32,
    pub x0: f32,
    pub w: f32,
    pub items: Vec<Item>,
    pub last: bool,
    pub brk: bool,
    pub avail: f32,
}

impl Line {
    /// X of the caret at char offset `off`, relative to the paragraph container's left edge.
    pub fn x_at(&self, off: usize) -> f32 {
        let mut last_end = self.x0;
        for it in &self.items {
            if off <= it.start {
                return self.x0 + it.x;
            }
            if off < it.end {
                return match it.kind {
                    ItemKind::Text | ItemKind::Space => self.x0 + it.x + it.cum[off - it.start],
                    _ => self.x0 + it.x,
                };
            }
            last_end = self.x0 + it.x + it.w;
        }
        last_end
    }

    /// Char offset nearest to `x` (relative to the container's left edge).
    pub fn offset_at(&self, x: f32) -> usize {
        let x = x - self.x0;
        for it in &self.items {
            if x < it.x {
                return it.start.min(self.caret_end);
            }
            if x < it.x + it.w {
                match it.kind {
                    ItemKind::Text | ItemKind::Space => {
                        let n = it.end - it.start;
                        for i in 0..n {
                            if x < it.x + (it.cum[i] + it.cum[i + 1]) * 0.5 {
                                return (it.start + i).min(self.caret_end);
                            }
                        }
                        return it.end.min(self.caret_end);
                    }
                    ItemKind::Break => return it.start.min(self.caret_end),
                    _ => {
                        return if x < it.x + it.w * 0.5 { it.start } else { it.end }.min(self.caret_end);
                    }
                }
            }
        }
        self.caret_end
    }
}

pub struct ParaLayout {
    pub lines: Vec<Line>,
    pub h: f32,
    pub fmts: Vec<FmtInfo>,
    pub rp: RPara,
    pub mark: RChar,
    pub mark_res: Resolved,
    pub width: f32,
    pub marker_x: f32,
    pub len: usize,
}

impl ParaLayout {
    pub fn line_for(&self, off: usize) -> usize {
        for (i, l) in self.lines.iter().enumerate() {
            if off < l.end {
                return i;
            }
        }
        self.lines.len() - 1
    }

    /// (ascent, descent) of the text at `off`, used to size the caret.
    pub fn caret_extent(&self, fonts: &FontStore, line: usize, off: usize) -> (f32, f32) {
        let l = &self.lines[line];
        let mut chosen: Option<&Item> = None;
        for it in &l.items {
            if matches!(it.kind, ItemKind::Break) {
                continue;
            }
            if it.start < off || chosen.is_none() {
                chosen = Some(it);
            }
            if it.end >= off {
                break;
            }
        }
        match chosen {
            Some(it) => match &it.kind {
                ItemKind::Image(img) => (img.h.min(l.base), 0.0),
                _ => {
                    let f = &fonts.faces[self.fmts[it.fi as usize].res.face];
                    (f.asc * it.size, (f.desc + f.gap) * it.size)
                }
            },
            None => {
                let f = &fonts.faces[self.mark_res.face];
                (f.asc * self.mark.size, (f.desc + f.gap) * self.mark.size)
            }
        }
    }
}

fn eff_size(r: &RChar) -> f32 {
    if r.vert == VertAlign::Baseline {
        r.size
    } else {
        r.size * SCRIPT_SCALE
    }
}

fn upper1(c: char) -> char {
    let mut u = c.to_uppercase();
    match (u.next(), u.next()) {
        (Some(x), None) => x,
        _ => c,
    }
}

fn transform(c: char, r: &RChar) -> (char, f32) {
    if r.all_caps {
        return (upper1(c), 1.0);
    }
    if r.small_caps && c.is_lowercase() {
        return (upper1(c), SMALL_CAPS_SCALE);
    }
    (c, 1.0)
}

fn tab_width(rp: &RPara, abs_x: f32, follow: f32) -> (f32, Leader) {
    let mut stops: Vec<TabStop> = rp.tabs.clone();
    stops.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap_or(std::cmp::Ordering::Equal));
    for st in &stops {
        let target = match st.kind {
            TabKind::Left => st.pos,
            TabKind::Center => st.pos - follow / 2.0,
            TabKind::Right => st.pos - follow,
        };
        if st.pos > abs_x + 0.5 && target >= abs_x - 0.01 {
            return ((target - abs_x).max(0.0), st.leader);
        }
    }
    let step = 36.0;
    let next = ((abs_x + 0.5) / step).floor() + 1.0;
    (next * step - abs_x, Leader::None)
}

struct ObjInfo {
    obj: Obj,
    w: f32,
    h: f32,
    text: String,
}

struct LR {
    s: usize,
    e: usize,
    brk: bool,
}

pub fn layout_para(fonts: &mut FontStore, p: &Paragraph, sc: &StyleCache, width: f32, in_cell: bool, fctx: Option<(usize, usize)>) -> ParaLayout {
    let (base_c, base_p) = sc.get(&p.style).clone();
    let mut rp = base_p;
    rp.apply(&p.fmt);
    if in_cell && p.style == "Normal" {
        if p.fmt.before.is_none() {
            rp.before = 0.0;
        }
        if p.fmt.after.is_none() {
            rp.after = 0.0;
        }
        if p.fmt.line.is_none() {
            rp.line = LineSpacing::Multiple(1.0);
        }
    }
    if let Some(li) = p.list {
        if p.fmt.left.is_none() {
            rp.left = 36.0 * (li.level as f32 + 1.0);
        }
        if p.fmt.first.is_none() {
            rp.first = -18.0;
        }
    }

    // ---- resolve run formats (needs &mut for lazy font loading)
    let mut fmts: Vec<FmtInfo> = Vec::with_capacity(p.runs.len().max(1));
    for run in &p.runs {
        let mut r = base_c.clone();
        r.apply(&run.fmt);
        if run.link.is_some() {
            if run.fmt.color.is_none() {
                r.color = LINK_COLOR;
            }
            if run.fmt.underline.is_none() {
                r.underline = true;
            }
        }
        let res = fonts.resolve(&r.font, r.bold, r.italic);
        fmts.push(FmtInfo { r, res, link: run.link.clone() });
    }
    let (mark, mark_res) = match fmts.last() {
        Some(l) => (l.r.clone(), l.res),
        None => (base_c.clone(), fonts.resolve(&base_c.font, base_c.bold, base_c.italic)),
    };
    if fmts.is_empty() {
        fmts.push(FmtInfo { r: mark.clone(), res: mark_res, link: None });
    }
    let fonts: &FontStore = fonts;

    // ---- flatten
    let max_w = (width - rp.left - rp.right).max(20.0);
    let mut chars: Vec<char> = vec![];
    let mut cf: Vec<u32> = vec![];
    let mut objs: HashMap<usize, ObjInfo> = HashMap::new();
    for (ri, run) in p.runs.iter().enumerate() {
        for ch in run.text.chars() {
            chars.push(ch);
            cf.push(ri as u32);
        }
        if let Some(o) = &run.obj {
            let idx = chars.len().saturating_sub(1);
            let info = &fmts[ri];
            let size = eff_size(&info.r);
            match o {
                Obj::Image(img) => {
                    let (mut w, mut h) = (img.w.max(1.0), img.h.max(1.0));
                    if w > max_w {
                        let s = max_w / w;
                        w *= s;
                        h *= s;
                    }
                    objs.insert(idx, ObjInfo { obj: o.clone(), w, h, text: String::new() });
                }
                Obj::Field(f) => {
                    let (pg, np) = fctx.unwrap_or((1, 1));
                    let text = match f {
                        Field::Page => pg.to_string(),
                        Field::NumPages => np.to_string(),
                    };
                    let w = fonts.text_width_em(info.res.face, &text) * size;
                    objs.insert(idx, ObjInfo { obj: o.clone(), w, h: 0.0, text });
                }
            }
        }
    }
    let n = chars.len();
    let mut adv = vec![0f32; n];
    let mut tch = chars.clone();
    let mut tsize = vec![0f32; n];
    let mut small = vec![false; n];
    for i in 0..n {
        let info = &fmts[cf[i] as usize];
        let size = eff_size(&info.r);
        tsize[i] = size;
        if let Some(o) = objs.get(&i) {
            adv[i] = o.w;
            continue;
        }
        let c = chars[i];
        if c == '\t' || c == '\n' || c == PAGE_BREAK {
            continue;
        }
        let (tc, scl) = transform(c, &info.r);
        let s = size * scl;
        tch[i] = tc;
        tsize[i] = s;
        small[i] = scl != 1.0;
        adv[i] = fonts.glyph_adv(info.res.face, tc) * s + info.r.spacing;
    }

    // ---- break opportunities -> segments
    let mut segs: Vec<(usize, usize)> = vec![];
    if n > 0 {
        let text: String = chars.iter().collect();
        let mut b2c = vec![0usize; text.len() + 1];
        for (ci, (bi, ch)) in text.char_indices().enumerate() {
            for k in 0..ch.len_utf8() {
                b2c[bi + k] = ci;
            }
        }
        b2c[text.len()] = n;
        let mut s = 0;
        for (b, op) in linebreaks(&text) {
            let c = b2c[b.min(text.len())];
            let _ = op == BreakOpportunity::Mandatory;
            if c > s {
                segs.push((s, c));
                s = c;
            }
        }
        if s < n {
            segs.push((s, n));
        }
    }
    let mut pre = vec![0f32; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i] + adv[i];
    }

    // ---- fit lines
    // For list items the marker occupies the hanging area; the text itself starts at the indent.
    let text_first = if p.list.is_some() { rp.first.max(0.0) } else { rp.first };
    let origin_of = |first: bool| -> f32 { (rp.left + if first { text_first } else { 0.0 }).max(0.0) };
    let avail_of = |first: bool| -> f32 { (width - rp.right - origin_of(first)).max(12.0) };
    let is_trailing = |c: char| c == ' ' || c == '\t' || c == '\n' || c == PAGE_BREAK;
    let mut ranges: Vec<LR> = vec![];
    let mut leaders: HashMap<usize, Leader> = HashMap::new();
    let mut cur = 0usize;
    let mut x = 0f32;
    let mut first = true;
    let mut avail = avail_of(true);
    for &(s, e) in &segs {
        let mut vis_end = e;
        while vis_end > s && is_trailing(chars[vis_end - 1]) {
            vis_end -= 1;
        }
        let vis_w = pre[vis_end] - pre[s];
        if s > cur && x + vis_w > avail + 0.05 {
            ranges.push(LR { s: cur, e: s, brk: false });
            cur = s;
            x = 0.0;
            first = false;
            avail = avail_of(false);
        }
        if vis_w > avail - x + 0.05 {
            let mut k = s;
            while k < vis_end {
                let w = adv[k];
                if k > cur && x + w > avail + 0.05 {
                    ranges.push(LR { s: cur, e: k, brk: false });
                    cur = k;
                    x = 0.0;
                    first = false;
                    avail = avail_of(false);
                }
                x += w;
                k += 1;
            }
        } else {
            x += vis_w;
        }
        for k in vis_end..e {
            match chars[k] {
                ' ' => x += adv[k],
                '\t' => {
                    let mut follow = 0.0;
                    let mut j = k + 1;
                    while j < n && chars[j] != '\t' && chars[j] != '\n' && chars[j] != PAGE_BREAK {
                        follow += adv[j];
                        j += 1;
                    }
                    let (mut w, leader) = tab_width(&rp, origin_of(first) + x, follow);
                    leaders.insert(k, leader);
                    let right_abs = origin_of(first) + avail;
                    if origin_of(first) + x + w > right_abs + 0.5 {
                        w = (right_abs - origin_of(first) - x).max(0.0);
                    }
                    adv[k] = w;
                    x += w;
                }
                '\n' | PAGE_BREAK => {
                    ranges.push(LR { s: cur, e: k + 1, brk: true });
                    cur = k + 1;
                    x = 0.0;
                    first = false;
                    avail = avail_of(false);
                }
                _ => {}
            }
        }
    }
    ranges.push(LR { s: cur, e: n, brk: false });

    // ---- build lines
    let mut lines: Vec<Line> = Vec::with_capacity(ranges.len());
    let mut top = 0f32;
    let nr = ranges.len();
    for (li, lr) in ranges.iter().enumerate() {
        let is_first = li == 0;
        let is_last = li == nr - 1;
        let (start, end) = (lr.s, lr.e);
        let mut vis_end = end;
        while vis_end > start && (chars[vis_end - 1] == ' ' || chars[vis_end - 1] == '\n' || chars[vis_end - 1] == PAGE_BREAK) {
            vis_end -= 1;
        }
        let content_w: f32 = (start..vis_end).map(|k| adv[k]).sum();
        let origin = origin_of(is_first);
        let avail = avail_of(is_first);
        let justify = rp.align == Align::Justify && !is_last && !lr.brk;
        let mut per_space = 0f32;
        if justify {
            let n_sp = (start..vis_end).filter(|&k| chars[k] == ' ').count();
            if n_sp > 0 {
                per_space = (avail - content_w).max(0.0) / n_sp as f32;
            }
        }
        let x_off = match rp.align {
            Align::Center => ((avail - content_w) / 2.0).max(0.0),
            Align::Right => (avail - content_w).max(0.0),
            _ => 0.0,
        };
        // items
        let mut items: Vec<Item> = vec![];
        let mut xr = x_off;
        let mut k = start;
        while k < end {
            let c = chars[k];
            let fi = cf[k];
            let info = &fmts[fi as usize];
            let rise = match info.r.vert {
                VertAlign::Baseline => 0.0,
                VertAlign::Super => info.r.size * 0.35,
                VertAlign::Sub => -info.r.size * 0.12,
            };
            if let Some(o) = objs.get(&k) {
                let (kind, w) = match &o.obj {
                    Obj::Image(img) => (ItemKind::Image(ImageObj { data: img.data.clone(), w: o.w, h: o.h }), o.w),
                    Obj::Field(_) => (ItemKind::Field(o.text.clone()), o.w),
                };
                items.push(Item { kind, start: k, end: k + 1, x: xr, w, fi, text: o.text.clone(), size: tsize[k], rise, cum: vec![] });
                xr += w;
                k += 1;
                continue;
            }
            match c {
                '\t' => {
                    let leader = leaders.get(&k).copied().unwrap_or(Leader::None);
                    items.push(Item { kind: ItemKind::Tab(leader), start: k, end: k + 1, x: xr, w: adv[k], fi, text: String::new(), size: tsize[k], rise, cum: vec![] });
                    xr += adv[k];
                    k += 1;
                }
                '\n' | PAGE_BREAK => {
                    items.push(Item { kind: ItemKind::Break, start: k, end: k + 1, x: xr, w: 0.0, fi, text: c.to_string(), size: tsize[k], rise, cum: vec![] });
                    k += 1;
                }
                _ => {
                    let is_space = c == ' ';
                    let mut j = k;
                    let mut text = String::new();
                    let mut cum = vec![0f32];
                    while j < end && cf[j] == fi && !objs.contains_key(&j) && small[j] == small[k] && ((chars[j] == ' ') == is_space) && chars[j] != '\t' && chars[j] != '\n' && chars[j] != PAGE_BREAK {
                        let mut a = adv[j];
                        if is_space && j < vis_end {
                            a += per_space;
                        }
                        text.push(tch[j]);
                        let l = *cum.last().unwrap();
                        cum.push(l + a);
                        j += 1;
                    }
                    let w = *cum.last().unwrap();
                    items.push(Item { kind: if is_space { ItemKind::Space } else { ItemKind::Text }, start: k, end: j, x: xr, w, fi, text, size: tsize[k], rise, cum });
                    xr += w;
                    k = j;
                }
            }
        }
        // vertical metrics
        let mut asc = 0f32;
        let mut desc = 0f32;
        for it in &items {
            match &it.kind {
                ItemKind::Image(img) => {
                    asc = asc.max(img.h + it.rise.max(0.0));
                }
                _ => {
                    let f = &fonts.faces[fmts[it.fi as usize].res.face];
                    asc = asc.max(f.asc * it.size + it.rise.max(0.0));
                    desc = desc.max((f.desc + f.gap) * it.size + (-it.rise).max(0.0));
                }
            }
        }
        if items.is_empty() {
            let f = &fonts.faces[mark_res.face];
            asc = f.asc * mark.size;
            desc = (f.desc + f.gap) * mark.size;
        }
        let natural = asc + desc;
        let (h, base) = match rp.line {
            LineSpacing::Multiple(m) => {
                let m = m.max(0.1);
                if m >= 1.0 {
                    (natural * m, asc + natural * (m - 1.0))
                } else {
                    (natural * m, asc * m)
                }
            }
            LineSpacing::Exact(v) => (v.max(1.0), v.max(1.0) * 0.8),
            LineSpacing::AtLeast(v) => {
                let hh = natural.max(v);
                (hh, asc + (hh - natural))
            }
        };
        let trailing_sp = end - vis_end;
        let caret_end = if lr.brk {
            end.saturating_sub(1).max(start)
        } else if is_last {
            end
        } else {
            end.saturating_sub(trailing_sp).max(start)
        };
        lines.push(Line { start, end, caret_end, top, h, base, x0: origin, w: if justify { avail } else { content_w }, items, last: is_last, brk: lr.brk, avail });
        top += h;
    }
    let marker_x = (rp.left + rp.first).max(0.0);
    ParaLayout { lines, h: top, fmts, rp, mark, mark_res, width, marker_x, len: n }
}

// ------------------------------------------------------------------ list numbering

fn alpha(n: u32, upper: bool) -> String {
    let n = n.max(1);
    let c = (((n - 1) % 26) as u8 + if upper { b'A' } else { b'a' }) as char;
    std::iter::repeat(c).take(((n - 1) / 26 + 1) as usize).collect()
}

fn roman(mut n: u32, upper: bool) -> String {
    let t = [(1000, "m"), (900, "cm"), (500, "d"), (400, "cd"), (100, "c"), (90, "xc"), (50, "l"), (40, "xl"), (10, "x"), (9, "ix"), (5, "v"), (4, "iv"), (1, "i")];
    let mut s = String::new();
    for (v, r) in t {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    if upper {
        s.to_uppercase()
    } else {
        s
    }
}

pub fn marker_text(kind: ListKind, level: usize, n: u32) -> String {
    match kind {
        ListKind::Bullet => ["•", "o", "▪"][level % 3].to_string(),
        ListKind::Decimal => format!("{n}."),
        ListKind::LowerAlpha => format!("{}.", alpha(n, false)),
        ListKind::UpperAlpha => format!("{}.", alpha(n, true)),
        ListKind::LowerRoman => format!("{}.", roman(n, false)),
        ListKind::UpperRoman => format!("{}.", roman(n, true)),
    }
}

pub fn compute_markers(story: &Story) -> Vec<Option<String>> {
    let mut counters: HashMap<u32, [u32; 9]> = HashMap::new();
    let mut out = vec![None; story.len()];
    for i in 0..story.len() {
        if let Some(li) = story.para(i).list {
            let lv = (li.level as usize).min(8);
            let c = counters.entry(li.id).or_insert([0; 9]);
            c[lv] += 1;
            for d in lv + 1..9 {
                c[d] = 0;
            }
            out[i] = Some(marker_text(li.kind, lv, c[lv]));
        }
    }
    out
}

// ------------------------------------------------------------------ pagination types

pub struct PlacedLine {
    pub para: usize,
    pub line: usize,
    /// Container left edge in page coordinates.
    pub x: f32,
    /// Top of the line in page coordinates.
    pub y: f32,
    pub w: f32,
    pub layout: Arc<ParaLayout>,
    pub marker: Option<String>,
}

pub struct Fill {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub color: Rgb,
}

pub struct Rule {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub w: f32,
    pub color: Rgb,
}

pub struct CellBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub b: usize,
    pub r: usize,
    pub c: usize,
}

pub struct Page {
    pub setup: PageSetup,
    pub body: Vec<PlacedLine>,
    pub fills: Vec<Fill>,
    pub rules: Vec<Rule>,
    pub cells: Vec<CellBox>,
    pub header: Vec<PlacedLine>,
    pub footer: Vec<PlacedLine>,
    pub header_kind: StoryKind,
    pub footer_kind: StoryKind,
    pub top: f32,
    pub content_h: f32,
}

pub struct Pagination {
    pub pages: Vec<Page>,
    pub index: HashMap<usize, Vec<(usize, usize)>>,
    pub markers: Vec<Option<String>>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    rev: u64,
    width: u32,
    in_cell: bool,
    fctx: Option<(usize, usize)>,
    styles: u64,
    fonts: u64,
}

#[derive(Default)]
pub struct LayoutEngine {
    cache: HashMap<Key, (Arc<ParaLayout>, u64)>,
    pass: u64,
}

impl LayoutEngine {
    pub fn para(&mut self, fonts: &mut FontStore, p: &Paragraph, sc: &StyleCache, width: f32, in_cell: bool, fctx: Option<(usize, usize)>) -> Arc<ParaLayout> {
        let key = Key { rev: p.rev, width: width.to_bits(), in_cell, fctx, styles: sc.ver, fonts: fonts.generation };
        if let Some(e) = self.cache.get_mut(&key) {
            e.1 = self.pass;
            return e.0.clone();
        }
        let lay = Arc::new(layout_para(fonts, p, sc, width, in_cell, fctx));
        self.cache.insert(key, (lay.clone(), self.pass));
        lay
    }

    fn gc(&mut self) {
        if self.cache.len() > 6000 {
            let pass = self.pass;
            self.cache.retain(|_, v| v.1 + 1 >= pass);
        }
    }

    fn story_height(&mut self, fonts: &mut FontStore, story: &Story, sc: &StyleCache, width: f32) -> f32 {
        let mut h = 0.0;
        for i in 0..story.len() {
            let lay = self.para(fonts, story.para(i), sc, width, false, Some((1, 1)));
            h += lay.rp.before + lay.h + lay.rp.after;
        }
        h
    }

    fn place_hf(&mut self, fonts: &mut FontStore, story: &Story, sc: &StyleCache, setup: &PageSetup, footer: bool, fctx: (usize, usize)) -> Vec<PlacedLine> {
        let cw = setup.content_w();
        let mut lays = vec![];
        let mut total = 0.0;
        for i in 0..story.len() {
            let lay = self.para(fonts, story.para(i), sc, cw, false, Some(fctx));
            total += lay.rp.before + lay.h + lay.rp.after;
            lays.push(lay);
        }
        let mut y = if footer { setup.h - setup.footer_dist - total } else { setup.header_dist };
        let mut out = vec![];
        for (i, lay) in lays.into_iter().enumerate() {
            y += lay.rp.before;
            for li in 0..lay.lines.len() {
                let ly = y + lay.lines[li].top;
                out.push(PlacedLine { para: i, line: li, x: setup.left, y: ly, w: cw, layout: lay.clone(), marker: None });
            }
            y += lay.h + lay.rp.after;
        }
        out
    }

    pub fn paginate(&mut self, fonts: &mut FontStore, doc: &Document, sc: &StyleCache) -> Pagination {
        self.pass += 1;
        self.gc();
        let setup = doc.page;
        let cw = setup.content_w();
        // Header / footer heights decide the body area.
        let hdr_h = |s: &mut LayoutEngine, f: &mut FontStore, st: &Story| s.story_height(f, st, sc, cw);
        let (h_def, f_def) = (hdr_h(self, fonts, &doc.header), hdr_h(self, fonts, &doc.footer));
        let (h_first, f_first) = if setup.diff_first { (hdr_h(self, fonts, &doc.first_header), hdr_h(self, fonts, &doc.first_footer)) } else { (h_def, f_def) };
        let geom = |first: bool| -> (f32, f32) {
            let (hh, ff) = if first && setup.diff_first { (h_first, f_first) } else { (h_def, f_def) };
            let top = setup.top.max(setup.header_dist + hh + 4.0).min(setup.h * 0.5);
            let bottom_margin = setup.bottom.max(setup.footer_dist + ff + 4.0).min(setup.h * 0.5);
            (top, (setup.h - top - bottom_margin).max(36.0))
        };
        let story = &doc.body;
        let markers = compute_markers(story);
        let mut pages: Vec<Page> = vec![];
        let new_page = |pages: &mut Vec<Page>| {
            let (top, ch) = geom(pages.is_empty());
            let first = pages.is_empty();
            pages.push(Page {
                setup,
                body: vec![],
                fills: vec![],
                rules: vec![],
                cells: vec![],
                header: vec![],
                footer: vec![],
                header_kind: if first && setup.diff_first { StoryKind::FirstHeader } else { StoryKind::Header },
                footer_kind: if first && setup.diff_first { StoryKind::FirstFooter } else { StoryKind::Footer },
                top,
                content_h: ch,
            });
        };
        new_page(&mut pages);
        let mut y = 0f32;
        let mut flat = 0usize;
        let nblocks = story.blocks.len();
        for bi in 0..nblocks {
            match &*story.blocks[bi] {
                Block::Para(p) => {
                    let lay = self.para(fonts, p, sc, cw, false, None);
                    let rp = lay.rp.clone();
                    if rp.page_break_before && y > 0.0 {
                        new_page(&mut pages);
                        y = 0.0;
                    }
                    let mut before = rp.before;
                    let first_of_doc = bi == 0 && pages.len() == 1;
                    if y == 0.0 && !first_of_doc {
                        before = 0.0;
                    }
                    // keep with next
                    if rp.keep_next && y > 0.0 {
                        let next_h = match story.blocks.get(bi + 1).map(|b| &**b) {
                            Some(Block::Para(np)) => {
                                let nl = self.para(fonts, np, sc, cw, false, None);
                                nl.lines.first().map(|l| l.h).unwrap_or(0.0)
                            }
                            Some(Block::Table(t)) => t.rows.first().and_then(|r| r.height).unwrap_or(20.0),
                            None => 0.0,
                        };
                        let ch = pages.last().unwrap().content_h;
                        if y + before + lay.h + next_h > ch && before + lay.h <= ch {
                            new_page(&mut pages);
                            y = 0.0;
                            before = 0.0;
                        }
                    }
                    let total = lay.lines.len();
                    let mut i = 0;
                    while i < total {
                        let (ptop, ch) = {
                            let pg = pages.last().unwrap();
                            (pg.top, pg.content_h)
                        };
                        let lead = if i == 0 { before } else { 0.0 };
                        let mut n = 0;
                        let mut hsum = lead;
                        while i + n < total && y + hsum + lay.lines[i + n].h <= ch + 0.01 {
                            hsum += lay.lines[i + n].h;
                            n += 1;
                        }
                        let remaining = total - i - n;
                        if remaining > 0 {
                            if i == 0 && ((n < 2 && total >= 2) || rp.keep_lines) {
                                n = 0;
                            } else if remaining == 1 && n >= 3 {
                                n -= 1;
                            }
                        }
                        if n == 0 {
                            if y > 0.0 {
                                new_page(&mut pages);
                                y = 0.0;
                                if i == 0 {
                                    before = if first_of_doc { before } else { 0.0 };
                                }
                                continue;
                            } else {
                                n = 1;
                            }
                        }
                        let pg_idx = pages.len() - 1;
                        let pg = pages.last_mut().unwrap();
                        let mut yy = ptop + y + lead;
                        let seg_top = yy;
                        for k in 0..n {
                            let li = i + k;
                            let line = &lay.lines[li];
                            pg.body.push(PlacedLine {
                                para: flat,
                                line: li,
                                x: setup.left,
                                y: yy,
                                w: cw,
                                layout: lay.clone(),
                                marker: if li == 0 { markers[flat].clone() } else { None },
                            });
                            yy += line.h;
                        }
                        let _ = pg_idx;
                        // shading and borders for this segment
                        let left_edge = setup.left + (rp.left + rp.first.min(0.0)).max(0.0);
                        let box_w = (cw - (left_edge - setup.left) - rp.right).max(1.0);
                        if let Some(c) = rp.shade {
                            pg.fills.push(Fill { x: left_edge, y: seg_top, w: box_w, h: yy - seg_top, color: c });
                        }
                        if let Some(b) = rp.borders.top {
                            if i == 0 {
                                pg.rules.push(Rule { x0: left_edge, y0: seg_top - 1.0, x1: left_edge + box_w, y1: seg_top - 1.0, w: b.w, color: b.color });
                            }
                        }
                        if let Some(b) = rp.borders.bottom {
                            if i + n == total {
                                pg.rules.push(Rule { x0: left_edge, y0: yy + 1.0, x1: left_edge + box_w, y1: yy + 1.0, w: b.w, color: b.color });
                            }
                        }
                        if let Some(b) = rp.borders.left {
                            pg.rules.push(Rule { x0: left_edge - 3.0, y0: seg_top, x1: left_edge - 3.0, y1: yy, w: b.w, color: b.color });
                        }
                        if let Some(b) = rp.borders.right {
                            pg.rules.push(Rule { x0: left_edge + box_w + 3.0, y0: seg_top, x1: left_edge + box_w + 3.0, y1: yy, w: b.w, color: b.color });
                        }
                        y = yy - ptop;
                        i += n;
                        if i < total {
                            new_page(&mut pages);
                            y = 0.0;
                        }
                    }
                    y += rp.after;
                    if p.runs.last().map(|r| r.text.ends_with(PAGE_BREAK)).unwrap_or(false) {
                        new_page(&mut pages);
                        y = 0.0;
                    }
                    flat += 1;
                }
                Block::Table(t) => {
                    flat = self.place_table(fonts, sc, &mut pages, &new_page, &mut y, flat, bi, t, &setup, &markers);
                }
            }
        }
        // headers and footers (need the final page count)
        let np = pages.len();
        for (pi, pg) in pages.iter_mut().enumerate() {
            let hs = doc.story(pg.header_kind);
            let fs = doc.story(pg.footer_kind);
            let hs = hs.clone();
            let fs = fs.clone();
            pg.header = self.place_hf(fonts, &hs, sc, &setup, false, (pi + 1, np));
            pg.footer = self.place_hf(fonts, &fs, sc, &setup, true, (pi + 1, np));
        }
        let mut index: HashMap<usize, Vec<(usize, usize)>> = HashMap::new();
        for (pi, pg) in pages.iter().enumerate() {
            for (li, pl) in pg.body.iter().enumerate() {
                index.entry(pl.para).or_default().push((pi, li));
            }
        }
        Pagination { pages, index, markers }
    }

    #[allow(clippy::too_many_arguments)]
    fn place_table(&mut self, fonts: &mut FontStore, sc: &StyleCache, pages: &mut Vec<Page>, new_page: &dyn Fn(&mut Vec<Page>), y: &mut f32, flat0: usize, bi: usize, t: &Table, setup: &PageSetup, markers: &[Option<String>]) -> usize {
        let cw = setup.content_w();
        let tw = t.width();
        let scale = if tw > cw { cw / tw } else { 1.0 };
        let cols: Vec<f32> = t.cols.iter().map(|w| w * scale).collect();
        let mut colx = vec![0f32; cols.len() + 1];
        for i in 0..cols.len() {
            colx[i + 1] = colx[i] + cols[i];
        }
        let x_off = match t.align {
            Align::Center => (cw - colx[cols.len()]) / 2.0,
            Align::Right => cw - colx[cols.len()],
            _ => 0.0,
        };
        let pad = t.pad_lr;
        let nrows = t.rows.len();
        let ncols = cols.len();
        struct CellLay {
            r: usize,
            c: usize,
            rs: usize,
            cs: usize,
            paras: Vec<(Arc<ParaLayout>, usize)>,
            content_h: f32,
            w: f32,
        }
        let mut flat = flat0;
        let mut cells: Vec<CellLay> = vec![];
        for r in 0..nrows {
            for c in 0..ncols {
                let cell = &t.rows[r].cells[c];
                if cell.covered {
                    continue;
                }
                let cs = cell.cs.max(1).min(ncols - c);
                let rs = cell.rs.max(1).min(nrows - r);
                let w = colx[c + cs] - colx[c];
                let inner = (w - 2.0 * pad).max(10.0);
                let mut paras = vec![];
                let mut content_h = 0.0;
                for p in &cell.paras {
                    let lay = self.para(fonts, p, sc, inner, true, None);
                    content_h += lay.rp.before + lay.h + lay.rp.after;
                    paras.push((lay, flat));
                    flat += 1;
                }
                cells.push(CellLay { r, c, rs, cs, paras, content_h, w });
            }
        }
        let mut rh = vec![0f32; nrows];
        for cl in cells.iter().filter(|c| c.rs == 1) {
            rh[cl.r] = rh[cl.r].max(cl.content_h + 2.0 * CELL_VPAD);
        }
        for r in 0..nrows {
            if let Some(h) = t.rows[r].height {
                rh[r] = rh[r].max(h);
            }
            if rh[r] <= 0.0 {
                rh[r] = 14.0;
            }
        }
        for cl in cells.iter().filter(|c| c.rs > 1) {
            let have: f32 = rh[cl.r..cl.r + cl.rs].iter().sum();
            let need = cl.content_h + 2.0 * CELL_VPAD;
            if need > have {
                rh[cl.r + cl.rs - 1] += need - have;
            }
        }
        let header_rows = t.rows.iter().take_while(|r| r.header).count();
        let mut r = 0;
        while r < nrows {
            // rows that must stay together because of vertical merges
            let mut r_end = r + 1;
            loop {
                let mut ext = r_end;
                for cl in cells.iter().filter(|c| c.r >= r && c.r < r_end) {
                    ext = ext.max(cl.r + cl.rs);
                }
                if ext == r_end {
                    break;
                }
                r_end = ext;
            }
            let group_h: f32 = rh[r..r_end].iter().sum();
            let (mut ptop, mut ch) = {
                let pg = pages.last().unwrap();
                (pg.top, pg.content_h)
            };
            let mut start_r = r;
            if *y + group_h > ch + 0.01 && *y > 0.0 {
                new_page(pages);
                *y = 0.0;
                let pg = pages.last().unwrap();
                ptop = pg.top;
                ch = pg.content_h;
                let _ = ch;
                if header_rows > 0 && r >= header_rows {
                    start_r = 0; // repeat header rows first
                }
            }
            let mut place_rows: Vec<usize> = vec![];
            if start_r == 0 && r >= header_rows && header_rows > 0 {
                place_rows.extend(0..header_rows);
            }
            place_rows.extend(r..r_end);
            for rr in place_rows {
                let row_top = ptop + *y;
                let rh_here = rh[rr];
                for cl in cells.iter().filter(|c| c.r == rr) {
                    let cell = &t.rows[cl.r].cells[cl.c];
                    let cell_h: f32 = rh[cl.r..cl.r + cl.rs].iter().sum();
                    let cx = setup.left + x_off + colx[cl.c];
                    let pg = pages.last_mut().unwrap();
                    pg.cells.push(CellBox { x: cx, y: row_top, w: cl.w, h: cell_h, b: bi, r: cl.r, c: cl.c });
                    if let Some(c) = cell.shade {
                        pg.fills.push(Fill { x: cx, y: row_top, w: cl.w, h: cell_h, color: c });
                    }
                    let free = (cell_h - 2.0 * CELL_VPAD - cl.content_h).max(0.0);
                    let mut yy = row_top
                        + CELL_VPAD
                        + match cell.valign {
                            VAlign::Top => 0.0,
                            VAlign::Center => free / 2.0,
                            VAlign::Bottom => free,
                        };
                    for (lay, fi) in &cl.paras {
                        yy += lay.rp.before;
                        for li in 0..lay.lines.len() {
                            pg.body.push(PlacedLine {
                                para: *fi,
                                line: li,
                                x: cx + pad,
                                y: yy + lay.lines[li].top,
                                w: (cl.w - 2.0 * pad).max(10.0),
                                layout: lay.clone(),
                                marker: if li == 0 { markers[*fi].clone() } else { None },
                            });
                        }
                        yy += lay.h + lay.rp.after;
                    }
                    // borders
                    let b = &t.borders;
                    let top_b = if cl.r == 0 { b.outer } else { b.inner_h };
                    let bot_b = if cl.r + cl.rs >= nrows { b.outer } else { b.inner_h };
                    let left_b = if cl.c == 0 { b.outer } else { b.inner_v };
                    let right_b = if cl.c + cl.cs >= ncols { b.outer } else { b.inner_v };
                    let (x0, x1, y0, y1) = (cx, cx + cl.w, row_top, row_top + cell_h);
                    if let Some(bd) = top_b {
                        pg.rules.push(Rule { x0, y0, x1, y1: y0, w: bd.w, color: bd.color });
                    }
                    if let Some(bd) = bot_b {
                        pg.rules.push(Rule { x0, y0: y1, x1, y1, w: bd.w, color: bd.color });
                    }
                    if let Some(bd) = left_b {
                        pg.rules.push(Rule { x0, y0, x1: x0, y1, w: bd.w, color: bd.color });
                    }
                    if let Some(bd) = right_b {
                        pg.rules.push(Rule { x0: x1, y0, x1, y1, w: bd.w, color: bd.color });
                    }
                }
                *y += rh_here;
            }
            r = r_end;
        }
        flat
    }
}

// ------------------------------------------------------------------ queries

impl Pagination {
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Find the placed line that shows `pos` in the body story.
    pub fn locate_body(&self, pos: Pos) -> Option<(usize, usize)> {
        let list = self.index.get(&pos.p)?;
        for &(pg, idx) in list {
            let pl = &self.pages[pg].body[idx];
            let ln = &pl.layout.lines[pl.line];
            if pos.o < ln.end || pl.line + 1 == pl.layout.lines.len() {
                return Some((pg, idx));
            }
        }
        list.last().copied()
    }

    fn list_for(&self, kind: StoryKind, page: usize) -> &Vec<PlacedLine> {
        let pg = &self.pages[page.min(self.pages.len() - 1)];
        match kind {
            StoryKind::Body => &pg.body,
            StoryKind::Header | StoryKind::FirstHeader => &pg.header,
            StoryKind::Footer | StoryKind::FirstFooter => &pg.footer,
        }
    }

    /// The placed line that displays `pos` in the given story: (page, line).
    pub fn line_of(&self, kind: StoryKind, pos: Pos, page_hint: usize) -> Option<(usize, &PlacedLine)> {
        if kind == StoryKind::Body {
            let (pg, idx) = self.locate_body(pos)?;
            return Some((pg, &self.pages[pg].body[idx]));
        }
        let pgi = page_hint.min(self.pages.len() - 1);
        let list = self.list_for(kind, pgi);
        for pl in list.iter().filter(|pl| pl.para == pos.p) {
            let ln = &pl.layout.lines[pl.line];
            if pos.o < ln.end || pl.line + 1 == pl.layout.lines.len() {
                return Some((pgi, pl));
            }
        }
        None
    }

    /// Caret geometry: (page, x, y, height) in page coordinates.
    pub fn caret_rect(&self, fonts: &FontStore, kind: StoryKind, pos: Pos, page_hint: usize) -> Option<(usize, f32, f32, f32)> {
        let (pg, pl) = if kind == StoryKind::Body {
            let (pg, idx) = self.locate_body(pos)?;
            (pg, &self.pages[pg].body[idx])
        } else {
            let pgi = page_hint.min(self.pages.len() - 1);
            let list = self.list_for(kind, pgi);
            let mut found = None;
            for pl in list.iter().filter(|pl| pl.para == pos.p) {
                let ln = &pl.layout.lines[pl.line];
                if pos.o < ln.end || pl.line + 1 == pl.layout.lines.len() {
                    found = Some(pl);
                    break;
                }
            }
            (pgi, found?)
        };
        let ln = &pl.layout.lines[pl.line];
        let off = pos.o.clamp(ln.start, ln.end);
        let x = pl.x + ln.x_at(off);
        let (up, down) = pl.layout.caret_extent(fonts, pl.line, off);
        let base = pl.y + ln.base;
        Some((pg, x, base - up, up + down))
    }

    /// Nearest position to a page-space point within the given story.
    pub fn hit(&self, kind: StoryKind, page: usize, x: f32, y: f32) -> Option<Pos> {
        let list = self.list_for(kind, page);
        let mut best: Option<(f32, &PlacedLine)> = None;
        for pl in list {
            let ln = &pl.layout.lines[pl.line];
            let (top, bottom) = (pl.y, pl.y + ln.h);
            let dy = if y < top { top - y } else if y > bottom { y - bottom } else { 0.0 };
            let dx = if x < pl.x { pl.x - x } else if x > pl.x + pl.w { x - pl.x - pl.w } else { 0.0 };
            let d = dy * dy + dx * dx * 0.25;
            if best.map(|b| d < b.0).unwrap_or(true) {
                best = Some((d, pl));
            }
        }
        let (_, pl) = best?;
        let ln = &pl.layout.lines[pl.line];
        Some(Pos { p: pl.para, o: ln.offset_at(x - pl.x) })
    }

    pub fn page_of(&self, pos: Pos) -> usize {
        self.locate_body(pos).map(|(p, _)| p).unwrap_or(0)
    }

    /// Placed lines of a body paragraph in order.
    pub fn lines_of(&self, para: usize) -> Vec<&PlacedLine> {
        self.index.get(&para).map(|v| v.iter().map(|&(pg, idx)| &self.pages[pg].body[idx]).collect()).unwrap_or_default()
    }

    /// The placed line directly above/below `pl` (visually), for Up/Down arrows.
    pub fn neighbor_line(&self, kind: StoryKind, page: usize, para: usize, line: usize, down: bool, x: f32) -> Option<(usize, Pos)> {
        let list = self.list_for(kind, page);
        let cur = list.iter().find(|pl| pl.para == para && pl.line == line)?;
        let cy = cur.y + pl_h(cur) / 2.0;
        let mut best: Option<(f32, &PlacedLine)> = None;
        for pl in list {
            if std::ptr::eq(pl, cur) {
                continue;
            }
            let mid = pl.y + pl_h(pl) / 2.0;
            let dy = if down { mid - cy } else { cy - mid };
            if dy <= 1.0 {
                continue;
            }
            // prefer horizontally overlapping lines (table columns)
            let dx = if x < pl.x { pl.x - x } else if x > pl.x + pl.w { x - pl.x - pl.w } else { 0.0 };
            let score = dy + dx * 4.0;
            if best.map(|b| score < b.0).unwrap_or(true) {
                best = Some((score, pl));
            }
        }
        if let Some((_, pl)) = best {
            let ln = &pl.layout.lines[pl.line];
            return Some((page, Pos { p: pl.para, o: ln.offset_at(x - pl.x) }));
        }
        // continue on neighbouring page (body only)
        if kind == StoryKind::Body {
            let np = if down { page + 1 } else { page.wrapping_sub(1) };
            if np < self.pages.len() {
                let l = &self.pages[np].body;
                let pick = if down { l.first() } else { l.last() };
                if let Some(pl) = pick {
                    let ln = &pl.layout.lines[pl.line];
                    return Some((np, Pos { p: pl.para, o: ln.offset_at(x - pl.x) }));
                }
            }
        }
        None
    }
}

fn pl_h(pl: &PlacedLine) -> f32 {
    pl.layout.lines[pl.line].h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (FontStore, StyleCache) {
        let fonts = FontStore::new();
        let sc = StyleCache::build(&Styles::word_defaults(), 1);
        (fonts, sc)
    }

    #[test]
    fn linebreaks_semantics() {
        let v: Vec<(usize, BreakOpportunity)> = linebreaks("ab cd").collect();
        assert_eq!(v[0], (3, BreakOpportunity::Allowed));
        assert_eq!(v.last().unwrap().0, 5);
    }

    #[test]
    fn wraps_text_into_lines() {
        let (mut fonts, sc) = setup();
        let p = Paragraph::with_text(&"the quick brown fox jumps over the lazy dog ".repeat(10));
        let lay = layout_para(&mut fonts, &p, &sc, 300.0, false, None);
        assert!(lay.lines.len() >= 5, "got {} lines", lay.lines.len());
        for l in &lay.lines {
            assert!(l.w <= l.avail + 0.5, "line wider than available: {} > {}", l.w, l.avail);
        }
        // lines tile the text with no gaps
        let mut pos = 0;
        for l in &lay.lines {
            assert_eq!(l.start, pos);
            pos = l.end;
        }
        assert_eq!(pos, p.len());
    }

    #[test]
    fn justify_fills_width() {
        let (mut fonts, sc) = setup();
        let mut p = Paragraph::with_text(&"lorem ipsum dolor sit amet consectetur ".repeat(8));
        p.fmt.align = Some(Align::Justify);
        let lay = layout_para(&mut fonts, &p, &sc, 250.0, false, None);
        for l in lay.lines.iter().take(lay.lines.len() - 1) {
            let end = l.items.iter().filter(|i| !matches!(i.kind, ItemKind::Space)).map(|i| i.x + i.w).fold(0.0, f32::max);
            assert!((end - l.avail).abs() < 0.6, "justified line ends at {end}, avail {}", l.avail);
        }
    }

    #[test]
    fn caret_positions_roundtrip() {
        let (mut fonts, sc) = setup();
        let p = Paragraph::with_text("Hello wonderful world");
        let lay = layout_para(&mut fonts, &p, &sc, 400.0, false, None);
        let l = &lay.lines[0];
        for off in 0..=p.len() {
            let x = l.x_at(off);
            assert_eq!(l.offset_at(x + 0.01), off, "offset {off} at x={x}");
        }
    }

    #[test]
    fn hanging_indent_and_tabs() {
        let (mut fonts, sc) = setup();
        let mut p = Paragraph::with_text("a\tb");
        p.fmt.tabs = Some(vec![TabStop { pos: 100.0, kind: TabKind::Left, leader: Leader::None }]);
        let lay = layout_para(&mut fonts, &p, &sc, 400.0, false, None);
        let b = lay.lines[0].items.iter().find(|i| i.start == 2).unwrap();
        assert!((b.x - 100.0).abs() < 0.01, "text after tab at {}", b.x);
    }

    #[test]
    fn paginates_long_documents() {
        let (mut fonts, sc) = setup();
        let mut doc = Document::new();
        let paras: Vec<Block> = (0..120).map(|i| Block::Para(Paragraph::with_text(&format!("Paragraph number {i} with some text that is long enough to wrap onto a second line when laid out in a normal letter page width. {}", "More words follow. ".repeat(4))))).collect();
        doc.body = Story::from_blocks(paras);
        let mut eng = LayoutEngine::default();
        let pg = eng.paginate(&mut fonts, &doc, &sc);
        assert!(pg.pages.len() >= 5, "pages: {}", pg.pages.len());
        // all lines are inside their page content area
        for page in &pg.pages {
            for pl in &page.body {
                let ln = &pl.layout.lines[pl.line];
                assert!(pl.y >= page.top - 0.1 && pl.y + ln.h <= page.top + page.content_h + 0.5);
            }
        }
        // every paragraph appears
        assert!(pg.index.len() >= 120);
    }

    #[test]
    fn table_cells_get_layout_and_borders() {
        let (mut fonts, sc) = setup();
        let mut doc = Document::new();
        let mut s = Story::new();
        let p = s.insert_table(Pos::new(0, 0), 3, 2, 400.0).unwrap();
        s.insert_text(p, "cell text", None);
        doc.body = s;
        let mut eng = LayoutEngine::default();
        let pg = eng.paginate(&mut fonts, &doc, &sc);
        assert_eq!(pg.pages[0].cells.len(), 6);
        assert!(pg.pages[0].rules.len() >= 6 * 4);
        let hit = pg.hit(StoryKind::Body, 0, pg.pages[0].cells[0].x + 10.0, pg.pages[0].cells[0].y + 5.0).unwrap();
        assert_eq!(hit.p, 0);
    }

    #[test]
    fn page_break_char_starts_new_page() {
        let (mut fonts, sc) = setup();
        let mut doc = Document::new();
        let mut s = Story::from_blocks(vec![Block::Para(Paragraph::with_text("one")), Block::Para(Paragraph::with_text("two"))]);
        s.insert_page_break(Pos::new(0, 3));
        doc.body = s;
        let mut eng = LayoutEngine::default();
        let pg = eng.paginate(&mut fonts, &doc, &sc);
        assert_eq!(pg.pages.len(), 2);
    }

    #[test]
    fn list_markers_count() {
        let mut s = Story::from_blocks((0..3).map(|i| Block::Para(Paragraph::with_text(&format!("item {i}")))).collect());
        for i in 0..3 {
            s.para_mut(i).list = Some(ListInfo { id: 1, level: 0, kind: ListKind::Decimal });
        }
        s.para_mut(1).list = Some(ListInfo { id: 1, level: 1, kind: ListKind::LowerAlpha });
        let m = compute_markers(&s);
        assert_eq!(m, vec![Some("1.".into()), Some("a.".into()), Some("2.".into())]);
        assert_eq!(roman(14, false), "xiv");
        assert_eq!(alpha(27, false), "aa");
    }
}

//! DOCX reader (tolerant: unknown elements are ignored).

use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::sync::Arc;

use roxmltree::Node;

use super::*;
use crate::model::*;
use crate::story::Story;

type Rels = HashMap<String, (String, String, bool)>; // id -> (type, target, external)

struct NumLevel {
    kind: ListKind,
    left: Option<f32>,
    hanging: Option<f32>,
}

struct RawStyle {
    id: String,
    name: String,
    kind: String,
    based_on: Option<String>,
    next: Option<String>,
    ch: CharFmt,
    para: ParaFmt,
    outline: Option<u8>,
    qformat: bool,
}

#[derive(Default)]
struct Ctx {
    files: HashMap<String, Vec<u8>>,
    theme_minor: Option<String>,
    theme_major: Option<String>,
    style_ids: HashMap<String, String>,
    char_styles: HashMap<String, CharFmt>,
    table_borders: HashMap<String, TableBorders>,
    style_num: HashMap<String, (u32, u8)>,
    nums: HashMap<u32, u32>,
    abstracts: HashMap<u32, Vec<NumLevel>>,
    used_styles: HashSet<String>,
    images: HashMap<String, Option<Arc<ImageData>>>,
}

fn attr<'a>(n: &Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == name).map(|a| a.value())
}

fn child<'a, 'i>(n: &Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    n.children().find(|c| c.is_element() && c.tag_name().name() == name)
}

fn is_on(n: &Node) -> bool {
    !matches!(attr(n, "val"), Some("0") | Some("false") | Some("off") | Some("none"))
}

fn tw(v: Option<&str>) -> Option<f32> {
    v.and_then(|s| s.parse::<f32>().ok()).map(|t| t / 20.0)
}

fn parse_hex(s: &str) -> Option<Rgb> {
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn parse_border(n: &Node) -> Option<Border> {
    let val = attr(n, "val").unwrap_or("single");
    if matches!(val, "nil" | "none") {
        return None;
    }
    let sz = attr(n, "sz").and_then(|s| s.parse::<f32>().ok()).unwrap_or(4.0);
    let color = attr(n, "color").and_then(parse_hex).unwrap_or(BLACK);
    Some(Border { w: (sz / 8.0).max(0.25), color })
}

fn parse_tbl_borders(n: &Node) -> TableBorders {
    let b = |name: &str| child(n, name).and_then(|c| parse_border(&c));
    let outer = b("top").or_else(|| b("left")).or_else(|| b("bottom")).or_else(|| b("right"));
    TableBorders { outer, inner_h: b("insideH"), inner_v: b("insideV") }
}

fn read_rels(xml: Option<&Vec<u8>>) -> Rels {
    let mut m = Rels::new();
    if let Some(x) = xml {
        if let Ok(doc) = roxmltree::Document::parse(&String::from_utf8_lossy(x)) {
            for r in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "Relationship") {
                let (Some(id), Some(t), Some(target)) = (attr(&r, "Id"), attr(&r, "Type"), attr(&r, "Target")) else { continue };
                let typ = t.rsplit('/').next().unwrap_or("").to_string();
                m.insert(id.to_string(), (typ, target.to_string(), attr(&r, "TargetMode") == Some("External")));
            }
        }
    }
    m
}

fn parse_rpr(rpr: Node, ctx: &Ctx) -> CharFmt {
    let mut f = CharFmt::default();
    for c in rpr.children().filter(|c| c.is_element()) {
        match c.tag_name().name() {
            "rStyle" => {
                if let Some(cf) = attr(&c, "val").and_then(|id| ctx.char_styles.get(id)) {
                    let mut base = cf.clone();
                    base.merge(&f);
                    f = base;
                }
            }
            "rFonts" => {
                if let Some(t) = attr(&c, "asciiTheme").or_else(|| attr(&c, "hAnsiTheme")) {
                    let theme = if t.starts_with("major") { &ctx.theme_major } else { &ctx.theme_minor };
                    if let Some(n) = theme {
                        f.font = Some(n.clone());
                    }
                } else if let Some(n) = attr(&c, "ascii").or_else(|| attr(&c, "hAnsi")).or_else(|| attr(&c, "cs")) {
                    f.font = Some(n.to_string());
                }
            }
            "b" => f.bold = Some(is_on(&c)),
            "i" => f.italic = Some(is_on(&c)),
            "strike" | "dstrike" => f.strike = Some(is_on(&c)),
            "caps" => f.all_caps = Some(is_on(&c)),
            "smallCaps" => f.small_caps = Some(is_on(&c)),
            "u" => f.underline = Some(attr(&c, "val") != Some("none")),
            "color" => {
                if let Some(h) = attr(&c, "val").and_then(parse_hex) {
                    f.color = Some(h);
                }
            }
            "sz" => {
                if let Some(v) = attr(&c, "val").and_then(|s| s.parse::<f32>().ok()) {
                    f.size = Some(v / 2.0);
                }
            }
            "highlight" => match attr(&c, "val") {
                Some("none") => f.highlight = Some(None),
                Some(n) => {
                    if let Some((_, col)) = HIGHLIGHTS.iter().find(|(name, _)| *name == n) {
                        f.highlight = Some(Some(*col));
                    }
                }
                None => {}
            },
            "shd" => {
                if let Some(h) = attr(&c, "fill").and_then(parse_hex) {
                    if f.highlight.is_none() {
                        f.highlight = Some(Some(h));
                    }
                }
            }
            "vertAlign" => {
                f.vert = match attr(&c, "val") {
                    Some("superscript") => Some(VertAlign::Super),
                    Some("subscript") => Some(VertAlign::Sub),
                    Some("baseline") => Some(VertAlign::Baseline),
                    _ => None,
                }
            }
            "spacing" => {
                if let Some(v) = tw(attr(&c, "val")) {
                    f.spacing = Some(v);
                }
            }
            _ => {}
        }
    }
    f
}

#[derive(Default)]
struct PInfo {
    fmt: ParaFmt,
    style_id: Option<String>,
    num: Option<(u32, u8)>,
    mark: Option<CharFmt>,
}

fn parse_ppr(ppr: Node, ctx: &Ctx) -> PInfo {
    let mut p = PInfo::default();
    for c in ppr.children().filter(|c| c.is_element()) {
        match c.tag_name().name() {
            "pStyle" => p.style_id = attr(&c, "val").map(String::from),
            "jc" => {
                p.fmt.align = match attr(&c, "val") {
                    Some("center") => Some(Align::Center),
                    Some("right") | Some("end") => Some(Align::Right),
                    Some("both") | Some("distribute") | Some("justify") => Some(Align::Justify),
                    Some("left") | Some("start") => Some(Align::Left),
                    _ => None,
                }
            }
            "ind" => {
                if let Some(v) = tw(attr(&c, "left").or_else(|| attr(&c, "start"))) {
                    p.fmt.left = Some(v);
                }
                if let Some(v) = tw(attr(&c, "right").or_else(|| attr(&c, "end"))) {
                    p.fmt.right = Some(v);
                }
                if let Some(v) = tw(attr(&c, "hanging")) {
                    p.fmt.first = Some(-v);
                } else if let Some(v) = tw(attr(&c, "firstLine")) {
                    p.fmt.first = Some(v);
                }
            }
            "spacing" => {
                if let Some(v) = tw(attr(&c, "before")) {
                    p.fmt.before = Some(v);
                }
                if let Some(v) = tw(attr(&c, "after")) {
                    p.fmt.after = Some(v);
                }
                if let Some(l) = attr(&c, "line").and_then(|s| s.parse::<f32>().ok()) {
                    p.fmt.line = Some(match attr(&c, "lineRule") {
                        Some("exact") => LineSpacing::Exact(l / 20.0),
                        Some("atLeast") => LineSpacing::AtLeast(l / 20.0),
                        _ => LineSpacing::Multiple(l / 240.0),
                    });
                }
            }
            "keepNext" => p.fmt.keep_next = Some(is_on(&c)),
            "keepLines" => p.fmt.keep_lines = Some(is_on(&c)),
            "pageBreakBefore" => p.fmt.page_break_before = Some(is_on(&c)),
            "numPr" => {
                let id = child(&c, "numId").and_then(|n| attr(&n, "val")).and_then(|v| v.parse::<u32>().ok());
                let lvl = child(&c, "ilvl").and_then(|n| attr(&n, "val")).and_then(|v| v.parse::<u8>().ok()).unwrap_or(0);
                if let Some(id) = id {
                    p.num = Some((id, lvl));
                }
            }
            "shd" => {
                if let Some(h) = attr(&c, "fill").and_then(parse_hex) {
                    p.fmt.shade = Some(Some(h));
                }
            }
            "pBdr" => {
                let b = |name: &str| child(&c, name).and_then(|x| parse_border(&x));
                let borders = ParaBorders { top: b("top"), bottom: b("bottom"), left: b("left"), right: b("right") };
                p.fmt.borders = Some(borders);
            }
            "tabs" => {
                let mut tabs = vec![];
                for t in c.children().filter(|n| n.is_element() && n.tag_name().name() == "tab") {
                    let kind = match attr(&t, "val") {
                        Some("center") => TabKind::Center,
                        Some("right") | Some("end") => TabKind::Right,
                        Some("clear") | Some("bar") => continue,
                        _ => TabKind::Left,
                    };
                    let leader = match attr(&t, "leader") {
                        Some("dot") | Some("middleDot") => Leader::Dots,
                        Some("hyphen") => Leader::Dashes,
                        Some("underscore") | Some("heavy") => Leader::Line,
                        _ => Leader::None,
                    };
                    if let Some(pos) = tw(attr(&t, "pos")) {
                        tabs.push(TabStop { pos, kind, leader });
                    }
                }
                p.fmt.tabs = Some(tabs);
            }
            "rPr" => p.mark = Some(parse_rpr(c, ctx)),
            _ => {}
        }
    }
    p
}

// ------------------------------------------------------------------ fields

#[derive(Default)]
struct FieldSt {
    depth: u32,
    instr: String,
    in_instr: bool,
    skip_result: bool,
    link: Option<String>,
}

fn field_kind(instr: &str) -> Option<Field> {
    let i = instr.trim().to_uppercase();
    if i.starts_with("NUMPAGES") {
        Some(Field::NumPages)
    } else if i.starts_with("PAGE") && !i.starts_with("PAGEREF") {
        Some(Field::Page)
    } else {
        None
    }
}

fn hyperlink_url(instr: &str) -> Option<String> {
    let t = instr.trim();
    if !t.to_uppercase().starts_with("HYPERLINK") {
        return None;
    }
    let q: Vec<&str> = t.split('"').collect();
    q.get(1).map(|s| s.to_string())
}

fn sym_char(c: &str) -> Option<char> {
    match c.to_uppercase().as_str() {
        "F0B7" => Some('\u{2022}'),
        "F0A7" => Some('\u{25AA}'),
        "F06F" => Some('o'),
        "F0D8" => Some('\u{27A2}'),
        "F0FC" => Some('\u{2713}'),
        _ => None,
    }
}

fn load_image(ctx: &mut Ctx, rels: &Rels, rid: &str) -> Option<Arc<ImageData>> {
    let (_, target, ext) = rels.get(rid)?;
    if *ext {
        return None;
    }
    let path = if let Some(t) = target.strip_prefix('/') { t.to_string() } else { format!("word/{}", target.trim_start_matches("./")) };
    if let Some(c) = ctx.images.get(&path) {
        return c.clone();
    }
    let img = ctx.files.get(&path).and_then(|b| ImageData::from_bytes(b.clone()).ok());
    ctx.images.insert(path, img.clone());
    img
}

fn parse_drawing(d: Node, ctx: &mut Ctx, rels: &Rels) -> Option<ImageObj> {
    let blip = d.descendants().find(|n| n.is_element() && n.tag_name().name() == "blip")?;
    let rid = attr(&blip, "embed")?;
    let data = load_image(ctx, rels, rid)?;
    let ext = d.descendants().find(|n| n.is_element() && n.tag_name().name() == "extent").or_else(|| d.descendants().find(|n| n.is_element() && n.tag_name().name() == "ext" && attr(n, "cx").is_some()));
    let (mut w, mut h) = (data.px_w as f32 * 0.75, data.px_h as f32 * 0.75);
    if let Some(e) = ext {
        if let (Some(cx), Some(cy)) = (attr(&e, "cx").and_then(|v| v.parse::<f32>().ok()), attr(&e, "cy").and_then(|v| v.parse::<f32>().ok())) {
            if cx > 0.0 && cy > 0.0 {
                w = cx / 12700.0;
                h = cy / 12700.0;
            }
        }
    }
    Some(ImageObj { data, w, h })
}

fn parse_run(r: Node, ctx: &mut Ctx, rels: &Rels, link: &Option<String>, st: &mut FieldSt, out: &mut Vec<Run>) {
    let fmt = child(&r, "rPr").map(|p| parse_rpr(p, ctx)).unwrap_or_default();
    let mut text = String::new();
    macro_rules! flush {
        () => {
            if !text.is_empty() {
                let l = st.link.clone().or_else(|| link.clone());
                out.push(Run { text: std::mem::take(&mut text), fmt: fmt.clone(), link: l, obj: None });
            }
        };
    }
    for c in r.children().filter(|c| c.is_element()) {
        let name = c.tag_name().name();
        match name {
            "fldChar" => match attr(&c, "fldCharType") {
                Some("begin") => {
                    flush!();
                    if st.depth == 0 {
                        st.instr.clear();
                        st.in_instr = true;
                    }
                    st.depth += 1;
                }
                Some("separate") => {
                    if st.depth == 1 {
                        st.in_instr = false;
                        if let Some(f) = field_kind(&st.instr) {
                            out.push(Run { text: OBJ_CHAR.to_string(), fmt: fmt.clone(), link: None, obj: Some(Obj::Field(f)) });
                            st.skip_result = true;
                        } else {
                            st.link = hyperlink_url(&st.instr);
                        }
                    }
                }
                Some("end") => {
                    flush!();
                    st.depth = st.depth.saturating_sub(1);
                    if st.depth == 0 {
                        st.skip_result = false;
                        st.in_instr = false;
                        st.link = None;
                    }
                }
                _ => {}
            },
            "instrText" => {
                if st.in_instr {
                    st.instr.push_str(&c.text().unwrap_or(""));
                }
            }
            _ if st.skip_result || st.in_instr => {}
            "t" => text.push_str(c.text().unwrap_or("")),
            "tab" => text.push('\t'),
            "br" => match attr(&c, "type") {
                Some("page") => text.push(PAGE_BREAK),
                Some("column") => text.push(PAGE_BREAK),
                _ => text.push('\n'),
            },
            "cr" => text.push('\n'),
            "noBreakHyphen" => text.push('\u{2011}'),
            "sym" => {
                if let Some(ch) = attr(&c, "char").and_then(sym_char) {
                    text.push(ch);
                }
            }
            "drawing" => {
                flush!();
                if let Some(img) = parse_drawing(c, ctx, rels) {
                    out.push(Run { text: OBJ_CHAR.to_string(), fmt: fmt.clone(), link: None, obj: Some(Obj::Image(img)) });
                }
            }
            _ => {}
        }
    }
    flush!();
}

fn parse_runs(node: Node, ctx: &mut Ctx, rels: &Rels, link: Option<String>, st: &mut FieldSt, out: &mut Vec<Run>) {
    for c in node.children().filter(|c| c.is_element()) {
        match c.tag_name().name() {
            "r" => parse_run(c, ctx, rels, &link, st, out),
            "hyperlink" => {
                let l = attr(&c, "id").and_then(|id| rels.get(id)).filter(|r| r.2).map(|r| r.1.clone());
                parse_runs(c, ctx, rels, l.or(link.clone()), st, out);
            }
            "fldSimple" => {
                let instr = attr(&c, "instr").unwrap_or("");
                if let Some(f) = field_kind(instr) {
                    let fmt = c.descendants().find(|n| n.is_element() && n.tag_name().name() == "rPr").map(|p| parse_rpr(p, ctx)).unwrap_or_default();
                    out.push(Run { text: OBJ_CHAR.to_string(), fmt, link: None, obj: Some(Obj::Field(f)) });
                } else {
                    parse_runs(c, ctx, rels, link.clone().or_else(|| hyperlink_url(instr)), st, out);
                }
            }
            "smartTag" | "ins" | "customXml" | "moveTo" | "dir" | "bdo" => parse_runs(c, ctx, rels, link.clone(), st, out),
            "sdt" => {
                if let Some(content) = child(&c, "sdtContent") {
                    parse_runs(content, ctx, rels, link.clone(), st, out);
                }
            }
            _ => {}
        }
    }
}

fn split_page_breaks(p: Paragraph) -> Vec<Paragraph> {
    let mut out = vec![];
    let mut cur = p;
    loop {
        let chars: Vec<char> = cur.text().chars().collect();
        let n = chars.len();
        match chars.iter().position(|&c| c == PAGE_BREAK) {
            Some(i) if i + 1 < n => {
                let mut tail = cur.split_off(i + 1);
                tail.list = None;
                tail.fmt.page_break_before = None;
                out.push(cur);
                cur = tail;
            }
            _ => {
                out.push(cur);
                return out;
            }
        }
    }
}

fn parse_para(node: Node, ctx: &mut Ctx, rels: &Rels) -> Vec<Paragraph> {
    let mut info = child(&node, "pPr").map(|p| parse_ppr(p, ctx)).unwrap_or_default();
    let mut para = Paragraph::new();
    let sid = info.style_id.clone().unwrap_or_default();
    if let Some(name) = ctx.style_ids.get(&sid) {
        para.style = name.clone();
    }
    ctx.used_styles.insert(para.style.clone());
    if info.num.is_none() {
        info.num = ctx.style_num.get(&sid).copied();
    }
    para.fmt = info.fmt;
    if let Some((id, lvl)) = info.num {
        if id != 0 {
            let lvl = lvl.min(8);
            let def = ctx.nums.get(&id).and_then(|a| ctx.abstracts.get(a)).and_then(|l| l.get(lvl as usize));
            let kind = def.map(|d| d.kind).unwrap_or(ListKind::Bullet);
            if let Some(d) = def {
                if para.fmt.left.is_none() {
                    if let Some(l) = d.left {
                        if (l - 36.0 * (lvl as f32 + 1.0)).abs() > 1.0 {
                            para.fmt.left = Some(l);
                        }
                    }
                }
                if para.fmt.first.is_none() {
                    if let Some(h) = d.hanging {
                        if (h - 18.0).abs() > 1.0 {
                            para.fmt.first = Some(-h);
                        }
                    }
                }
            }
            para.list = Some(ListInfo { id, level: lvl, kind });
        }
    }
    let mut st = FieldSt::default();
    let mut runs = vec![];
    parse_runs(node, ctx, rels, None, &mut st, &mut runs);
    para.runs = runs;
    if para.runs.is_empty() {
        if let Some(m) = info.mark {
            para.runs.push(Run { text: String::new(), fmt: m, link: None, obj: None });
        }
    }
    para.normalize();
    split_page_breaks(para)
}

fn parse_table(node: Node, ctx: &mut Ctx, rels: &Rels) -> Option<Table> {
    let mut cols: Vec<f32> = child(&node, "tblGrid").map(|g| g.children().filter(|c| c.is_element() && c.tag_name().name() == "gridCol").filter_map(|c| tw(attr(&c, "w"))).collect()).unwrap_or_default();
    let mut borders = TableBorders::none();
    let mut align = Align::Left;
    let mut pad = 5.4;
    let mut style_b: Option<TableBorders> = None;
    if let Some(pr) = child(&node, "tblPr") {
        if let Some(s) = child(&pr, "tblStyle").and_then(|s| attr(&s, "val")) {
            style_b = ctx.table_borders.get(s).cloned();
        }
        if let Some(b) = child(&pr, "tblBorders") {
            borders = parse_tbl_borders(&b);
            style_b = None;
        }
        if let Some(j) = child(&pr, "jc").and_then(|j| attr(&j, "val")) {
            align = match j {
                "center" => Align::Center,
                "right" | "end" => Align::Right,
                _ => Align::Left,
            };
        }
        if let Some(m) = child(&pr, "tblCellMar").and_then(|m| child(&m, "left")).and_then(|l| tw(attr(&l, "w"))) {
            pad = m;
        }
    }
    if let Some(sb) = style_b {
        borders = sb;
    }
    let trs: Vec<Node> = node.children().filter(|c| c.is_element() && c.tag_name().name() == "tr").collect();
    if trs.is_empty() {
        return None;
    }
    // first pass: determine the column count
    let span_of = |tc: &Node| -> usize { child(tc, "tcPr").and_then(|p| child(&p, "gridSpan")).and_then(|g| attr(&g, "val")).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).max(1) };
    let mut ncols = cols.len();
    for tr in &trs {
        let n: usize = tr.children().filter(|c| c.is_element() && c.tag_name().name() == "tc").map(|tc| span_of(&tc)).sum();
        ncols = ncols.max(n);
    }
    if ncols == 0 {
        return None;
    }
    while cols.len() < ncols {
        let w = cols.last().copied().unwrap_or(72.0);
        cols.push(w);
    }
    let mut table = Table { cols, rows: vec![], borders, align, pad_lr: pad };
    for (ri, tr) in trs.iter().enumerate() {
        let mut row = TRow { cells: (0..ncols).map(|_| Cell::new()).collect(), height: None, header: false };
        if let Some(p) = child(tr, "trPr") {
            if let Some(h) = child(&p, "trHeight").and_then(|h| tw(attr(&h, "val"))) {
                row.height = Some(h);
            }
            row.header = child(&p, "tblHeader").map(|n| is_on(&n)).unwrap_or(false);
        }
        table.rows.push(row);
        let mut col = 0usize;
        for tc in tr.children().filter(|c| c.is_element() && c.tag_name().name() == "tc") {
            if col >= ncols {
                break;
            }
            let cs = span_of(&tc).min(ncols - col);
            let mut shade = None;
            let mut valign = VAlign::Top;
            let mut vmerge: Option<bool> = None; // Some(true)=restart, Some(false)=continue
            if let Some(pr) = child(&tc, "tcPr") {
                if let Some(vm) = child(&pr, "vMerge") {
                    vmerge = Some(attr(&vm, "val") == Some("restart"));
                }
                if let Some(h) = child(&pr, "shd").and_then(|s| attr(&s, "fill")).and_then(parse_hex) {
                    shade = Some(h);
                }
                valign = match child(&pr, "vAlign").and_then(|v| attr(&v, "val")) {
                    Some("center") => VAlign::Center,
                    Some("bottom") => VAlign::Bottom,
                    _ => VAlign::Top,
                };
            }
            if vmerge == Some(false) && ri > 0 {
                // continuation of the cell above
                let (or, oc) = Story::origin_of(&table, ri - 1, col);
                if or + table.rows[or].cells[oc].rs == ri {
                    table.rows[or].cells[oc].rs += 1;
                    for c in col..col + cs {
                        table.rows[ri].cells[c] = Cell::covered();
                    }
                    col += cs;
                    continue;
                }
            }
            let mut paras: Vec<Paragraph> = vec![];
            for b in tc.children().filter(|c| c.is_element()) {
                match b.tag_name().name() {
                    "p" => paras.extend(parse_para(b, ctx, rels)),
                    "sdt" => {
                        if let Some(content) = child(&b, "sdtContent") {
                            for p in content.children().filter(|c| c.is_element() && c.tag_name().name() == "p") {
                                paras.extend(parse_para(p, ctx, rels));
                            }
                        }
                    }
                    "tbl" => {
                        // nested tables are flattened to their text
                        for p in b.descendants().filter(|c| c.is_element() && c.tag_name().name() == "p") {
                            paras.extend(parse_para(p, ctx, rels));
                        }
                    }
                    _ => {}
                }
            }
            if paras.is_empty() {
                paras.push(Paragraph::new());
            }
            table.rows[ri].cells[col] = Cell { paras, cs, rs: 1, covered: false, shade, valign };
            for c in col + 1..col + cs {
                table.rows[ri].cells[c] = Cell::covered();
            }
            col += cs;
        }
    }
    Some(table)
}

fn parse_blocks(node: Node, ctx: &mut Ctx, rels: &Rels, out: &mut Vec<Block>) {
    for c in node.children().filter(|c| c.is_element()) {
        match c.tag_name().name() {
            "p" => out.extend(parse_para(c, ctx, rels).into_iter().map(Block::Para)),
            "tbl" => {
                if let Some(t) = parse_table(c, ctx, rels) {
                    out.push(Block::Table(t));
                }
            }
            "sdt" => {
                if let Some(content) = child(&c, "sdtContent") {
                    parse_blocks(content, ctx, rels, out);
                }
            }
            "customXml" | "ins" | "moveTo" => parse_blocks(c, ctx, rels, out),
            _ => {}
        }
    }
}

fn map_num_fmt(v: &str) -> ListKind {
    match v {
        "bullet" | "none" => ListKind::Bullet,
        "lowerLetter" => ListKind::LowerAlpha,
        "upperLetter" => ListKind::UpperAlpha,
        "lowerRoman" => ListKind::LowerRoman,
        "upperRoman" => ListKind::UpperRoman,
        _ => ListKind::Decimal,
    }
}

fn parse_numbering(xml: &str, ctx: &mut Ctx) {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return };
    for an in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "abstractNum") {
        let Some(id) = attr(&an, "abstractNumId").and_then(|v| v.parse::<u32>().ok()) else { continue };
        let mut levels: Vec<NumLevel> = vec![];
        for lvl in an.children().filter(|c| c.is_element() && c.tag_name().name() == "lvl") {
            let idx = attr(&lvl, "ilvl").and_then(|v| v.parse::<usize>().ok()).unwrap_or(levels.len());
            let kind = child(&lvl, "numFmt").and_then(|n| attr(&n, "val")).map(map_num_fmt).unwrap_or(ListKind::Decimal);
            let ind = child(&lvl, "pPr").and_then(|p| child(&p, "ind"));
            let left = ind.and_then(|i| tw(attr(&i, "left").or_else(|| attr(&i, "start"))));
            let hanging = ind.and_then(|i| tw(attr(&i, "hanging")));
            while levels.len() < idx {
                levels.push(NumLevel { kind: ListKind::Decimal, left: None, hanging: None });
            }
            let nl = NumLevel { kind, left, hanging };
            if idx < levels.len() {
                levels[idx] = nl;
            } else {
                levels.push(nl);
            }
        }
        ctx.abstracts.insert(id, levels);
    }
    for n in doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "num") {
        if let (Some(id), Some(a)) = (attr(&n, "numId").and_then(|v| v.parse::<u32>().ok()), child(&n, "abstractNumId").and_then(|a| attr(&a, "val")).and_then(|v| v.parse::<u32>().ok())) {
            ctx.nums.insert(id, a);
        }
    }
}

fn parse_styles(xml: &str, ctx: &mut Ctx) -> (Vec<RawStyle>, CharFmt, ParaFmt) {
    let mut raws = vec![];
    let (mut def_c, mut def_p) = (CharFmt::default(), ParaFmt::default());
    let Ok(doc) = roxmltree::Document::parse(xml) else { return (raws, def_c, def_p) };
    if let Some(dd) = doc.descendants().find(|n| n.is_element() && n.tag_name().name() == "docDefaults") {
        if let Some(r) = dd.descendants().find(|n| n.is_element() && n.tag_name().name() == "rPr") {
            def_c = parse_rpr(r, ctx);
        }
        if let Some(p) = dd.descendants().find(|n| n.is_element() && n.tag_name().name() == "pPr") {
            def_p = parse_ppr(p, ctx).fmt;
        }
    }
    // character styles first (paragraph styles may reference them via rStyle)
    let styles: Vec<Node> = doc.descendants().filter(|n| n.is_element() && n.tag_name().name() == "style").collect();
    for s in &styles {
        if attr(s, "type") == Some("character") {
            if let (Some(id), Some(r)) = (attr(s, "styleId"), child(s, "rPr")) {
                let f = parse_rpr(r, ctx);
                ctx.char_styles.insert(id.to_string(), f);
            }
        }
    }
    for s in &styles {
        let Some(id) = attr(s, "styleId") else { continue };
        let kind = attr(s, "type").unwrap_or("paragraph").to_string();
        if kind == "table" {
            if let Some(b) = child(s, "tblPr").and_then(|p| child(&p, "tblBorders")) {
                ctx.table_borders.insert(id.to_string(), parse_tbl_borders(&b));
            }
            continue;
        }
        if kind != "paragraph" {
            continue;
        }
        let name = child(s, "name").and_then(|n| attr(&n, "val")).unwrap_or(id).to_string();
        let our = our_style_name(&name);
        let pinfo = child(s, "pPr").map(|p| parse_ppr(p, ctx)).unwrap_or_default();
        let ch = child(s, "rPr").map(|r| parse_rpr(r, ctx)).unwrap_or_default();
        let outline = child(s, "pPr").and_then(|p| child(&p, "outlineLvl")).and_then(|o| attr(&o, "val")).and_then(|v| v.parse::<u8>().ok()).map(|v| v + 1).filter(|v| *v <= 9).or_else(|| our.strip_prefix("Heading ").and_then(|n| n.parse::<u8>().ok()));
        if let Some(num) = pinfo.num {
            ctx.style_num.insert(id.to_string(), num);
        }
        ctx.style_ids.insert(id.to_string(), our.clone());
        raws.push(RawStyle {
            id: id.to_string(),
            name: our,
            kind,
            based_on: child(s, "basedOn").and_then(|b| attr(&b, "val")).map(String::from),
            next: child(s, "next").and_then(|b| attr(&b, "val")).map(String::from),
            ch,
            para: pinfo.fmt,
            outline,
            qformat: child(s, "qFormat").is_some(),
        });
    }
    (raws, def_c, def_p)
}

fn parse_theme(xml: &str) -> (Option<String>, Option<String>) {
    let Ok(doc) = roxmltree::Document::parse(xml) else { return (None, None) };
    let font = |tag: &str| doc.descendants().find(|n| n.is_element() && n.tag_name().name() == tag).and_then(|f| child(&f, "latin")).and_then(|l| attr(&l, "typeface")).map(String::from).filter(|s| !s.is_empty());
    (font("majorFont"), font("minorFont"))
}

fn part_blocks(ctx: &mut Ctx, path: &str, root_tag: &str) -> Option<Story> {
    let bytes = ctx.files.get(path)?.clone();
    let file = path.rsplit('/').next().unwrap_or(path);
    let rels_path = format!("word/_rels/{file}.rels");
    let rels = read_rels(ctx.files.get(&rels_path));
    let xml = String::from_utf8_lossy(&bytes).into_owned();
    let doc = roxmltree::Document::parse(&xml).ok()?;
    let root = doc.descendants().find(|n| n.is_element() && n.tag_name().name() == root_tag)?;
    let mut blocks = vec![];
    parse_blocks(root, ctx, &rels, &mut blocks);
    Some(Story::from_blocks(blocks))
}

pub fn read(bytes: &[u8]) -> Result<Document, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("Not a valid .docx file: {e}"))?;
    let mut ctx = Ctx::default();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_string();
        if name.starts_with("word/") || name.starts_with("docProps/") || name.starts_with("_rels/") {
            let mut buf = vec![];
            f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            ctx.files.insert(name, buf);
        }
    }
    let doc_xml = ctx.files.get("word/document.xml").cloned().ok_or("Missing word/document.xml (not a Word document?)")?;
    let rels = read_rels(ctx.files.get("word/_rels/document.xml.rels"));
    // theme fonts
    if let Some(t) = ctx.files.get("word/theme/theme1.xml").map(|b| String::from_utf8_lossy(b).into_owned()) {
        let (maj, min) = parse_theme(&t);
        ctx.theme_major = maj;
        ctx.theme_minor = min;
    }
    if let Some(n) = ctx.files.get("word/numbering.xml").map(|b| String::from_utf8_lossy(b).into_owned()) {
        parse_numbering(&n, &mut ctx);
    }
    let (raws, def_c, def_p) = match ctx.files.get("word/styles.xml").map(|b| String::from_utf8_lossy(b).into_owned()) {
        Some(x) => parse_styles(&x, &mut ctx),
        None => (vec![], CharFmt::default(), ParaFmt::default()),
    };

    let xml = String::from_utf8_lossy(&doc_xml).into_owned();
    let parsed = roxmltree::Document::parse(&xml).map_err(|e| format!("Invalid document.xml: {e}"))?;
    let body = parsed.descendants().find(|n| n.is_element() && n.tag_name().name() == "body").ok_or("Missing document body")?;
    let mut blocks = vec![];
    parse_blocks(body, &mut ctx, &rels, &mut blocks);

    let mut doc = Document::new();
    doc.body = Story::from_blocks(blocks);

    // page setup and headers/footers
    if let Some(sect) = body.children().filter(|c| c.is_element() && c.tag_name().name() == "sectPr").last() {
        let mut pg = doc.page;
        if let Some(sz) = child(&sect, "pgSz") {
            if let (Some(w), Some(h)) = (tw(attr(&sz, "w")), tw(attr(&sz, "h"))) {
                pg.w = w;
                pg.h = h;
            }
        }
        if let Some(m) = child(&sect, "pgMar") {
            let g = |n: &str| tw(attr(&m, n)).map(|v| v.abs());
            pg.top = g("top").unwrap_or(pg.top);
            pg.bottom = g("bottom").unwrap_or(pg.bottom);
            pg.left = g("left").unwrap_or(pg.left);
            pg.right = g("right").unwrap_or(pg.right);
            pg.header_dist = g("header").unwrap_or(pg.header_dist);
            pg.footer_dist = g("footer").unwrap_or(pg.footer_dist);
        }
        pg.diff_first = child(&sect, "titlePg").is_some();
        doc.page = pg;
        for r in sect.children().filter(|c| c.is_element()) {
            let tag = r.tag_name().name();
            if tag != "headerReference" && tag != "footerReference" {
                continue;
            }
            let typ = attr(&r, "type").unwrap_or("default");
            let Some(target) = attr(&r, "id").and_then(|id| rels.get(id)).map(|r| r.1.clone()) else { continue };
            let path = if let Some(t) = target.strip_prefix('/') { t.to_string() } else { format!("word/{target}") };
            let is_h = tag == "headerReference";
            if let Some(st) = part_blocks(&mut ctx, &path, if is_h { "hdr" } else { "ftr" }) {
                match (is_h, typ) {
                    (true, "default") => doc.header = st,
                    (true, "first") => doc.first_header = st,
                    (false, "default") => doc.footer = st,
                    (false, "first") => doc.first_footer = st,
                    _ => {}
                }
            }
        }
    }
    // header/footer stories use the Header/Footer styles unless the file says otherwise
    for (st, name) in [(&mut doc.header, "Header"), (&mut doc.footer, "Footer"), (&mut doc.first_header, "Header"), (&mut doc.first_footer, "Footer")] {
        for i in 0..st.len() {
            if st.para(i).style == "Normal" {
                st.para_mut(i).style = name.to_string();
            }
        }
    }

    // styles: import those that matter (quick styles, used styles and their ancestors)
    if !raws.is_empty() {
        let mut keep: HashSet<String> = HashSet::new();
        for r in &raws {
            if r.kind == "paragraph" && (r.qformat || ctx.used_styles.contains(&r.name) || r.name == "Normal") {
                keep.insert(r.id.clone());
            }
        }
        loop {
            let mut added = false;
            for r in &raws {
                if keep.contains(&r.id) {
                    for dep in [&r.based_on, &r.next].into_iter().flatten() {
                        if !keep.contains(dep) && raws.iter().any(|x| &x.id == dep) {
                            keep.insert(dep.clone());
                            added = true;
                        }
                    }
                }
            }
            if !added {
                break;
            }
        }
        let mut list: Vec<Style> = vec![];
        for r in raws.iter().filter(|r| keep.contains(&r.id)) {
            list.push(Style {
                name: r.name.clone(),
                based_on: r.based_on.as_ref().and_then(|b| ctx.style_ids.get(b)).cloned(),
                next: r.next.as_ref().and_then(|b| ctx.style_ids.get(b)).cloned(),
                ch: r.ch.clone(),
                para: r.para.clone(),
                outline: r.outline,
            });
        }
        // make sure the styles our UI relies on exist
        for d in Styles::word_defaults().list {
            if !list.iter().any(|s| s.name == d.name) && (d.name == "Normal" || d.name == "Header" || d.name == "Footer" || d.name.starts_with("TOC")) {
                list.push(d);
            }
        }
        let mut dc = Styles::word_defaults().def_char;
        dc.merge(&def_c);
        let mut dp = Styles::word_defaults().def_para;
        dp.merge(&def_p);
        doc.styles = Styles { list, def_char: dc, def_para: dp };
        doc.touch_styles();
    }

    if let Some(core) = ctx.files.get("docProps/core.xml").map(|b| String::from_utf8_lossy(b).into_owned()) {
        if let Ok(x) = roxmltree::Document::parse(&core) {
            let get = |tag: &str| x.descendants().find(|n| n.is_element() && n.tag_name().name() == tag).and_then(|n| n.text()).unwrap_or("").to_string();
            doc.meta.title = get("title");
            doc.meta.author = get("creator");
        }
    }
    Ok(doc)
}

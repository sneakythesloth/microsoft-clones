//! DOCX writer.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::sync::Arc;

use super::*;
use crate::model::*;
use crate::story::{Story, TableRef};

const NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";
const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => o.push(c),
        }
    }
    o
}

#[derive(Default)]
struct Rels {
    items: Vec<(String, &'static str, String, bool)>,
}

impl Rels {
    fn add(&mut self, typ: &'static str, target: String, external: bool) -> String {
        if let Some(i) = self.items.iter().find(|i| i.1 == typ && i.2 == target && i.3 == external) {
            return i.0.clone();
        }
        let id = format!("rId{}", self.items.len() + 1);
        self.items.push((id.clone(), typ, target, external));
        id
    }
    fn xml(&self) -> String {
        let mut s = String::from(XML_HEAD);
        s.push_str("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
        for (id, t, target, ext) in &self.items {
            s.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/{t}\" Target=\"{}\"{}/>", esc(target), if *ext { " TargetMode=\"External\"" } else { "" }));
        }
        s.push_str("</Relationships>");
        s
    }
}

struct Ctx {
    media: Vec<(String, Arc<ImageData>)>,
    media_names: HashMap<u64, String>,
    docpr: u32,
    lists: BTreeMap<u32, [ListKind; 9]>,
}

fn rpr(f: &CharFmt, link: bool) -> String {
    let mut s = String::new();
    if let Some(font) = &f.font {
        let e = esc(font);
        s.push_str(&format!("<w:rFonts w:ascii=\"{e}\" w:hAnsi=\"{e}\" w:cs=\"{e}\" w:eastAsia=\"{e}\"/>"));
    }
    let tog = |tag: &str, v: Option<bool>| -> String {
        match v {
            Some(true) => format!("<w:{tag}/>"),
            Some(false) => format!("<w:{tag} w:val=\"0\"/>"),
            None => String::new(),
        }
    };
    s.push_str(&tog("b", f.bold));
    s.push_str(&tog("i", f.italic));
    s.push_str(&tog("caps", f.all_caps));
    s.push_str(&tog("smallCaps", f.small_caps));
    s.push_str(&tog("strike", f.strike));
    match f.color {
        Some(c) => s.push_str(&format!("<w:color w:val=\"{}\"/>", hex(c))),
        None if link => s.push_str("<w:color w:val=\"0563C1\"/>"),
        None => {}
    }
    if let Some(sp) = f.spacing {
        s.push_str(&format!("<w:spacing w:val=\"{}\"/>", twips(sp)));
    }
    if let Some(sz) = f.size {
        let h = (sz * 2.0).round() as i64;
        s.push_str(&format!("<w:sz w:val=\"{h}\"/><w:szCs w:val=\"{h}\"/>"));
    }
    let mut shd = None;
    if let Some(Some(h)) = f.highlight {
        if let Some((name, _)) = HIGHLIGHTS.iter().find(|(_, c)| *c == h) {
            s.push_str(&format!("<w:highlight w:val=\"{name}\"/>"));
        } else {
            shd = Some(h);
        }
    }
    match f.underline {
        Some(true) => s.push_str("<w:u w:val=\"single\"/>"),
        Some(false) => s.push_str("<w:u w:val=\"none\"/>"),
        None if link => s.push_str("<w:u w:val=\"single\"/>"),
        None => {}
    }
    if let Some(c) = shd {
        s.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(c)));
    }
    match f.vert {
        Some(VertAlign::Super) => s.push_str("<w:vertAlign w:val=\"superscript\"/>"),
        Some(VertAlign::Sub) => s.push_str("<w:vertAlign w:val=\"subscript\"/>"),
        Some(VertAlign::Baseline) => s.push_str("<w:vertAlign w:val=\"baseline\"/>"),
        None => {}
    }
    s
}

fn border_xml(tag: &str, b: &Border, space: i32) -> String {
    format!("<w:{tag} w:val=\"single\" w:sz=\"{}\" w:space=\"{space}\" w:color=\"{}\"/>", ((b.w * 8.0).round() as i64).clamp(2, 96), hex(b.color))
}

fn ppr(f: &ParaFmt, style: Option<&str>, list: Option<ListInfo>, outline: Option<u8>) -> String {
    let mut s = String::new();
    if let Some(st) = style {
        s.push_str(&format!("<w:pStyle w:val=\"{}\"/>", style_id(st)));
    }
    let tog = |tag: &str, v: Option<bool>| match v {
        Some(true) => format!("<w:{tag}/>"),
        Some(false) => format!("<w:{tag} w:val=\"0\"/>"),
        None => String::new(),
    };
    s.push_str(&tog("keepNext", f.keep_next));
    s.push_str(&tog("keepLines", f.keep_lines));
    s.push_str(&tog("pageBreakBefore", f.page_break_before));
    if let Some(l) = list {
        s.push_str(&format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{}\"/></w:numPr>", l.level, l.id));
    }
    if let Some(b) = &f.borders {
        let mut inner = String::new();
        if let Some(x) = &b.top {
            inner.push_str(&border_xml("top", x, 1));
        }
        if let Some(x) = &b.left {
            inner.push_str(&border_xml("left", x, 4));
        }
        if let Some(x) = &b.bottom {
            inner.push_str(&border_xml("bottom", x, 1));
        }
        if let Some(x) = &b.right {
            inner.push_str(&border_xml("right", x, 4));
        }
        if !inner.is_empty() {
            s.push_str(&format!("<w:pBdr>{inner}</w:pBdr>"));
        }
    }
    if let Some(sh) = f.shade {
        match sh {
            Some(c) => s.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(c))),
            None => s.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"auto\"/>"),
        }
    }
    if let Some(tabs) = &f.tabs {
        if !tabs.is_empty() {
            s.push_str("<w:tabs>");
            for t in tabs {
                let kind = match t.kind {
                    TabKind::Left => "left",
                    TabKind::Center => "center",
                    TabKind::Right => "right",
                };
                let leader = match t.leader {
                    Leader::None => "none",
                    Leader::Dots => "dot",
                    Leader::Dashes => "hyphen",
                    Leader::Line => "underscore",
                };
                s.push_str(&format!("<w:tab w:val=\"{kind}\" w:leader=\"{leader}\" w:pos=\"{}\"/>", twips(t.pos)));
            }
            s.push_str("</w:tabs>");
        }
    }
    if f.before.is_some() || f.after.is_some() || f.line.is_some() {
        let mut a = String::new();
        if let Some(b) = f.before {
            a.push_str(&format!(" w:before=\"{}\"", twips(b)));
        }
        if let Some(b) = f.after {
            a.push_str(&format!(" w:after=\"{}\"", twips(b)));
        }
        match f.line {
            Some(LineSpacing::Multiple(m)) => a.push_str(&format!(" w:line=\"{}\" w:lineRule=\"auto\"", (m * 240.0).round() as i64)),
            Some(LineSpacing::Exact(v)) => a.push_str(&format!(" w:line=\"{}\" w:lineRule=\"exact\"", twips(v))),
            Some(LineSpacing::AtLeast(v)) => a.push_str(&format!(" w:line=\"{}\" w:lineRule=\"atLeast\"", twips(v))),
            None => {}
        }
        s.push_str(&format!("<w:spacing{a}/>"));
    }
    if f.left.is_some() || f.right.is_some() || f.first.is_some() {
        let mut a = String::new();
        if let Some(v) = f.left {
            a.push_str(&format!(" w:left=\"{}\"", twips(v)));
        }
        if let Some(v) = f.right {
            a.push_str(&format!(" w:right=\"{}\"", twips(v)));
        }
        if let Some(v) = f.first {
            if v < 0.0 {
                a.push_str(&format!(" w:hanging=\"{}\"", twips(-v)));
            } else {
                a.push_str(&format!(" w:firstLine=\"{}\"", twips(v)));
            }
        }
        s.push_str(&format!("<w:ind{a}/>"));
    }
    if let Some(al) = f.align {
        s.push_str(&format!("<w:jc w:val=\"{}\"/>", match al {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "both",
        }));
    }
    if let Some(o) = outline {
        s.push_str(&format!("<w:outlineLvl w:val=\"{}\"/>", o.saturating_sub(1)));
    }
    s
}

fn run_wrap(rp: &str, inner: &str) -> String {
    if rp.is_empty() {
        format!("<w:r>{inner}</w:r>")
    } else {
        format!("<w:r><w:rPr>{rp}</w:rPr>{inner}</w:r>")
    }
}

fn text_runs(text: &str, rp: &str) -> String {
    let mut out = String::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, out: &mut String| {
        if !buf.is_empty() {
            out.push_str(&run_wrap(rp, &format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(buf))));
            buf.clear();
        }
    };
    for c in text.chars() {
        match c {
            '\t' => {
                flush(&mut buf, &mut out);
                out.push_str(&run_wrap(rp, "<w:tab/>"));
            }
            '\n' => {
                flush(&mut buf, &mut out);
                out.push_str(&run_wrap(rp, "<w:br/>"));
            }
            PAGE_BREAK => {
                flush(&mut buf, &mut out);
                out.push_str(&run_wrap(rp, "<w:br w:type=\"page\"/>"));
            }
            c => buf.push(c),
        }
    }
    flush(&mut buf, &mut out);
    out
}

fn image_xml(img: &ImageObj, rid: &str, id: u32) -> String {
    let (cx, cy) = ((img.w * 12700.0) as i64, (img.h * 12700.0) as i64);
    format!(
        "<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"Picture {id}\"/><wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"Picture {id}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>"
    )
}

fn write_para(p: &Paragraph, ctx: &mut Ctx, rels: &mut Rels, styles: &Styles) -> String {
    let mut o = String::from("<w:p>");
    let style = if p.style == "Normal" { None } else { Some(p.style.as_str()) };
    let pp = ppr(&p.fmt, style, p.list, None);
    if !pp.is_empty() {
        o.push_str(&format!("<w:pPr>{pp}</w:pPr>"));
    }
    let _ = styles;
    let mut i = 0;
    while i < p.runs.len() {
        let r = &p.runs[i];
        if let Some(link) = &r.link {
            // group consecutive runs with the same link
            let mut j = i;
            let mut inner = String::new();
            while j < p.runs.len() && p.runs[j].link.as_ref() == Some(link) {
                inner.push_str(&text_runs(&p.runs[j].text, &rpr(&p.runs[j].fmt, true)));
                j += 1;
            }
            let rid = rels.add("hyperlink", link.clone(), true);
            o.push_str(&format!("<w:hyperlink r:id=\"{rid}\" w:history=\"1\">{inner}</w:hyperlink>"));
            i = j;
            continue;
        }
        match &r.obj {
            Some(Obj::Image(img)) => {
                let name = match ctx.media_names.get(&img.data.id) {
                    Some(n) => n.clone(),
                    None => {
                        let n = format!("image{}.{}", ctx.media.len() + 1, img.data.ext());
                        ctx.media_names.insert(img.data.id, n.clone());
                        ctx.media.push((n.clone(), img.data.clone()));
                        n
                    }
                };
                let rid = rels.add("image", format!("media/{name}"), false);
                ctx.docpr += 1;
                o.push_str(&run_wrap(&rpr(&r.fmt, false), &image_xml(img, &rid, ctx.docpr)));
            }
            Some(Obj::Field(f)) => {
                let rp = rpr(&r.fmt, false);
                let (instr, shown) = match f {
                    Field::Page => (" PAGE ", "1"),
                    Field::NumPages => (" NUMPAGES ", "1"),
                };
                o.push_str(&run_wrap(&rp, "<w:fldChar w:fldCharType=\"begin\"/>"));
                o.push_str(&run_wrap(&rp, &format!("<w:instrText xml:space=\"preserve\">{instr}</w:instrText>")));
                o.push_str(&run_wrap(&rp, "<w:fldChar w:fldCharType=\"separate\"/>"));
                o.push_str(&run_wrap(&rp, &format!("<w:t>{shown}</w:t>")));
                o.push_str(&run_wrap(&rp, "<w:fldChar w:fldCharType=\"end\"/>"));
            }
            None => o.push_str(&text_runs(&r.text, &rpr(&r.fmt, false))),
        }
        i += 1;
    }
    o.push_str("</w:p>");
    o
}

fn border_el(tag: &str, b: &Option<Border>) -> String {
    match b {
        Some(b) => format!("<w:{tag} w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{}\"/>", ((b.w * 8.0).round() as i64).clamp(2, 96), hex(b.color)),
        None => format!("<w:{tag} w:val=\"nil\"/>"),
    }
}

fn write_table(t: &Table, ctx: &mut Ctx, rels: &mut Rels, styles: &Styles) -> String {
    let mut o = String::from("<w:tbl><w:tblPr>");
    o.push_str(&format!("<w:tblW w:w=\"{}\" w:type=\"dxa\"/>", twips(t.width())));
    match t.align {
        Align::Center => o.push_str("<w:jc w:val=\"center\"/>"),
        Align::Right => o.push_str("<w:jc w:val=\"right\"/>"),
        _ => {}
    }
    o.push_str(&format!(
        "<w:tblBorders>{}{}{}{}{}{}</w:tblBorders><w:tblLayout w:type=\"fixed\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"{}\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"{}\" w:type=\"dxa\"/></w:tblCellMar>",
        border_el("top", &t.borders.outer),
        border_el("left", &t.borders.outer),
        border_el("bottom", &t.borders.outer),
        border_el("right", &t.borders.outer),
        border_el("insideH", &t.borders.inner_h),
        border_el("insideV", &t.borders.inner_v),
        twips(t.pad_lr),
        twips(t.pad_lr)
    ));
    o.push_str("<w:tblLook w:val=\"0000\"/></w:tblPr><w:tblGrid>");
    for w in &t.cols {
        o.push_str(&format!("<w:gridCol w:w=\"{}\"/>", twips(*w)));
    }
    o.push_str("</w:tblGrid>");
    for (ri, row) in t.rows.iter().enumerate() {
        o.push_str("<w:tr>");
        let mut trpr = String::new();
        if let Some(h) = row.height {
            trpr.push_str(&format!("<w:trHeight w:val=\"{}\" w:hRule=\"atLeast\"/>", twips(h)));
        }
        if row.header {
            trpr.push_str("<w:tblHeader/>");
        }
        if !trpr.is_empty() {
            o.push_str(&format!("<w:trPr>{trpr}</w:trPr>"));
        }
        for (ci, cell) in row.cells.iter().enumerate() {
            let (cs, vm, content): (usize, Option<bool>, Option<&Cell>) = if cell.covered {
                let (or, oc) = Story::origin_of(t, ri, ci);
                if or == ri || oc != ci {
                    continue; // horizontal continuation or interior of a span
                }
                (t.rows[or].cells[oc].cs, Some(false), None)
            } else {
                (cell.cs, if cell.rs > 1 { Some(true) } else { None }, Some(cell))
            };
            let w: f32 = t.cols[ci..(ci + cs).min(t.cols.len())].iter().sum();
            o.push_str("<w:tc><w:tcPr>");
            o.push_str(&format!("<w:tcW w:w=\"{}\" w:type=\"dxa\"/>", twips(w)));
            if cs > 1 {
                o.push_str(&format!("<w:gridSpan w:val=\"{cs}\"/>"));
            }
            match vm {
                Some(true) => o.push_str("<w:vMerge w:val=\"restart\"/>"),
                Some(false) => o.push_str("<w:vMerge/>"),
                None => {}
            }
            if let Some(c) = content {
                if let Some(sh) = c.shade {
                    o.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>", hex(sh)));
                }
                match c.valign {
                    VAlign::Center => o.push_str("<w:vAlign w:val=\"center\"/>"),
                    VAlign::Bottom => o.push_str("<w:vAlign w:val=\"bottom\"/>"),
                    VAlign::Top => {}
                }
            }
            o.push_str("</w:tcPr>");
            match content {
                Some(c) => {
                    for p in &c.paras {
                        o.push_str(&write_para(p, ctx, rels, styles));
                    }
                }
                None => o.push_str("<w:p/>"),
            }
            o.push_str("</w:tc>");
        }
        o.push_str("</w:tr>");
    }
    o.push_str("</w:tbl>");
    let _ = TableRef { b: 0, r: 0, c: 0 };
    o
}

fn write_story(st: &Story, ctx: &mut Ctx, rels: &mut Rels, styles: &Styles) -> String {
    let mut o = String::new();
    for blk in &st.blocks {
        match &**blk {
            Block::Para(p) => o.push_str(&write_para(p, ctx, rels, styles)),
            Block::Table(t) => o.push_str(&write_table(t, ctx, rels, styles)),
        }
    }
    o
}

fn collect_lists(st: &Story, ctx: &mut Ctx) {
    for i in 0..st.len() {
        if let Some(l) = st.para(i).list {
            let e = ctx.lists.entry(l.id).or_insert([ListKind::Decimal; 9]);
            if (l.level as usize) < 9 {
                e[l.level as usize] = l.kind;
            }
        }
    }
}

fn numbering_xml(ctx: &Ctx) -> String {
    let mut s = String::from(XML_HEAD);
    s.push_str(&format!("<w:numbering {NS}>"));
    for (id, kinds) in &ctx.lists {
        s.push_str(&format!("<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>"));
        for (lvl, kind) in kinds.iter().enumerate() {
            // a level that was never used inherits the style of level 0
            let kind = if *kind == ListKind::Decimal && lvl > 0 && kinds[0] == ListKind::Bullet { ListKind::Bullet } else { *kind };
            let (fmt, text) = match kind {
                ListKind::Bullet => ("bullet", ["\u{2022}", "o", "\u{25AA}"][lvl % 3].to_string()),
                ListKind::Decimal => ("decimal", format!("%{}.", lvl + 1)),
                ListKind::LowerAlpha => ("lowerLetter", format!("%{}.", lvl + 1)),
                ListKind::UpperAlpha => ("upperLetter", format!("%{}.", lvl + 1)),
                ListKind::LowerRoman => ("lowerRoman", format!("%{}.", lvl + 1)),
                ListKind::UpperRoman => ("upperRoman", format!("%{}.", lvl + 1)),
            };
            let font = if kind == ListKind::Bullet { "<w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:hint=\"default\"/></w:rPr>" } else { "" };
            s.push_str(&format!(
                "<w:lvl w:ilvl=\"{lvl}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr>{font}</w:lvl>",
                esc(&text),
                720 * (lvl + 1)
            ));
        }
        s.push_str("</w:abstractNum>");
    }
    for id in ctx.lists.keys() {
        s.push_str(&format!("<w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{id}\"/></w:num>"));
    }
    s.push_str("</w:numbering>");
    s
}

fn styles_xml(styles: &Styles) -> String {
    let mut s = String::from(XML_HEAD);
    s.push_str(&format!("<w:styles {NS}><w:docDefaults><w:rPrDefault><w:rPr>{}</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>{}</w:pPr></w:pPrDefault></w:docDefaults>", rpr(&styles.def_char, false), ppr(&styles.def_para, None, None, None)));
    s.push_str("<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>");
    s.push_str("<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/><w:uiPriority w:val=\"1\"/><w:semiHidden/><w:unhideWhenUsed/></w:style>");
    s.push_str("<w:style w:type=\"table\" w:default=\"1\" w:styleId=\"TableNormal\"><w:name w:val=\"Normal Table\"/><w:uiPriority w:val=\"99\"/><w:semiHidden/><w:unhideWhenUsed/><w:tblPr><w:tblInd w:w=\"0\" w:type=\"dxa\"/><w:tblCellMar><w:top w:w=\"0\" w:type=\"dxa\"/><w:left w:w=\"108\" w:type=\"dxa\"/><w:bottom w:w=\"0\" w:type=\"dxa\"/><w:right w:w=\"108\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr></w:style>");
    for st in &styles.list {
        if st.name == "Normal" {
            continue;
        }
        let based = st.based_on.as_deref().map(|b| format!("<w:basedOn w:val=\"{}\"/>", style_id(b))).unwrap_or_default();
        let next = st.next.as_deref().map(|b| format!("<w:next w:val=\"{}\"/>", style_id(b))).unwrap_or_default();
        let pp = ppr(&st.para, None, None, st.outline);
        let rp = rpr(&st.ch, false);
        s.push_str(&format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"{}\"><w:name w:val=\"{}\"/>{based}{next}<w:qFormat/>{}{}</w:style>",
            style_id(&st.name),
            esc(&word_style_name(&st.name)),
            if pp.is_empty() { String::new() } else { format!("<w:pPr>{pp}</w:pPr>") },
            if rp.is_empty() { String::new() } else { format!("<w:rPr>{rp}</w:rPr>") }
        ));
    }
    s.push_str("</w:styles>");
    s
}

fn is_blank(st: &Story) -> bool {
    st.len() == 1 && st.blocks.len() == 1 && st.para(0).is_empty() && st.para(0).runs.iter().all(|r| r.obj.is_none())
}

pub fn write(doc: &Document) -> Result<Vec<u8>, String> {
    let mut ctx = Ctx { media: vec![], media_names: HashMap::new(), docpr: 0, lists: BTreeMap::new() };
    collect_lists(&doc.body, &mut ctx);
    let mut doc_rels = Rels::default();
    doc_rels.add("styles", "styles.xml".into(), false);
    let body = write_story(&doc.body, &mut ctx, &mut doc_rels, &doc.styles);

    // headers and footers
    struct HfPart {
        name: String,
        xml: String,
        rels: Rels,
        is_header: bool,
        typ: &'static str,
    }
    let mut parts: Vec<HfPart> = vec![];
    let mut sect_refs = String::new();
    let candidates: Vec<(&Story, bool, &'static str)> = if doc.page.diff_first {
        vec![(&doc.header, true, "default"), (&doc.first_header, true, "first"), (&doc.footer, false, "default"), (&doc.first_footer, false, "first")]
    } else {
        vec![(&doc.header, true, "default"), (&doc.footer, false, "default")]
    };
    for (st, is_header, typ) in candidates {
        if !doc.page.diff_first && is_blank(st) {
            continue;
        }
        collect_lists(st, &mut ctx);
        let mut rels = Rels::default();
        let tag = if is_header { "hdr" } else { "ftr" };
        let inner = write_story(st, &mut ctx, &mut rels, &doc.styles);
        let n = parts.iter().filter(|p| p.is_header == is_header).count() + 1;
        let name = format!("{}{}.xml", if is_header { "header" } else { "footer" }, n);
        let xml = format!("{XML_HEAD}<w:{tag} {NS}>{inner}</w:{tag}>");
        let rid = doc_rels.add(if is_header { "header" } else { "footer" }, name.clone(), false);
        sect_refs.push_str(&format!("<w:{}Reference w:type=\"{typ}\" r:id=\"{rid}\"/>", if is_header { "header" } else { "footer" }));
        parts.push(HfPart { name, xml, rels, is_header, typ });
    }
    let _ = parts.iter().map(|p| p.typ).count();
    let has_numbering = !ctx.lists.is_empty();
    if has_numbering {
        doc_rels.add("numbering", "numbering.xml".into(), false);
    }
    doc_rels.add("settings", "settings.xml".into(), false);

    let pg = &doc.page;
    let sect = format!(
        "<w:sectPr>{sect_refs}<w:pgSz w:w=\"{}\" w:h=\"{}\"{}/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"{}\" w:footer=\"{}\" w:gutter=\"0\"/>{}</w:sectPr>",
        twips(pg.w),
        twips(pg.h),
        if pg.landscape() { " w:orient=\"landscape\"" } else { "" },
        twips(pg.top),
        twips(pg.right),
        twips(pg.bottom),
        twips(pg.left),
        twips(pg.header_dist),
        twips(pg.footer_dist),
        if pg.diff_first { "<w:titlePg/>" } else { "" }
    );
    let document = format!("{XML_HEAD}<w:document {NS}><w:body>{body}{sect}</w:body></w:document>");

    let mut ct = String::from(XML_HEAD);
    ct.push_str("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/><Default Extension=\"gif\" ContentType=\"image/gif\"/><Default Extension=\"bmp\" ContentType=\"image/bmp\"/><Default Extension=\"webp\" ContentType=\"image/webp\"/>");
    ct.push_str("<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>");
    if has_numbering {
        ct.push_str("<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>");
    }
    for p in &parts {
        ct.push_str(&format!("<Override PartName=\"/word/{}\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{}+xml\"/>", p.name, if p.is_header { "header" } else { "footer" }));
    }
    ct.push_str("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/><Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/></Types>");

    let root_rels = format!("{XML_HEAD}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/><Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/></Relationships>");
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let core = format!(
        "{XML_HEAD}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>{}</dc:title><dc:creator>{}</dc:creator><dcterms:created xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">{now}</dcterms:modified></cp:coreProperties>",
        esc(&doc.meta.title),
        esc(&doc.meta.author)
    );
    let app = format!("{XML_HEAD}<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Word RS</Application></Properties>");
    let settings = format!("{XML_HEAD}<w:settings {NS}><w:defaultTabStop w:val=\"720\"/><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>");

    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut put = |name: &str, data: &[u8]| -> Result<(), String> {
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
            zip.write_all(data).map_err(|e| e.to_string())
        };
        put("[Content_Types].xml", ct.as_bytes())?;
        put("_rels/.rels", root_rels.as_bytes())?;
        put("docProps/core.xml", core.as_bytes())?;
        put("docProps/app.xml", app.as_bytes())?;
        put("word/document.xml", document.as_bytes())?;
        put("word/_rels/document.xml.rels", doc_rels.xml().as_bytes())?;
        put("word/styles.xml", styles_xml(&doc.styles).as_bytes())?;
        put("word/settings.xml", settings.as_bytes())?;
        if has_numbering {
            put("word/numbering.xml", numbering_xml(&ctx).as_bytes())?;
        }
        for p in &parts {
            put(&format!("word/{}", p.name), p.xml.as_bytes())?;
            if !p.rels.items.is_empty() {
                put(&format!("word/_rels/{}.rels", p.name), p.rels.xml().as_bytes())?;
            }
        }
        for (name, data) in &ctx.media {
            put(&format!("word/media/{name}"), &data.bytes)?;
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    Ok(buf.into_inner())
}

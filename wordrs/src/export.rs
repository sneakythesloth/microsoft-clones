//! Text, Markdown and HTML exporters.

use base64::Engine;

use crate::layout::compute_markers;
use crate::model::*;

pub fn text(doc: &Document) -> String {
    let mut s = doc.body.plain_text();
    s.push('\n');
    s
}

fn md_escape(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        if "\\`*_{}[]#|".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn md_inline(p: &Paragraph, base_bold: bool, base_italic: bool) -> String {
    let mut out = String::new();
    for r in &p.runs {
        if r.text.is_empty() {
            continue;
        }
        if let Some(Obj::Image(img)) = &r.obj {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&img.data.bytes);
            out.push_str(&format!("![image](data:{};base64,{})", img.data.mime, b64));
            continue;
        }
        let mut t = md_escape(&r.text.replace(PAGE_BREAK, "").replace('\n', "  \n"));
        if t.trim().is_empty() {
            out.push_str(&t);
            continue;
        }
        let lead: String = t.chars().take_while(|c| c.is_whitespace()).collect();
        let trail: String = t.chars().rev().take_while(|c| c.is_whitespace()).collect::<String>().chars().rev().collect();
        t = t.trim().to_string();
        if r.fmt.strike == Some(true) {
            t = format!("~~{t}~~");
        }
        if r.fmt.italic.unwrap_or(base_italic) {
            t = format!("*{t}*");
        }
        if r.fmt.bold.unwrap_or(base_bold) {
            t = format!("**{t}**");
        }
        if let Some(l) = &r.link {
            t = format!("[{t}]({l})");
        }
        out.push_str(&lead);
        out.push_str(&t);
        out.push_str(&trail);
    }
    out
}

pub fn markdown(doc: &Document) -> String {
    let story = &doc.body;
    let markers = compute_markers(story);
    let mut out = String::new();
    let mut flat = 0usize;
    for blk in &story.blocks {
        match &**blk {
            Block::Para(p) => {
                let (base, _) = doc.styles.resolve(&p.style);
                let body = md_inline(p, base.bold, base.italic);
                let lvl = doc.styles.outline_level(&p.style);
                if let Some(l) = lvl {
                    if !body.trim().is_empty() {
                        out.push_str(&format!("{} {}\n\n", "#".repeat(l.min(6) as usize), body.trim()));
                    }
                } else if let Some(li) = p.list {
                    let indent = "  ".repeat(li.level as usize);
                    let marker = if li.kind == ListKind::Bullet { "-".to_string() } else { markers[flat].clone().unwrap_or_else(|| "1.".into()) };
                    out.push_str(&format!("{indent}{marker} {}\n", body.trim()));
                } else if p.style == "Quote" || p.style == "Intense Quote" {
                    out.push_str(&format!("> {}\n\n", body.trim()));
                } else if p.style == "Code" {
                    out.push_str(&format!("    {}\n", p.plain()));
                } else if body.trim().is_empty() {
                    if !out.ends_with("\n\n") {
                        out.push('\n');
                    }
                } else {
                    out.push_str(&format!("{}\n\n", body.trim()));
                }
                flat += 1;
            }
            Block::Table(t) => {
                if !out.ends_with("\n\n") {
                    out.push('\n');
                }
                for (ri, row) in t.rows.iter().enumerate() {
                    let cells: Vec<String> = row
                        .cells
                        .iter()
                        .map(|c| c.paras.iter().map(|p| md_inline(p, false, false).trim().to_string()).collect::<Vec<_>>().join("<br>"))
                        .collect();
                    out.push_str(&format!("| {} |\n", cells.join(" | ")));
                    if ri == 0 {
                        out.push_str(&format!("|{}|\n", vec![" --- "; cells.len()].join("|")));
                    }
                }
                out.push('\n');
                for r in &t.rows {
                    for c in &r.cells {
                        if !c.covered {
                            flat += c.paras.len();
                        }
                    }
                }
            }
        }
    }
    out
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn generic_family(name: &str) -> &'static str {
    let l = name.to_lowercase();
    if ["times", "georgia", "garamond", "cambria", "palatino", "serif", "tinos", "playfair", "merriweather", "lora"].iter().any(|k| l.contains(k)) && !l.contains("sans") {
        "serif"
    } else if ["courier", "consolas", "mono", "code", "typewriter"].iter().any(|k| l.contains(k)) {
        "monospace"
    } else {
        "sans-serif"
    }
}

fn char_css(r: &RChar, base: Option<&RChar>) -> String {
    let mut s = String::new();
    let differs = |f: &dyn Fn(&RChar) -> bool| base.map(|b| f(b) != f(r)).unwrap_or(true);
    if differs(&|x| x.font.as_ref() == "") || base.map(|b| b.font != r.font).unwrap_or(true) {
        s.push_str(&format!("font-family:'{}',{};", r.font, generic_family(&r.font)));
    }
    if base.map(|b| (b.size - r.size).abs() > 0.01).unwrap_or(true) {
        s.push_str(&format!("font-size:{}pt;", r.size));
    }
    if base.map(|b| b.bold != r.bold).unwrap_or(r.bold) {
        s.push_str(if r.bold { "font-weight:bold;" } else { "font-weight:normal;" });
    }
    if base.map(|b| b.italic != r.italic).unwrap_or(r.italic) {
        s.push_str(if r.italic { "font-style:italic;" } else { "font-style:normal;" });
    }
    let mut deco = vec![];
    if r.underline {
        deco.push("underline");
    }
    if r.strike {
        deco.push("line-through");
    }
    if !deco.is_empty() {
        s.push_str(&format!("text-decoration:{};", deco.join(" ")));
    }
    if base.map(|b| b.color != r.color).unwrap_or(true) {
        s.push_str(&format!("color:{};", hex(r.color)));
    }
    if let Some(h) = r.highlight {
        s.push_str(&format!("background:{};", hex(h)));
    }
    if r.small_caps {
        s.push_str("font-variant:small-caps;");
    }
    if r.all_caps {
        s.push_str("text-transform:uppercase;");
    }
    if r.spacing != 0.0 {
        s.push_str(&format!("letter-spacing:{}pt;", r.spacing));
    }
    s
}

fn para_html(doc: &Document, p: &Paragraph, marker: Option<&str>, in_cell: bool) -> String {
    let (base, mut rp) = doc.styles.resolve(&p.style);
    rp.apply(&p.fmt);
    if let Some(li) = p.list {
        if p.fmt.left.is_none() {
            rp.left = 36.0 * (li.level as f32 + 1.0);
        }
        if p.fmt.first.is_none() {
            rp.first = -18.0;
        }
    }
    if in_cell && p.style == "Normal" {
        if p.fmt.before.is_none() {
            rp.before = 0.0;
        }
        if p.fmt.after.is_none() {
            rp.after = 0.0;
        }
    }
    let mut css = format!("margin:{}pt {}pt {}pt {}pt;", rp.before, rp.right, rp.after, rp.left);
    if rp.first != 0.0 {
        css.push_str(&format!("text-indent:{}pt;", rp.first));
    }
    css.push_str(&format!("text-align:{};", match rp.align {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
    }));
    match rp.line {
        LineSpacing::Multiple(m) => css.push_str(&format!("line-height:{:.2};", m * 1.2)),
        LineSpacing::Exact(v) | LineSpacing::AtLeast(v) => css.push_str(&format!("line-height:{v}pt;")),
    }
    css.push_str(&char_css(&base, None));
    if let Some(s) = rp.shade {
        css.push_str(&format!("background:{};", hex(s)));
    }
    if let Some(b) = rp.borders.bottom {
        css.push_str(&format!("border-bottom:{}pt solid {};", b.w, hex(b.color)));
    }
    if let Some(b) = rp.borders.top {
        css.push_str(&format!("border-top:{}pt solid {};", b.w, hex(b.color)));
    }
    if rp.page_break_before {
        css.push_str("page-break-before:always;");
    }
    let tag = match doc.styles.outline_level(&p.style) {
        Some(l) if l <= 6 => format!("h{l}"),
        _ => "p".to_string(),
    };
    let mut inner = String::new();
    if let Some(m) = marker {
        inner.push_str(&format!("{}&nbsp;&nbsp;", esc(m)));
    }
    for r in &p.runs {
        if r.text.is_empty() {
            continue;
        }
        if let Some(Obj::Image(img)) = &r.obj {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&img.data.bytes);
            inner.push_str(&format!("<img src=\"data:{};base64,{}\" style=\"width:{}pt;height:{}pt\" alt=\"\">", img.data.mime, b64, img.w, img.h));
            continue;
        }
        if let Some(Obj::Field(_)) = &r.obj {
            continue;
        }
        let mut rc = base.clone();
        rc.apply(&r.fmt);
        let mut t = esc(&r.text.replace(PAGE_BREAK, "")).replace('\n', "<br>").replace('\t', "&emsp;");
        if t.contains("  ") {
            t = t.replace("  ", "&nbsp; ");
        }
        let c = char_css(&rc, Some(&base));
        let sup = match rc.vert {
            VertAlign::Super => ("<sup>", "</sup>"),
            VertAlign::Sub => ("<sub>", "</sub>"),
            VertAlign::Baseline => ("", ""),
        };
        let mut piece = if c.is_empty() { t } else { format!("<span style=\"{c}\">{t}</span>") };
        piece = format!("{}{}{}", sup.0, piece, sup.1);
        if let Some(l) = &r.link {
            piece = format!("<a href=\"{}\">{}</a>", esc(l), piece);
        }
        inner.push_str(&piece);
        if r.text.ends_with(PAGE_BREAK) {
            inner.push_str("<div style=\"page-break-after:always\"></div>");
        }
    }
    if inner.is_empty() {
        inner.push_str("&nbsp;");
    }
    format!("<{tag} style=\"{css}\">{inner}</{tag}>\n")
}

/// `is_google(name)` tells whether a family came from Google Fonts (so we can link it).
pub fn html(doc: &Document, is_google: &dyn Fn(&str) -> bool) -> String {
    let mut fams: Vec<String> = vec![];
    let mut note = |f: &str| {
        if is_google(f) && !fams.iter().any(|x| x == f) {
            fams.push(f.to_string());
        }
    };
    note(doc.styles.def_char.font.as_deref().unwrap_or("Calibri"));
    for s in &doc.styles.list {
        if let Some(f) = &s.ch.font {
            note(f);
        }
    }
    let story = &doc.body;
    for i in 0..story.len() {
        for r in &story.para(i).runs {
            if let Some(f) = &r.fmt.font {
                note(f);
            }
        }
    }
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\">\n");
    out.push_str(&format!("<title>{}</title>\n", esc(if doc.meta.title.is_empty() { "Document" } else { &doc.meta.title })));
    for f in &fams {
        out.push_str(&format!("<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family={}:ital,wght@0,400;0,700;1,400;1,700&display=swap\">\n", f.replace(' ', "+")));
    }
    let (base, _) = doc.styles.resolve("Normal");
    out.push_str(&format!(
        "<style>body{{max-width:{}pt;margin:24pt auto;padding:0 {}pt;font-family:'{}',{};font-size:{}pt}}table{{border-collapse:collapse}}td{{vertical-align:top;padding:1pt 5.4pt}}img{{max-width:100%}}</style>\n</head><body>\n",
        doc.page.content_w(), 12, base.font, generic_family(&base.font), base.size
    ));
    let markers = compute_markers(story);
    let mut flat = 0usize;
    for blk in &story.blocks {
        match &**blk {
            Block::Para(p) => {
                out.push_str(&para_html(doc, p, markers[flat].as_deref(), false));
                flat += 1;
            }
            Block::Table(t) => {
                let bd = |b: Option<Border>| b.map(|b| format!("{}pt solid {}", b.w, hex(b.color))).unwrap_or_else(|| "none".into());
                out.push_str("<table style=\"table-layout:fixed;");
                out.push_str(match t.align {
                    Align::Center => "margin:0 auto;",
                    Align::Right => "margin-left:auto;",
                    _ => "",
                });
                out.push_str(&format!("width:{}pt\">\n", t.width()));
                let nr = t.rows.len();
                for (ri, row) in t.rows.iter().enumerate() {
                    out.push_str("<tr>");
                    for (ci, c) in row.cells.iter().enumerate() {
                        if c.covered {
                            continue;
                        }
                        let w: f32 = t.cols[ci..(ci + c.cs).min(t.cols.len())].iter().sum();
                        let top = if ri == 0 { t.borders.outer } else { t.borders.inner_h };
                        let bottom = if ri + c.rs >= nr { t.borders.outer } else { t.borders.inner_h };
                        let left = if ci == 0 { t.borders.outer } else { t.borders.inner_v };
                        let right = if ci + c.cs >= t.cols.len() { t.borders.outer } else { t.borders.inner_v };
                        let mut css = format!("width:{w}pt;border-top:{};border-bottom:{};border-left:{};border-right:{};", bd(top), bd(bottom), bd(left), bd(right));
                        if let Some(s) = c.shade {
                            css.push_str(&format!("background:{};", hex(s)));
                        }
                        css.push_str(match c.valign {
                            VAlign::Top => "vertical-align:top;",
                            VAlign::Center => "vertical-align:middle;",
                            VAlign::Bottom => "vertical-align:bottom;",
                        });
                        out.push_str(&format!("<td colspan=\"{}\" rowspan=\"{}\" style=\"{}\">", c.cs, c.rs, css));
                        for p in &c.paras {
                            out.push_str(&para_html(doc, p, markers.get(flat).and_then(|m| m.as_deref()), true));
                            flat += 1;
                        }
                        out.push_str("</td>");
                    }
                    out.push_str("</tr>\n");
                }
                out.push_str("</table>\n");
            }
        }
    }
    out.push_str("</body></html>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Document {
        let mut d = Document::new();
        let mut h = Paragraph::styled("Heading 1", "Title here");
        h.touch();
        let mut p = Paragraph::with_text("Hello ");
        p.runs.push(Run::text("bold", CharFmt { bold: Some(true), ..Default::default() }));
        p.runs.push(Run { text: " link".into(), fmt: CharFmt::default(), link: Some("https://example.com".into()), obj: None });
        let mut li = Paragraph::with_text("item one");
        li.list = Some(ListInfo { id: 1, level: 0, kind: ListKind::Bullet });
        d.body = crate::story::Story::from_blocks(vec![Block::Para(h), Block::Para(p), Block::Para(li)]);
        d
    }

    #[test]
    fn markdown_basics() {
        let md = markdown(&sample());
        assert!(md.contains("# Title here"));
        assert!(md.contains("**bold**"));
        assert!(md.contains("[link](https://example.com)"));
        assert!(md.contains("- item one"));
    }

    #[test]
    fn html_basics() {
        let h = html(&sample(), &|f| f == "Calibri");
        assert!(h.contains("<h1 "));
        assert!(h.contains("font-weight:bold"));
        assert!(h.contains("<a href=\"https://example.com\">"));
        assert!(h.contains("fonts.googleapis.com/css2?family=Calibri"));
    }
}

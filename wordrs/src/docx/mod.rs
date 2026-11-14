//! Microsoft Word (.docx / OOXML) import and export.

mod read;
mod write;

pub use read::read;
pub use write::write;

pub(crate) fn twips(pt: f32) -> i64 {
    (pt * 20.0).round() as i64
}

pub(crate) fn hex(c: crate::model::Rgb) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Names of the 16 highlight colours Word supports.
pub(crate) const HIGHLIGHTS: [(&str, [u8; 3]); 15] = [
    ("yellow", [255, 255, 0]),
    ("green", [0, 255, 0]),
    ("cyan", [0, 255, 255]),
    ("magenta", [255, 0, 255]),
    ("blue", [0, 0, 255]),
    ("red", [255, 0, 0]),
    ("darkBlue", [0, 0, 128]),
    ("darkCyan", [0, 128, 128]),
    ("darkGreen", [0, 128, 0]),
    ("darkMagenta", [128, 0, 128]),
    ("darkRed", [128, 0, 0]),
    ("darkYellow", [128, 128, 0]),
    ("darkGray", [128, 128, 128]),
    ("lightGray", [192, 192, 192]),
    ("black", [0, 0, 0]),
];

pub(crate) fn style_id(name: &str) -> String {
    let id: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
    if id.is_empty() {
        "Style".into()
    } else {
        id
    }
}

/// Word's internal (lowercase) names for built-in styles.
pub(crate) fn word_style_name(name: &str) -> String {
    if let Some(n) = name.strip_prefix("Heading ") {
        return format!("heading {n}");
    }
    if let Some(n) = name.strip_prefix("TOC ") {
        return format!("toc {n}");
    }
    match name {
        "Caption" => "caption".into(),
        "Header" => "header".into(),
        "Footer" => "footer".into(),
        other => other.to_string(),
    }
}

/// Inverse of `word_style_name`.
pub(crate) fn our_style_name(word: &str) -> String {
    let l = word.to_lowercase();
    if let Some(n) = l.strip_prefix("heading ") {
        return format!("Heading {n}");
    }
    if let Some(n) = l.strip_prefix("toc ") {
        return format!("TOC {n}");
    }
    match l.as_str() {
        "caption" => "Caption".into(),
        "header" => "Header".into(),
        "footer" => "Footer".into(),
        "title" => "Title".into(),
        "subtitle" => "Subtitle".into(),
        "quote" => "Quote".into(),
        "intense quote" => "Intense Quote".into(),
        "list paragraph" => "List Paragraph".into(),
        "no spacing" => "No Spacing".into(),
        "normal" => "Normal".into(),
        _ => word.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::model::*;
    use crate::story::{Story, TableRef};
    use std::sync::Arc;

    fn png() -> Arc<ImageData> {
        let img = image::RgbaImage::from_fn(40, 20, |x, y| image::Rgba([(x * 6) as u8, (y * 12) as u8, 120, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        ImageData::from_bytes(buf.into_inner()).unwrap()
    }

    pub(super) fn sample_doc() -> Document {
        let mut d = Document::new();
        d.meta.title = "Round trip".into();
        d.meta.author = "Tester".into();
        let h = Paragraph::styled("Heading 1", "Quarterly Report");
        let mut p = Paragraph::with_text("Plain, ");
        p.runs.push(Run::text("bold red", CharFmt { bold: Some(true), color: Some([200, 0, 0]), ..Default::default() }));
        p.runs.push(Run::text(" and ", CharFmt::default()));
        p.runs.push(Run::text("italic Georgia 14pt", CharFmt { italic: Some(true), font: Some("Georgia".into()), size: Some(14.0), ..Default::default() }));
        p.runs.push(Run::text(" x", CharFmt::default()));
        p.runs.push(Run::text("2", CharFmt { vert: Some(VertAlign::Super), ..Default::default() }));
        p.runs.push(Run::text(" highlighted", CharFmt { highlight: Some(Some([255, 255, 0])), underline: Some(true), ..Default::default() }));
        p.runs.push(Run { text: " link".into(), fmt: CharFmt::default(), link: Some("https://example.com/a?b=1&c=2".into()), obj: None });
        p.fmt.align = Some(Align::Justify);
        p.fmt.before = Some(6.0);
        p.fmt.line = Some(LineSpacing::Multiple(1.5));
        p.fmt.left = Some(18.0);
        p.fmt.first = Some(-9.0);
        let mut tabbed = Paragraph::with_text("Name\tValue\nsecond line");
        tabbed.fmt.tabs = Some(vec![TabStop { pos: 200.0, kind: TabKind::Right, leader: Leader::Dots }]);
        let mut b1 = Paragraph::with_text("first bullet");
        b1.list = Some(ListInfo { id: 1, level: 0, kind: ListKind::Bullet });
        let mut b2 = Paragraph::with_text("nested bullet");
        b2.list = Some(ListInfo { id: 1, level: 1, kind: ListKind::Bullet });
        let mut n1 = Paragraph::with_text("step one");
        n1.list = Some(ListInfo { id: 2, level: 0, kind: ListKind::Decimal });
        let mut n2 = Paragraph::with_text("step two");
        n2.list = Some(ListInfo { id: 2, level: 0, kind: ListKind::Decimal });
        let mut img_p = Paragraph::with_text("Picture: ");
        img_p.runs.push(Run { text: OBJ_CHAR.to_string(), fmt: CharFmt::default(), link: None, obj: Some(Obj::Image(ImageObj { data: png(), w: 80.0, h: 40.0 })) });
        let tail = Paragraph::with_text("After the table & <escaped> \"text\".");
        let mut s = Story::from_blocks(vec![Block::Para(h), Block::Para(p), Block::Para(tabbed), Block::Para(b1), Block::Para(b2), Block::Para(n1), Block::Para(n2), Block::Para(img_p), Block::Para(Paragraph::with_text("before table")), Block::Para(tail)]);
        // table after "before table" (index 8)
        let at = crate::model::Pos::new(8, 12);
        let tp = s.insert_table(at, 3, 3, 400.0).unwrap();
        s.insert_text(tp, "A", None);
        let tr = s.table_at(tp.p).unwrap();
        s.table_set_shade(tr.b, 0, 2, 0, 2, Some([200, 220, 255]));
        s.table_merge(tr.b, 1, 0, 2, 1);
        let p2 = s.cell_first_para(tr.b, 1, 0).unwrap();
        s.insert_text(Pos::new(p2, 0), "merged", None);
        s.tbl_mut(tr.b).rows[0].header = true;
        let _ = TableRef { b: 0, r: 0, c: 0 };
        let last = s.len() - 1;
        let n = s.plen(last);
        s.insert_page_break(Pos::new(last, n));
        s.insert_text(Pos::new(s.len() - 1, 0), "Last page", None);
        d.body = s;
        // header / footer with fields
        let hp = d.header.para_mut(0);
        hp.insert(0, "Confidential\t", None, None);
        let ft = d.footer.para_mut(0);
        ft.fmt.align = Some(Align::Center);
        ft.insert(0, "Page ", None, None);
        ft.insert_obj(5, Obj::Field(Field::Page), None);
        ft.insert(6, " of ", None, None);
        ft.insert_obj(10, Obj::Field(Field::NumPages), None);
        d.page = PageSetup { w: 595.3, h: 841.9, top: 60.0, bottom: 60.0, left: 70.0, right: 70.0, header_dist: 30.0, footer_dist: 30.0, diff_first: false };
        d
    }

    #[test]
    fn docx_roundtrip_preserves_content_and_formatting() {
        let d = sample_doc();
        let bytes = super::write(&d).expect("write");
        std::fs::write("/tmp/wordrs-roundtrip.docx", &bytes).ok();
        let r = super::read(&bytes).expect("read");
        assert_eq!(r.meta.title, "Round trip");
        assert_eq!(r.meta.author, "Tester");
        assert_eq!(r.body.plain_text(), d.body.plain_text());
        // page setup
        assert!((r.page.w - 595.3).abs() < 0.1 && (r.page.left - 70.0).abs() < 0.1);
        // heading style survives, with outline level
        assert_eq!(r.body.para(0).style, "Heading 1");
        assert_eq!(r.styles.outline_level("Heading 1"), Some(1));
        // run formatting
        let p = r.body.para(1);
        assert!(p.runs.iter().any(|x| x.text == "bold red" && x.fmt.bold == Some(true) && x.fmt.color == Some([200, 0, 0])));
        assert!(p.runs.iter().any(|x| x.text == "italic Georgia 14pt" && x.fmt.italic == Some(true) && x.fmt.font.as_deref() == Some("Georgia") && x.fmt.size == Some(14.0)));
        assert!(p.runs.iter().any(|x| x.text == "2" && x.fmt.vert == Some(VertAlign::Super)));
        assert!(p.runs.iter().any(|x| x.fmt.highlight == Some(Some([255, 255, 0]))));
        assert!(p.runs.iter().any(|x| x.link.as_deref() == Some("https://example.com/a?b=1&c=2")));
        assert_eq!(p.fmt.align, Some(Align::Justify));
        assert_eq!(p.fmt.line, Some(LineSpacing::Multiple(1.5)));
        assert_eq!(p.fmt.first, Some(-9.0));
        // tabs and soft line breaks
        let t = r.body.para(2);
        assert_eq!(t.text(), "Name\tValue\nsecond line");
        assert_eq!(t.fmt.tabs.as_ref().unwrap()[0].leader, Leader::Dots);
        // lists
        assert_eq!(r.body.para(3).list.map(|l| (l.id, l.level, l.kind)), Some((1, 0, ListKind::Bullet)));
        assert_eq!(r.body.para(4).list.map(|l| l.level), Some(1));
        assert_eq!(r.body.para(5).list.map(|l| l.kind), Some(ListKind::Decimal));
        // image
        let img = r.body.para(7).runs.iter().find_map(|x| if let Some(Obj::Image(i)) = &x.obj { Some(i.clone()) } else { None }).expect("image survived");
        assert!((img.w - 80.0).abs() < 0.1 && (img.h - 40.0).abs() < 0.1);
        assert_eq!((img.data.px_w, img.data.px_h), (40, 20));
        // table structure incl. merge, shading and header row
        let tb = r.body.blocks.iter().find_map(|b| if let Block::Table(t) = &**b { Some(t.clone()) } else { None }).expect("table");
        assert_eq!(tb.rows.len(), 3);
        assert_eq!((tb.rows[1].cells[0].rs, tb.rows[1].cells[0].cs), (2, 2));
        assert!(tb.rows[2].cells[0].covered && tb.rows[1].cells[1].covered);
        assert_eq!(tb.rows[0].cells[2].shade, Some([200, 220, 255]));
        assert!(tb.rows[0].header);
        assert_eq!(tb.rows[1].cells[0].paras[0].text(), "merged");
        // page break is kept at the end of its paragraph
        assert!(r.body.plain_text().contains("Last page"));
        assert!(r.body.plain_text().contains('\u{c}') || r.body.blocks.iter().any(|b| matches!(&**b, Block::Para(p) if p.text().ends_with(PAGE_BREAK))));
        // header/footer fields
        let f = r.footer.para(0);
        assert!(f.runs.iter().any(|x| matches!(x.obj, Some(Obj::Field(Field::Page)))));
        assert!(f.runs.iter().any(|x| matches!(x.obj, Some(Obj::Field(Field::NumPages)))));
        assert_eq!(f.fmt.align, Some(Align::Center));
        assert!(r.header.para(0).text().starts_with("Confidential"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(super::read(b"not a zip").is_err());
    }
}

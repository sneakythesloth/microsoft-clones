//! PDF export: uses the same layout as the screen, embeds subsetted TrueType fonts
//! (with ToUnicode maps so text stays searchable), images, link annotations and metadata.

use std::collections::{BTreeMap, HashMap};

use pdf_writer::types::{ActionType, AnnotationType, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, Str, TextStr};

use crate::fonts::FontStore;
use crate::layout::*;
use crate::model::*;

#[derive(Default)]
struct FontUse {
    remap: subsetter::GlyphRemapper,
    uni: BTreeMap<u16, char>,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

struct Link {
    rect: [f32; 4],
    url: String,
}

struct Exporter<'a> {
    fonts: &'a FontStore,
    uses: BTreeMap<usize, FontUse>,
    images: HashMap<u64, usize>,
    image_list: Vec<std::sync::Arc<ImageData>>,
}

impl<'a> Exporter<'a> {
    fn encode(&mut self, face: usize, text: &str) -> Vec<(usize, Vec<u8>)> {
        let mut out: Vec<(usize, Vec<u8>)> = vec![];
        for ch in text.chars() {
            let (f, g) = match self.fonts.glyph_for(face, ch) {
                Some(x) => x,
                None => match self.fonts.glyph_for(face, '\u{FFFD}').or_else(|| self.fonts.glyph_for(face, '?')) {
                    Some(x) => x,
                    None => continue,
                },
            };
            let u = self.uses.entry(f).or_default();
            if u.remap.get(0).is_none() {
                u.remap.remap(0);
            }
            let ng = u.remap.remap(g);
            u.uni.entry(ng).or_insert(ch);
            if out.last().map(|l| l.0 != f).unwrap_or(true) {
                out.push((f, vec![]));
            }
            out.last_mut().unwrap().1.extend_from_slice(&ng.to_be_bytes());
        }
        out
    }

    fn image_index(&mut self, img: &std::sync::Arc<ImageData>) -> usize {
        if let Some(&i) = self.images.get(&img.id) {
            return i;
        }
        let i = self.image_list.len();
        self.image_list.push(img.clone());
        self.images.insert(img.id, i);
        i
    }
}

fn col(c: Rgb) -> (f32, f32, f32) {
    (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0)
}

#[allow(clippy::too_many_arguments)]
fn draw_lines(ex: &mut Exporter, c: &mut Content, lines: &[PlacedLine], page_h: f32, links: &mut Vec<Link>) {
    for pl in lines {
        let lay = &pl.layout;
        let ln = &lay.lines[pl.line];
        let base_y = pl.y + ln.base;
        // list marker
        if let Some(m) = &pl.marker {
            let r = &lay.mark;
            let x = pl.x + lay.marker_x;
            show_text(ex, c, m, lay.mark_res.face, r.size, x, page_h - base_y, r.color, 0.0, false, false);
        }
        for it in &ln.items {
            let info = &lay.fmts[it.fi as usize];
            let face = &ex.fonts.faces[info.res.face];
            let x = pl.x + ln.x0 + it.x;
            let base = base_y - it.rise;
            let (asc, desc) = (face.asc * it.size, face.desc * it.size);
            if let Some(h) = info.r.highlight {
                if !matches!(it.kind, ItemKind::Break) {
                    let (r, g, b) = col(h);
                    c.save_state().set_fill_rgb(r, g, b).rect(x, page_h - (base + desc), it.w, asc + desc).fill_nonzero().restore_state();
                }
            }
            match &it.kind {
                ItemKind::Text => show_text(ex, c, &it.text, info.res.face, it.size, x, page_h - base, info.r.color, info.r.spacing, info.res.synth_bold, info.res.synth_italic),
                ItemKind::Field(t) => show_text(ex, c, t, info.res.face, it.size, x, page_h - base, info.r.color, info.r.spacing, info.res.synth_bold, info.res.synth_italic),
                ItemKind::Image(img) => {
                    let idx = ex.image_index(&img.data);
                    let (w, h) = (img.w, img.h);
                    c.save_state().transform([w, 0.0, 0.0, h, x, page_h - base_y]).x_object(Name(format!("Im{idx}").as_bytes())).restore_state();
                }
                ItemKind::Tab(leader) => {
                    let (r, g, b) = col(info.r.color);
                    match leader {
                        Leader::Dots => {
                            c.save_state().set_stroke_rgb(r, g, b).set_line_width(0.8).set_line_cap(pdf_writer::types::LineCapStyle::RoundCap).set_dash_pattern([0.0, 4.0], 0.0);
                            let y = page_h - (base - 0.8);
                            let start = ((x / 4.0).ceil() * 4.0).max(x);
                            c.move_to(start, y).line_to(x + it.w - 1.0, y).stroke().restore_state();
                        }
                        Leader::Dashes => {
                            c.save_state().set_stroke_rgb(r, g, b).set_line_width(0.6).set_dash_pattern([3.0, 2.0], 0.0);
                            let y = page_h - (base - 0.3 * it.size);
                            c.move_to(x, y).line_to(x + it.w, y).stroke().restore_state();
                        }
                        Leader::Line => {
                            c.save_state().set_stroke_rgb(r, g, b).set_line_width(0.6);
                            let y = page_h - (base + 0.5);
                            c.move_to(x, y).line_to(x + it.w, y).stroke().restore_state();
                        }
                        Leader::None => {}
                    }
                }
                _ => {}
            }
            if !matches!(it.kind, ItemKind::Break | ItemKind::Image(_)) {
                let trailing = matches!(it.kind, ItemKind::Space) && it.start >= ln.caret_end && !ln.last;
                let (r, g, b) = col(info.r.color);
                if info.r.underline && !trailing {
                    let y = page_h - (base + (face.ul_pos * it.size).max(0.8));
                    c.save_state().set_stroke_rgb(r, g, b).set_line_width((face.ul_thick * it.size).max(0.5)).move_to(x, y).line_to(x + it.w, y).stroke().restore_state();
                }
                if info.r.strike && !trailing {
                    let y = page_h - (base - 0.3 * it.size);
                    c.save_state().set_stroke_rgb(r, g, b).set_line_width((face.ul_thick * it.size).max(0.5)).move_to(x, y).line_to(x + it.w, y).stroke().restore_state();
                }
            }
            if let Some(url) = &info.link {
                if !matches!(it.kind, ItemKind::Break) {
                    links.push(Link { rect: [x, page_h - (base + desc), x + it.w, page_h - (base - asc)], url: url.clone() });
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn show_text(ex: &mut Exporter, c: &mut Content, text: &str, face: usize, size: f32, x: f32, y: f32, color: Rgb, spacing: f32, synth_bold: bool, synth_italic: bool) {
    if text.is_empty() {
        return;
    }
    let segs = ex.encode(face, text);
    let (r, g, b) = col(color);
    let mut cx = x;
    for (f, bytes) in segs {
        c.begin_text();
        c.set_fill_rgb(r, g, b);
        if synth_bold {
            c.set_stroke_rgb(r, g, b);
            c.set_line_width(size * 0.03);
            c.set_text_rendering_mode(pdf_writer::types::TextRenderingMode::FillStroke);
        }
        c.set_font(Name(format!("F{f}").as_bytes()), size);
        if spacing != 0.0 {
            c.set_char_spacing(spacing);
        }
        c.set_text_matrix([1.0, 0.0, if synth_italic { 0.2 } else { 0.0 }, 1.0, cx, y]);
        c.show(Str(&bytes));
        c.end_text();
        // advance for the next fallback segment (rare)
        cx += bytes.len() as f32 / 2.0 * size * 0.5;
    }
}

pub fn export(doc: &Document, fonts: &FontStore, pag: &Pagination) -> Result<Vec<u8>, String> {
    let mut pdf = Pdf::new();
    let mut next = 1;
    let mut new_ref = || {
        let r = Ref::new(next);
        next += 1;
        r
    };
    let catalog_id = new_ref();
    let pages_id = new_ref();
    let info_id = new_ref();
    let page_ids: Vec<Ref> = pag.pages.iter().map(|_| new_ref()).collect();
    let content_ids: Vec<Ref> = pag.pages.iter().map(|_| new_ref()).collect();

    let mut ex = Exporter { fonts, uses: BTreeMap::new(), images: HashMap::new(), image_list: vec![] };
    let mut all_links: Vec<Vec<Link>> = vec![];
    let mut contents: Vec<Vec<u8>> = vec![];
    for page in &pag.pages {
        let h = page.setup.h;
        let mut c = Content::new();
        for f in &page.fills {
            let (r, g, b) = col(f.color);
            c.save_state().set_fill_rgb(r, g, b).rect(f.x, h - f.y - f.h, f.w, f.h).fill_nonzero().restore_state();
        }
        let mut links = vec![];
        draw_lines(&mut ex, &mut c, &page.body, h, &mut links);
        for r in &page.rules {
            let (cr, cg, cb) = col(r.color);
            c.save_state().set_stroke_rgb(cr, cg, cb).set_line_width(r.w.max(0.25)).move_to(r.x0, h - r.y0).line_to(r.x1, h - r.y1).stroke().restore_state();
        }
        draw_lines(&mut ex, &mut c, &page.header, h, &mut links);
        draw_lines(&mut ex, &mut c, &page.footer, h, &mut links);
        contents.push(c.finish().to_vec());
        all_links.push(links);
    }

    // fonts
    let mut font_refs: Vec<(usize, Ref)> = vec![];
    for (face_idx, usage) in &ex.uses {
        let face = &fonts.faces[*face_idx];
        let type0 = new_ref();
        let cid = new_ref();
        let desc = new_ref();
        let file = new_ref();
        let cmap_ref = new_ref();
        let base_name = format!("AAAAAA+{}", face.family.replace(' ', ""));
        let subset = subsetter::subset(&face.data, face.index, &usage.remap).map_err(|e| format!("font subsetting failed: {e:?}"))?;
        let parsed = ttf_parser::Face::parse(&face.data, face.index).map_err(|e| e.to_string())?;
        let bbox = parsed.global_bounding_box();
        let upem = face.upem;
        let sc = 1000.0 / upem;
        pdf.type0_font(type0).base_font(Name(base_name.as_bytes())).encoding_predefined(Name(b"Identity-H")).descendant_font(cid).to_unicode(cmap_ref);
        {
            let mut cf = pdf.cid_font(cid);
            cf.subtype(if face.glyf { pdf_writer::types::CidFontType::Type2 } else { pdf_writer::types::CidFontType::Type0 });
            cf.base_font(Name(base_name.as_bytes()));
            cf.system_info(SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"Identity"), supplement: 0 });
            cf.font_descriptor(desc);
            cf.default_width(0.0);
            if face.glyf {
                cf.cid_to_gid_map_predefined(Name(b"Identity"));
            }
            let n = usage.remap.num_gids() as usize;
            let mut widths = vec![0f32; n];
            for old in usage.remap.remapped_gids() {
                if let Some(new) = usage.remap.get(old) {
                    widths[new as usize] = face.adv.get(old as usize).copied().unwrap_or(0) as f32 * sc;
                }
            }
            cf.widths().consecutive(0, widths);
        }
        {
            let mut fd = pdf.font_descriptor(desc);
            fd.name(Name(base_name.as_bytes()));
            fd.flags(pdf_writer::types::FontFlags::NON_SYMBOLIC);
            fd.bbox(Rect::new(bbox.x_min as f32 * sc, bbox.y_min as f32 * sc, bbox.x_max as f32 * sc, bbox.y_max as f32 * sc));
            fd.italic_angle(parsed.italic_angle());
            fd.ascent(face.asc * 1000.0);
            fd.descent(-face.desc * 1000.0);
            fd.cap_height(parsed.capital_height().map(|v| v as f32 * sc).unwrap_or(face.asc * 700.0));
            fd.stem_v(if face.bold { 140.0 } else { 80.0 });
            if face.glyf {
                fd.font_file2(file);
            } else {
                fd.font_file3(file);
            }
        }
        let compressed = deflate(&subset);
        let mut st = pdf.stream(file, &compressed);
        st.filter(Filter::FlateDecode);
        if face.glyf {
            st.pair(Name(b"Length1"), subset.len() as i32);
        } else {
            st.pair(Name(b"Subtype"), Name(b"OpenType"));
        }
        st.finish();
        let mut cmap = UnicodeCmap::new(Name(b"Custom"), SystemInfo { registry: Str(b"Adobe"), ordering: Str(b"UCS"), supplement: 0 });
        for (gid, ch) in &usage.uni {
            cmap.pair(*gid, *ch);
        }
        pdf.cmap(cmap_ref, &cmap.finish());
        font_refs.push((*face_idx, type0));
    }

    // images
    let mut image_refs: Vec<Ref> = vec![];
    for img in ex.image_list.clone() {
        let id = new_ref();
        image_refs.push(id);
        write_image(&mut pdf, id, &img, &mut new_ref)?;
    }

    // pages
    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id).kids(page_ids.iter().copied()).count(page_ids.len() as i32);
    for (i, page) in pag.pages.iter().enumerate() {
        let mut annots: Vec<Ref> = vec![];
        for l in &all_links[i] {
            let aid = new_ref();
            annots.push(aid);
            let mut a = pdf.annotation(aid);
            a.subtype(AnnotationType::Link).rect(Rect::new(l.rect[0], l.rect[1], l.rect[2], l.rect[3])).border(0.0, 0.0, 0.0, None);
            a.action().action_type(ActionType::Uri).uri(Str(l.url.as_bytes()));
            a.finish();
        }
        let mut p = pdf.page(page_ids[i]);
        p.media_box(Rect::new(0.0, 0.0, page.setup.w, page.setup.h)).parent(pages_id).contents(content_ids[i]);
        if !annots.is_empty() {
            p.annotations(annots.iter().copied());
        }
        {
            let mut res = p.resources();
            let mut fd = res.fonts();
            for (fi, r) in &font_refs {
                fd.pair(Name(format!("F{fi}").as_bytes()), *r);
            }
            fd.finish();
            if !image_refs.is_empty() {
                let mut xo = res.x_objects();
                for (k, r) in image_refs.iter().enumerate() {
                    xo.pair(Name(format!("Im{k}").as_bytes()), *r);
                }
                xo.finish();
            }
        }
        p.finish();
        pdf.stream(content_ids[i], &deflate(&contents[i])).filter(Filter::FlateDecode);
    }
    let mut di = pdf.document_info(info_id);
    di.producer(TextStr("Word RS"));
    if !doc.meta.title.is_empty() {
        di.title(TextStr(&doc.meta.title));
    }
    if !doc.meta.author.is_empty() {
        di.author(TextStr(&doc.meta.author));
    }
    di.finish();
    Ok(pdf.finish())
}

fn write_image(pdf: &mut Pdf, id: Ref, img: &ImageData, new_ref: &mut impl FnMut() -> Ref) -> Result<(), String> {
    use image::ImageDecoder;
    // JPEG: embed the original bytes (DCT) when it is plain RGB or grayscale
    if img.mime == "image/jpeg" {
        if let Ok(dec) = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(&img.bytes[..])) {
            let ct = dec.color_type();
            if matches!(ct, image::ColorType::Rgb8 | image::ColorType::L8) {
                let mut x = pdf.image_xobject(id, &img.bytes);
                x.filter(Filter::DctDecode);
                x.width(img.px_w as i32);
                x.height(img.px_h as i32);
                x.bits_per_component(8);
                if ct == image::ColorType::Rgb8 {
                    x.color_space().device_rgb();
                } else {
                    x.color_space().device_gray();
                }
                x.finish();
                return Ok(());
            }
        }
    }
    let rgba = img.decode().ok_or("could not decode image")?;
    let (w, h) = rgba.dimensions();
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    let mut alpha = Vec::with_capacity((w * h) as usize);
    let mut has_alpha = false;
    for p in rgba.pixels() {
        rgb.extend_from_slice(&p.0[..3]);
        alpha.push(p.0[3]);
        has_alpha |= p.0[3] != 255;
    }
    let smask = if has_alpha { Some(new_ref()) } else { None };
    let data = deflate(&rgb);
    let mut x = pdf.image_xobject(id, &data);
    x.filter(Filter::FlateDecode);
    x.width(w as i32);
    x.height(h as i32);
    x.bits_per_component(8);
    x.color_space().device_rgb();
    if let Some(s) = smask {
        x.s_mask(s);
    }
    x.finish();
    if let Some(s) = smask {
        let ad = deflate(&alpha);
        let mut m = pdf.image_xobject(s, &ad);
        m.filter(Filter::FlateDecode);
        m.width(w as i32);
        m.height(h as i32);
        m.bits_per_component(8);
        m.color_space().device_gray();
        m.finish();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_valid_pdf_with_text_and_fonts() {
        let doc = crate::docx::read(&crate::docx::write(&{
            let mut d = Document::new();
            d.body = crate::story::Story::from_blocks(vec![Block::Para(Paragraph::styled("Heading 1", "Hello PDF")), Block::Para(Paragraph::with_text("The quick brown fox — café ñ ü € “quotes”."))]);
            d
        }).unwrap()).unwrap();
        let mut fonts = FontStore::new();
        let sc = StyleCache::build(&doc.styles, doc.styles_ver);
        let mut eng = LayoutEngine::default();
        let pag = eng.paginate(&mut fonts, &doc, &sc);
        let bytes = export(&doc, &fonts, &pag).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        std::fs::write("/tmp/wordrs-test.pdf", &bytes).ok();
    }
}

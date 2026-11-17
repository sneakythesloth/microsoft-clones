//! Painting of laid-out pages with egui. All geometry arrives in points and is scaled by
//! `scale` (screen points per document point = zoom * 96/72).

use std::collections::HashMap;

use eframe::egui::{self, Color32, Context, FontId, Painter, Pos2, Rect, Stroke, TextureHandle, TextureId};

use crate::fonts::FontStore;
use crate::layout::*;
use crate::model::*;

pub fn rgb(c: Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn dim(c: Color32, on: bool) -> Color32 {
    if !on {
        return c;
    }
    let m = |v: u8| (v as f32 + (255.0 - v as f32) * 0.62) as u8;
    Color32::from_rgba_unmultiplied(m(c.r()), m(c.g()), m(c.b()), c.a())
}

#[derive(Default)]
pub struct TexCache {
    map: HashMap<u64, TextureHandle>,
}

impl TexCache {
    pub fn get(&mut self, ctx: &Context, img: &ImageData) -> Option<TextureId> {
        if let Some(t) = self.map.get(&img.id) {
            return Some(t.id());
        }
        let mut rgba = img.decode()?;
        let (w, h) = rgba.dimensions();
        if w.max(h) > 2048 {
            let k = 2048.0 / w.max(h) as f32;
            rgba = image::imageops::resize(&rgba, ((w as f32 * k) as u32).max(1), ((h as f32 * k) as u32).max(1), image::imageops::FilterType::Triangle);
        }
        let (w, h) = rgba.dimensions();
        let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], rgba.as_raw());
        let t = ctx.load_texture(format!("img{}", img.id), ci, egui::TextureOptions::LINEAR);
        let id = t.id();
        self.map.insert(img.id, t);
        Some(id)
    }
}

pub struct View<'a> {
    pub painter: &'a Painter,
    pub ctx: &'a Context,
    pub fonts: &'a FontStore,
    pub tex: &'a mut TexCache,
    /// Screen position of the page's top-left corner.
    pub origin: Pos2,
    pub scale: f32,
    pub show_marks: bool,
}

impl View<'_> {
    pub fn pt(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.origin.x + x * self.scale, self.origin.y + y * self.scale)
    }
}

fn galley_baseline(g: &egui::Galley) -> f32 {
    g.rows.first().and_then(|r| r.row.glyphs.first().map(|gl| r.pos.y + gl.pos.y)).unwrap_or(g.size().y * 0.8)
}

fn draw_text(v: &View, text: &str, x: f32, base: f32, size_px: f32, info: &FmtInfo, color: Color32) {
    if text.is_empty() {
        return;
    }
    let family = v.fonts.egui_family(info.res.face);
    let fmt = egui::TextFormat {
        font_id: FontId::new(size_px.max(1.0), family),
        color,
        italics: info.res.synth_italic,
        extra_letter_spacing: info.r.spacing * v.scale,
        ..Default::default()
    };
    let job = egui::text::LayoutJob::single_section(text.to_owned(), fmt);
    let galley = v.ctx.fonts_mut(|f| f.layout_job(job));
    let by = galley_baseline(&galley);
    let pos = Pos2::new(x, base - by);
    v.painter.galley(pos, galley.clone(), color);
    if info.res.synth_bold {
        v.painter.galley(Pos2::new(pos.x + (size_px * 0.03).max(0.6), pos.y), galley, color);
    }
}

pub fn paint_page_frame(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect.translate(egui::vec2(3.0, 4.0)).expand(1.0), 0.0, Color32::from_black_alpha(55));
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_gray(175)), egui::StrokeKind::Outside);
}

pub fn paint_fills(v: &View, page: &Page) {
    for f in &page.fills {
        let r = Rect::from_min_size(v.pt(f.x, f.y), egui::vec2(f.w * v.scale, f.h * v.scale));
        v.painter.rect_filled(r, 0.0, rgb(f.color));
    }
}

pub fn paint_rules(v: &View, page: &Page) {
    for r in &page.rules {
        let w = (r.w * v.scale).max(1.0);
        v.painter.line_segment([v.pt(r.x0, r.y0), v.pt(r.x1, r.y1)], Stroke::new(w, rgb(r.color)));
    }
}

/// Highlight the part of `a..b` that falls on each placed line.
pub fn paint_range(v: &View, lines: &[PlacedLine], a: Pos, b: Pos, color: Color32) {
    if a >= b {
        return;
    }
    let clip = v.painter.clip_rect();
    for pl in lines {
        if pl.para < a.p || pl.para > b.p {
            continue;
        }
        let ln = &pl.layout.lines[pl.line];
        let top = v.origin.y + pl.y * v.scale;
        let bot = top + ln.h * v.scale;
        if bot < clip.top() || top > clip.bottom() {
            continue;
        }
        let s = if pl.para == a.p { a.o.max(ln.start) } else { ln.start };
        let e = if pl.para == b.p { b.o.min(ln.end) } else { ln.end };
        let continues = pl.para < b.p && pl.line + 1 == pl.layout.lines.len();
        if s > e || (s == e && !continues) {
            continue;
        }
        let x1 = pl.x + ln.x_at(s);
        let mut x2 = pl.x + ln.x_at(e);
        if continues {
            x2 += 5.0;
        }
        if x2 - x1 < 0.01 {
            continue;
        }
        let r = Rect::from_min_max(Pos2::new(v.origin.x + x1 * v.scale, top), Pos2::new(v.origin.x + x2 * v.scale, bot));
        v.painter.rect_filled(r, 0.0, color);
    }
}

pub fn paint_cells(v: &View, page: &Page, b: usize, r0: usize, c0: usize, r1: usize, c1: usize, color: Color32) {
    for c in &page.cells {
        if c.b == b && c.r >= r0 && c.r <= r1 && c.c >= c0 && c.c <= c1 {
            let r = Rect::from_min_size(v.pt(c.x, c.y), egui::vec2(c.w * v.scale, c.h * v.scale));
            v.painter.rect_filled(r, 0.0, color);
        }
    }
}

pub fn paint_squiggles(v: &View, lines: &[PlacedLine], spans: &[(usize, usize, usize)]) {
    let clip = v.painter.clip_rect();
    for pl in lines {
        let ln = &pl.layout.lines[pl.line];
        let base = v.origin.y + (pl.y + ln.base) * v.scale;
        if base < clip.top() - 20.0 || base > clip.bottom() + 20.0 {
            continue;
        }
        for &(p, s, e) in spans {
            if p != pl.para {
                continue;
            }
            let (s, e) = (s.max(ln.start), e.min(ln.caret_end));
            if s >= e {
                continue;
            }
            let x1 = v.origin.x + (pl.x + ln.x_at(s)) * v.scale;
            let x2 = v.origin.x + (pl.x + ln.x_at(e)) * v.scale;
            let y = base + 1.5 * v.scale.max(1.0);
            let step = 2.0 * v.scale.max(1.0);
            let mut pts = vec![];
            let mut x = x1;
            let mut up = false;
            while x < x2 {
                pts.push(Pos2::new(x, if up { y - step * 0.5 } else { y + step * 0.5 }));
                up = !up;
                x += step;
            }
            pts.push(Pos2::new(x2, y));
            v.painter.add(egui::Shape::line(pts, Stroke::new(1.0, Color32::from_rgb(220, 40, 40))));
        }
    }
}

pub fn paint_lines(v: &mut View, lines: &[PlacedLine], dimmed: bool, page_w_pt: f32, left_pt: f32) {
    let s = v.scale;
    let clip = v.painter.clip_rect();
    for pl in lines {
        let lay = &pl.layout;
        let ln = &lay.lines[pl.line];
        let top = v.origin.y + pl.y * s;
        let bot = top + ln.h * s;
        if bot < clip.top() - 4.0 || top > clip.bottom() + 4.0 {
            continue;
        }
        let base_y = v.origin.y + (pl.y + ln.base) * s;
        // list marker
        if let Some(m) = &pl.marker {
            let info = FmtInfo { r: lay.mark.clone(), res: lay.mark_res, link: None };
            let mut r = lay.mark.clone();
            r.underline = false;
            let info = FmtInfo { r, ..info };
            let size = lay.mark.size * s;
            let x = v.origin.x + (pl.x + lay.marker_x) * s;
            draw_text(v, m, x, base_y, size, &info, dim(rgb(lay.mark.color), dimmed));
        }
        for it in &ln.items {
            let info = &lay.fmts[it.fi as usize];
            let face = &v.fonts.faces[info.res.face];
            let x = v.origin.x + (pl.x + ln.x0 + it.x) * s;
            let w = it.w * s;
            let size_px = it.size * s;
            let base = base_y - it.rise * s;
            let color = dim(rgb(info.r.color), dimmed);
            if let Some(h) = info.r.highlight {
                if !matches!(it.kind, ItemKind::Break) {
                    let r = Rect::from_min_max(Pos2::new(x, base - face.asc * size_px), Pos2::new(x + w, base + (face.desc + face.gap * 0.0) * size_px));
                    v.painter.rect_filled(r, 0.0, dim(rgb(h), dimmed));
                }
            }
            match &it.kind {
                ItemKind::Text => draw_text(v, &it.text, x, base, size_px, info, color),
                ItemKind::Field(t) => draw_text(v, t, x, base, size_px, info, color),
                ItemKind::Image(img) => {
                    let r = Rect::from_min_size(Pos2::new(x, base_y - img.h * s), egui::vec2(img.w * s, img.h * s));
                    if let Some(tid) = v.tex.get(v.ctx, &img.data) {
                        let tint = if dimmed { Color32::from_white_alpha(120) } else { Color32::WHITE };
                        v.painter.image(tid, r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), tint);
                    } else {
                        v.painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::GRAY), egui::StrokeKind::Inside);
                    }
                }
                ItemKind::Tab(leader) => {
                    match leader {
                        Leader::Dots => {
                            let step = 4.0 * s;
                            let mut xx = (x / step).ceil() * step;
                            while xx < x + w - 1.0 {
                                v.painter.circle_filled(Pos2::new(xx, base - 0.8 * s), (0.45 * s).max(0.5), color);
                                xx += step;
                            }
                        }
                        Leader::Dashes => {
                            let mut xx = x + 1.0;
                            while xx + 3.0 * s < x + w {
                                v.painter.line_segment([Pos2::new(xx, base - 0.3 * size_px), Pos2::new(xx + 3.0 * s, base - 0.3 * size_px)], Stroke::new(1.0, color));
                                xx += 5.0 * s;
                            }
                        }
                        Leader::Line => {
                            v.painter.line_segment([Pos2::new(x, base + 0.5 * s), Pos2::new(x + w, base + 0.5 * s)], Stroke::new(1.0, color));
                        }
                        Leader::None => {}
                    }
                    if v.show_marks && w > 6.0 {
                        let y = base - 0.3 * size_px;
                        let c = Color32::from_rgb(90, 130, 190);
                        v.painter.line_segment([Pos2::new(x + 2.0, y), Pos2::new(x + w - 2.0, y)], Stroke::new(1.0, c));
                        v.painter.line_segment([Pos2::new(x + w - 2.0, y), Pos2::new(x + w - 5.0, y - 2.5)], Stroke::new(1.0, c));
                        v.painter.line_segment([Pos2::new(x + w - 2.0, y), Pos2::new(x + w - 5.0, y + 2.5)], Stroke::new(1.0, c));
                    }
                }
                ItemKind::Space => {
                    if v.show_marks {
                        let c = Color32::from_rgb(90, 130, 190);
                        let n = it.end - it.start;
                        for i in 0..n {
                            let mid = x + (it.cum[i] + it.cum[i + 1]) * 0.5 * s;
                            v.painter.circle_filled(Pos2::new(mid, base - 0.3 * size_px), (0.07 * size_px).max(0.6), c);
                        }
                    }
                }
                ItemKind::Break => {
                    if v.show_marks {
                        let c = Color32::from_rgb(90, 130, 190);
                        if it.text == "\u{c}" {
                            // page break: dashed rule with a label
                            let x0 = v.origin.x + left_pt * s;
                            let x1 = v.origin.x + (left_pt + (page_w_pt - 2.0 * left_pt)) * s;
                            let y = base - 0.3 * size_px;
                            let label = "Page Break";
                            let font = FontId::proportional((8.0 * s).max(7.0));
                            let galley = v.ctx.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font, c));
                            let gw = galley.size().x;
                            let mid = (x0 + x1) * 0.5;
                            let mut xx = x0;
                            while xx < mid - gw * 0.5 - 6.0 {
                                v.painter.line_segment([Pos2::new(xx, y), Pos2::new(xx + 3.0, y)], Stroke::new(1.0, c));
                                xx += 6.0;
                            }
                            let mut xx = mid + gw * 0.5 + 6.0;
                            while xx < x1 {
                                v.painter.line_segment([Pos2::new(xx, y), Pos2::new((xx + 3.0).min(x1), y)], Stroke::new(1.0, c));
                                xx += 6.0;
                            }
                            v.painter.galley(Pos2::new(mid - gw * 0.5, y - galley.size().y * 0.5), galley, c);
                        } else {
                            // soft line break arrow
                            let y = base - 0.35 * size_px;
                            let a = Pos2::new(x + 1.0, y);
                            v.painter.line_segment([a, Pos2::new(a.x + 5.0, y)], Stroke::new(1.0, c));
                            v.painter.line_segment([Pos2::new(a.x + 5.0, y), Pos2::new(a.x + 5.0, y - 5.0)], Stroke::new(1.0, c));
                            v.painter.line_segment([a, Pos2::new(a.x + 3.0, y - 2.5)], Stroke::new(1.0, c));
                            v.painter.line_segment([a, Pos2::new(a.x + 3.0, y + 2.5)], Stroke::new(1.0, c));
                        }
                    }
                }
            }
            // decorations
            if !matches!(it.kind, ItemKind::Break | ItemKind::Image(_)) {
                let trailing = matches!(it.kind, ItemKind::Space) && it.start >= ln.caret_end && !ln.last;
                if info.r.underline && !trailing {
                    let y = base + (face.ul_pos * size_px).max(1.0);
                    let th = (face.ul_thick * size_px).max(1.0);
                    v.painter.line_segment([Pos2::new(x, y), Pos2::new(x + w, y)], Stroke::new(th, color));
                }
                if info.r.strike && !trailing {
                    let y = base - 0.3 * size_px;
                    v.painter.line_segment([Pos2::new(x, y), Pos2::new(x + w, y)], Stroke::new((face.ul_thick * size_px).max(1.0), color));
                }
            }
        }
        // paragraph mark
        if v.show_marks && ln.last {
            let info = FmtInfo { r: lay.mark.clone(), res: lay.mark_res, link: None };
            let end_x = pl.x + ln.x_at(ln.end);
            let x = v.origin.x + end_x * s + 1.0;
            draw_text(v, "¶", x, base_y, lay.mark.size * s, &info, Color32::from_rgb(90, 130, 190));
        }
    }
}

pub fn paint_caret(v: &View, x: f32, y: f32, h: f32, color: Color32) {
    let p = v.pt(x, y);
    let w = (1.0 * v.scale.max(1.0)).max(1.0);
    let r = Rect::from_min_size(Pos2::new(p.x - w * 0.5, p.y), egui::vec2(w, (h * v.scale).max(8.0)));
    v.painter.rect_filled(r, 0.0, color);
}

/// Rectangle (page points) of the image item at `(para, off)`, if any.
pub fn image_rect(lines: &[PlacedLine], para: usize, off: usize) -> Option<(Rect, u64)> {
    for pl in lines.iter().filter(|pl| pl.para == para) {
        let ln = &pl.layout.lines[pl.line];
        for it in &ln.items {
            if let ItemKind::Image(img) = &it.kind {
                if it.start == off {
                    let x = pl.x + ln.x0 + it.x;
                    let y = pl.y + ln.base - img.h;
                    return Some((Rect::from_min_size(Pos2::new(x, y), egui::vec2(img.w, img.h)), img.data.id));
                }
            }
        }
    }
    None
}

/// Find an image under a page-space point: returns (para, char offset).
pub fn image_at(lines: &[PlacedLine], x: f32, y: f32) -> Option<(usize, usize)> {
    for pl in lines {
        let ln = &pl.layout.lines[pl.line];
        if y < pl.y || y > pl.y + ln.h {
            continue;
        }
        for it in &ln.items {
            if let ItemKind::Image(img) = &it.kind {
                let r = Rect::from_min_size(Pos2::new(pl.x + ln.x0 + it.x, pl.y + ln.base - img.h), egui::vec2(img.w, img.h));
                if r.contains(Pos2::new(x, y)) {
                    return Some((pl.para, it.start));
                }
            }
        }
    }
    None
}

/// Link under a page-space point.
pub fn link_at(lines: &[PlacedLine], x: f32, y: f32) -> Option<String> {
    for pl in lines {
        let ln = &pl.layout.lines[pl.line];
        if y < pl.y || y > pl.y + ln.h {
            continue;
        }
        for it in &ln.items {
            let rx = pl.x + ln.x0 + it.x;
            if x >= rx && x <= rx + it.w {
                if let Some(l) = &pl.layout.fmts[it.fi as usize].link {
                    return Some(l.clone());
                }
            }
        }
    }
    None
}

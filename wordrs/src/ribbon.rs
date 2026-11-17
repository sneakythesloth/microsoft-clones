use eframe::egui::{self, vec2, Align2, Color32, Context, FontId, Painter, Pos2, Popup, PopupCloseBehavior, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

use crate::app::*;
use crate::editor::*;
use crate::model::*;
use crate::render::rgb;

const SEL_BG: Color32 = Color32::from_rgb(198, 218, 245);
const ACCENT: Color32 = Color32::from_rgb(43, 87, 154);

// ---------------------------------------------------------------- widgets

fn group(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.set_min_height(70.0);
        add(ui);
        ui.add_space(1.0);
        ui.label(RichText::new(title).small().color(Color32::from_gray(110)));
    });
    ui.separator();
}

fn icon_btn(ui: &mut Ui, selected: bool, tip: &str, size: Vec2, draw: impl FnOnce(&Painter, Rect, Color32)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if selected || resp.hovered() {
        ui.painter().rect_filled(rect, 3.0, if selected { SEL_BG } else { Color32::from_gray(224) });
    }
    let col = if resp.hovered() || selected { Color32::from_gray(20) } else { Color32::from_gray(60) };
    draw(ui.painter(), rect, col);
    resp.on_hover_text(tip)
}

fn arrow_btn(ui: &mut Ui, tip: &str) -> Response {
    icon_btn(ui, false, tip, vec2(13.0, 24.0), |p, r, c| {
        let ctr = r.center() + vec2(0.0, 1.0);
        p.add(egui::Shape::convex_polygon(vec![ctr + vec2(-3.5, -2.0), ctr + vec2(3.5, -2.0), ctr + vec2(0.0, 2.5)], c, Stroke::NONE));
    })
}

fn lines_icon(p: &Painter, r: Rect, c: Color32, widths: [f32; 4], align: i32) {
    let inner = r.shrink2(vec2(5.0, 5.5));
    for (i, w) in widths.iter().enumerate() {
        let y = inner.top() + i as f32 * (inner.height() / 3.0);
        let len = inner.width() * w;
        let x0 = match align {
            0 => inner.left(),
            1 => inner.center().x - len / 2.0,
            _ => inner.right() - len,
        };
        p.line_segment([Pos2::new(x0, y), Pos2::new(x0 + len, y)], Stroke::new(1.5_f32, c));
    }
}

fn align_icon(p: &Painter, r: Rect, c: Color32, kind: usize) {
    match kind {
        0 => lines_icon(p, r, c, [1.0, 0.6, 1.0, 0.6], 0),
        1 => lines_icon(p, r, c, [1.0, 0.6, 1.0, 0.6], 1),
        2 => lines_icon(p, r, c, [1.0, 0.6, 1.0, 0.6], 2),
        _ => lines_icon(p, r, c, [1.0, 1.0, 1.0, 1.0], 0),
    }
}

fn list_icon(p: &Painter, r: Rect, c: Color32, numbered: bool) {
    let inner = r.shrink2(vec2(4.0, 5.5));
    for i in 0..3 {
        let y = inner.top() + i as f32 * (inner.height() / 2.0);
        if numbered {
            p.text(Pos2::new(inner.left() + 2.0, y), Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(8.5), c);
        } else {
            p.circle_filled(Pos2::new(inner.left() + 2.0, y), 1.6, c);
        }
        p.line_segment([Pos2::new(inner.left() + 7.0, y), Pos2::new(inner.right(), y)], Stroke::new(1.5_f32, c));
    }
}

fn indent_icon(p: &Painter, r: Rect, c: Color32, inc: bool) {
    let inner = r.shrink2(vec2(4.5, 5.5));
    for i in 0..4 {
        let y = inner.top() + i as f32 * (inner.height() / 3.0);
        let x0 = if (1..3).contains(&i) { inner.left() + 6.0 } else { inner.left() };
        p.line_segment([Pos2::new(x0, y), Pos2::new(inner.right(), y)], Stroke::new(1.4_f32, c));
    }
    let cy = inner.center().y;
    let x = inner.left() + 1.0;
    let pts = if inc { vec![Pos2::new(x, cy - 3.0), Pos2::new(x + 4.0, cy), Pos2::new(x, cy + 3.0)] } else { vec![Pos2::new(x + 4.0, cy - 3.0), Pos2::new(x, cy), Pos2::new(x + 4.0, cy + 3.0)] };
    p.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
}

fn spacing_icon(p: &Painter, r: Rect, c: Color32) {
    let inner = r.shrink2(vec2(5.0, 4.0));
    let x = inner.left() + 2.5;
    p.line_segment([Pos2::new(x, inner.top()), Pos2::new(x, inner.bottom())], Stroke::new(1.3_f32, c));
    p.add(egui::Shape::convex_polygon(vec![Pos2::new(x - 2.5, inner.top() + 3.5), Pos2::new(x + 2.5, inner.top() + 3.5), Pos2::new(x, inner.top())], c, Stroke::NONE));
    p.add(egui::Shape::convex_polygon(vec![Pos2::new(x - 2.5, inner.bottom() - 3.5), Pos2::new(x + 2.5, inner.bottom() - 3.5), Pos2::new(x, inner.bottom())], c, Stroke::NONE));
    for i in 0..3 {
        let y = inner.top() + 1.5 + i as f32 * (inner.height() - 3.0) / 2.0;
        p.line_segment([Pos2::new(x + 5.0, y), Pos2::new(inner.right(), y)], Stroke::new(1.4_f32, c));
    }
}

fn undo_icon(p: &Painter, r: Rect, c: Color32, redo: bool) {
    let ctr = r.center();
    let s = if redo { -1.0 } else { 1.0 };
    let center = Pos2::new(ctr.x, ctr.y + 2.0);
    let rad = 5.5;
    let (a0, a1) = (0.1f32, std::f32::consts::PI + 0.1);
    let mut pts = vec![];
    for i in 0..=14 {
        let t = a0 + (a1 - a0) * i as f32 / 14.0;
        pts.push(Pos2::new(center.x + s * rad * t.cos(), center.y - rad * t.sin()));
    }
    let end = *pts.last().unwrap();
    p.add(egui::Shape::line(pts, Stroke::new(1.8_f32, c)));
    p.add(egui::Shape::convex_polygon(vec![Pos2::new(end.x - 3.2, end.y - 0.5), Pos2::new(end.x + 3.2, end.y - 0.5), Pos2::new(end.x, end.y + 4.2)], c, Stroke::NONE));
}

fn save_icon(p: &Painter, r: Rect, c: Color32) {
    let b = r.shrink(6.0);
    p.rect_stroke(b, 1.0, Stroke::new(1.4_f32, c), egui::StrokeKind::Inside);
    p.rect_filled(Rect::from_min_size(b.min + vec2(2.5, 0.0), vec2(b.width() - 5.0, 3.5)), 0.0, c);
    p.rect_stroke(Rect::from_min_max(b.center() + vec2(-3.0, 0.5), b.max - vec2(2.5, 1.5)), 0.0, Stroke::new(1.2_f32, c), egui::StrokeKind::Inside);
}

fn letter_a(p: &Painter, r: Rect, c: Color32, bar: Color32, mark: bool) {
    let ctr = r.center() - vec2(0.0, 2.0);
    if mark {
        p.text(ctr, Align2::CENTER_CENTER, "ab", FontId::proportional(12.0), Color32::from_gray(60));
    } else {
        p.text(ctr, Align2::CENTER_CENTER, "A", FontId::proportional(15.0), c);
    }
    p.rect_filled(Rect::from_min_size(Pos2::new(r.left() + 4.0, r.bottom() - 6.5), vec2(r.width() - 8.0, 3.5)), 0.0, bar);
}

fn same<T: PartialEq + Clone>(chars: &[RChar], f: impl Fn(&RChar) -> T) -> Option<T> {
    let first = f(chars.first()?);
    if chars.iter().all(|r| f(r) == first) {
        Some(first)
    } else {
        None
    }
}

fn shade(c: Rgb, f: f32) -> Rgb {
    let m = |v: u8| -> u8 {
        if f >= 0.0 {
            (v as f32 + (255.0 - v as f32) * f) as u8
        } else {
            (v as f32 * (1.0 + f)) as u8
        }
    };
    [m(c[0]), m(c[1]), m(c[2])]
}

const THEME: [Rgb; 10] = [[255, 255, 255], [0, 0, 0], [231, 230, 230], [68, 84, 106], [68, 114, 196], [237, 125, 49], [165, 165, 165], [255, 192, 0], [91, 155, 213], [112, 173, 71]];
const STANDARD: [Rgb; 10] = [[192, 0, 0], [255, 0, 0], [255, 192, 0], [255, 255, 0], [146, 208, 80], [0, 176, 80], [0, 176, 240], [0, 112, 192], [0, 32, 96], [112, 48, 160]];
const HIGHLIGHTS: [Rgb; 15] = [[255, 255, 0], [0, 255, 0], [0, 255, 255], [255, 0, 255], [0, 0, 255], [255, 0, 0], [0, 0, 128], [0, 128, 128], [0, 128, 0], [128, 0, 128], [128, 0, 0], [128, 128, 0], [128, 128, 128], [192, 192, 192], [0, 0, 0]];

fn swatch(ui: &mut Ui, c: Rgb, tip: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(vec2(17.0, 17.0), Sense::click());
    ui.painter().rect_filled(r, 1.0, rgb(c));
    ui.painter().rect_stroke(r, 1.0, Stroke::new(if resp.hovered() { 2.0_f32 } else { 1.0_f32 }, if resp.hovered() { ACCENT } else { Color32::from_gray(170) }), egui::StrokeKind::Inside);
    resp.on_hover_text(tip).clicked()
}

/// Returns Some(Some(color)) / Some(None) (= "automatic"/"no color") when something was chosen.
fn palette(ui: &mut Ui, none_label: &str, highlight: bool) -> Option<Option<Rgb>> {
    let mut out = None;
    ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
    if ui.button(none_label).clicked() {
        out = Some(None);
        ui.close();
    }
    if highlight {
        ui.label(RichText::new("Highlight colors").small());
        for row in HIGHLIGHTS.chunks(8) {
            ui.horizontal(|ui| {
                for c in row {
                    if swatch(ui, *c, &format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])) {
                        out = Some(Some(*c));
                        ui.close();
                    }
                }
            });
        }
        return out;
    }
    ui.label(RichText::new("Theme colors").small());
    ui.horizontal(|ui| {
        for c in THEME {
            if swatch(ui, c, "Theme color") {
                out = Some(Some(c));
                ui.close();
            }
        }
    });
    for f in [0.8, 0.6, 0.4, -0.25, -0.5] {
        ui.horizontal(|ui| {
            for c in THEME {
                let s = shade(c, f);
                if swatch(ui, s, "Shade") {
                    out = Some(Some(s));
                    ui.close();
                }
            }
        });
    }
    ui.label(RichText::new("Standard colors").small());
    ui.horizontal(|ui| {
        for c in STANDARD {
            if swatch(ui, c, "Standard color") {
                out = Some(Some(c));
                ui.close();
            }
        }
    });
    ui.separator();
    let mut c = [0u8, 112, 192];
    ui.horizontal(|ui| {
        ui.label("More colors:");
        if ui.color_edit_button_srgb(&mut c).changed() {
            out = Some(Some(c));
        }
    });
    out
}

fn menu_item(ui: &mut Ui, text: &str) -> bool {
    ui.add(egui::Button::new(text).frame(false).min_size(vec2(160.0, 0.0))).clicked()
}

impl App {
    pub fn ribbon_ui(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.horizontal(|ui| {
            self.file_menu(ui, ctx);
            if icon_btn(ui, false, "Save (Ctrl+S)", vec2(24.0, 24.0), save_icon).clicked() {
                self.save();
            }
            let (can_undo, can_redo) = (!self.hist.undo.is_empty(), !self.hist.redo.is_empty());
            ui.add_enabled_ui(can_undo, |ui| {
                if icon_btn(ui, false, "Undo (Ctrl+Z)", vec2(24.0, 24.0), |p, r, c| undo_icon(p, r, c, false)).clicked() {
                    self.undo();
                }
            });
            ui.add_enabled_ui(can_redo, |ui| {
                if icon_btn(ui, false, "Redo (Ctrl+Y)", vec2(24.0, 24.0), |p, r, c| undo_icon(p, r, c, true)).clicked() {
                    self.redo();
                }
            });
            ui.separator();
            for (t, label) in [(Tab::Home, "Home"), (Tab::Insert, "Insert"), (Tab::Layout, "Layout"), (Tab::Review, "Review"), (Tab::View, "View")] {
                if ui.selectable_label(self.tab == t, RichText::new(label).size(14.5)).clicked() {
                    self.tab = t;
                }
            }
            if self.in_table() {
                if ui.selectable_label(self.tab == Tab::Table, RichText::new("Table").color(ACCENT).size(14.5)).clicked() {
                    self.tab = Tab::Table;
                }
            } else if self.tab == Tab::Table {
                self.tab = Tab::Home;
            }
            if self.selected_image().is_some() {
                if ui.selectable_label(self.tab == Tab::Picture, RichText::new("Picture").color(ACCENT).size(14.5)).clicked() {
                    self.tab = Tab::Picture;
                }
            } else if self.tab == Tab::Picture {
                self.tab = Tab::Home;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(RichText::new("Google Fonts").color(ACCENT)).on_hover_text("Browse and download fonts from Google Fonts").clicked() {
                    self.dlg.gfonts_open = true;
                }
                ui.label(RichText::new(self.title()).small().weak());
            });
        });
        ui.separator();
        egui::ScrollArea::horizontal().id_salt("ribbon_scroll").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_top(|ui| match self.tab {
                Tab::Home => self.home_tab(ui, ctx),
                Tab::Insert => self.insert_tab(ui),
                Tab::Layout => self.layout_tab(ui),
                Tab::Review => self.review_tab(ui),
                Tab::View => self.view_tab(ui),
                Tab::Table => self.table_tab(ui),
                Tab::Picture => self.picture_tab(ui),
            });
        });
    }

    fn file_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        let r = ui.add(egui::Button::new(RichText::new("File").color(Color32::WHITE).size(14.5)).fill(ACCENT));
        Popup::menu(&r).show(|ui| {
            if menu_item(ui, "New            Ctrl+N") {
                self.new_request();
            }
            if menu_item(ui, "Open…          Ctrl+O") {
                self.open_request();
            }
            let recent = self.settings.recent.clone();
            if !recent.is_empty() {
                ui.menu_button("Open Recent", |ui| {
                    for r in recent {
                        let name = std::path::Path::new(&r).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(r.clone());
                        if ui.button(name).on_hover_text(&r).clicked() {
                            let p = std::path::PathBuf::from(&r);
                            if self.dirty {
                                self.confirm = Some(Confirm::Open(Some(p)));
                            } else {
                                self.open_path(p);
                            }
                            ui.close();
                        }
                    }
                });
            }
            ui.separator();
            if menu_item(ui, "Save           Ctrl+S") {
                self.save();
            }
            if menu_item(ui, "Save As…   Ctrl+Shift+S") {
                self.save_as_dialog();
            }
            ui.separator();
            ui.menu_button("Export", |ui| {
                if ui.button("PDF…").clicked() {
                    self.export_dialog("pdf");
                    ui.close();
                }
                if ui.button("Web page (HTML)…").clicked() {
                    self.export_dialog("html");
                    ui.close();
                }
                if ui.button("Plain text…").clicked() {
                    self.export_dialog("txt");
                    ui.close();
                }
                if ui.button("Markdown…").clicked() {
                    self.export_dialog("md");
                    ui.close();
                }
            });
            if menu_item(ui, "Print…         Ctrl+P") {
                self.print();
            }
            ui.separator();
            if menu_item(ui, "Document properties…") {
                self.dlg.props_open = true;
            }
            if menu_item(ui, "Settings…") {
                self.dlg.settings_open = true;
            }
            if menu_item(ui, "Keyboard shortcuts") {
                self.dlg.help_open = true;
            }
            if menu_item(ui, "About") {
                self.dlg.about_open = true;
            }
            ui.separator();
            if menu_item(ui, "Exit") {
                if self.dirty {
                    self.confirm = Some(Confirm::Quit);
                } else {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        });
    }

    // ---------------------------------------------------------------- Home

    fn home_tab(&mut self, ui: &mut Ui, ctx: &Context) {
        let chars = self.eff_chars();
        let all = |f: fn(&RChar) -> bool| chars.iter().all(|r| f(r));
        let (bold, italic, underline, strike) = (all(|r| r.bold), all(|r| r.italic), all(|r| r.underline), all(|r| r.strike));
        let (sup, sub) = (all(|r| r.vert == VertAlign::Super), all(|r| r.vert == VertAlign::Sub));
        let font = same(&chars, |r| r.font.to_string());
        let size = same(&chars, |r| (r.size * 10.0) as i32);
        let color = chars.first().map(|r| r.color).unwrap_or([0, 0, 0]);
        let head = self.story().clamp(self.sel.head);
        let (rp, style_name, has_list) = {
            let p = self.story().para(head.p);
            let mut r = self.sc.get(&p.style).1.clone();
            r.apply(&p.fmt);
            (r, p.style.clone(), p.list)
        };

        group(ui, "Clipboard", |ui| {
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("Paste").size(14.5)).min_size(vec2(46.0, 50.0))).on_hover_text("Paste (Ctrl+V)").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                }
                ui.vertical(|ui| {
                    if ui.button("Cut").on_hover_text("Cut (Ctrl+X)").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestCut);
                    }
                    if ui.button("Copy").on_hover_text("Copy (Ctrl+C)").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestCopy);
                    }
                    if ui.selectable_label(self.format_painter.is_some(), "Format Painter").on_hover_text("Copy formatting from the selection, then select text to apply it").clicked() {
                        if self.format_painter.is_some() {
                            self.format_painter = None;
                        } else {
                            self.start_format_painter();
                        }
                    }
                });
            });
        });

        group(ui, "Font", |ui| {
            ui.horizontal(|ui| {
                self.font_combo(ui, font.as_deref().unwrap_or(""));
                let cur = size.map(|s| s as f32 / 10.0);
                egui::ComboBox::from_id_salt("fontsize").width(48.0).selected_text(cur.map(fmt_size).unwrap_or_default()).show_ui(ui, |ui| {
                    for s in SIZES {
                        if ui.selectable_label(cur == Some(s), fmt_size(s)).clicked() {
                            self.set_size(s);
                        }
                    }
                });
                if icon_btn(ui, false, "Grow font (Ctrl+])", vec2(24.0, 24.0), |p, r, c| {
                    p.text(r.center() + vec2(-2.0, 1.0), Align2::CENTER_CENTER, "A", FontId::proportional(15.0), c);
                    p.text(r.center() + vec2(6.0, -5.0), Align2::CENTER_CENTER, "+", FontId::proportional(9.0), c);
                })
                .clicked()
                {
                    self.size_step(1);
                }
                if icon_btn(ui, false, "Shrink font (Ctrl+[)", vec2(24.0, 24.0), |p, r, c| {
                    p.text(r.center() + vec2(-2.0, 1.0), Align2::CENTER_CENTER, "A", FontId::proportional(11.5), c);
                    p.text(r.center() + vec2(6.0, -4.0), Align2::CENTER_CENTER, "−", FontId::proportional(9.0), c);
                })
                .clicked()
                {
                    self.size_step(-1);
                }
                let case = icon_btn(ui, false, "Change case", vec2(28.0, 24.0), |p, r, c| {
                    p.text(r.center(), Align2::CENTER_CENTER, "Aa", FontId::proportional(13.5), c);
                });
                Popup::menu(&case).show(|ui| {
                    for (label, m) in [("UPPERCASE", CaseMode::Upper), ("lowercase", CaseMode::Lower), ("Capitalize Each Word", CaseMode::Title), ("Sentence case", CaseMode::Sentence), ("tOGGLE cASE", CaseMode::Toggle)] {
                        if menu_item(ui, label) {
                            self.change_case(m);
                        }
                    }
                    ui.separator();
                    if menu_item(ui, "Small Caps (formatting)") {
                        self.toggle_small_caps();
                    }
                    if menu_item(ui, "ALL CAPS (formatting)") {
                        self.toggle_all_caps();
                    }
                });
                if icon_btn(ui, false, "Clear all formatting", vec2(24.0, 24.0), |p, r, c| {
                    p.text(r.center() + vec2(-1.0, 0.0), Align2::CENTER_CENTER, "A", FontId::proportional(14.0), c);
                    p.line_segment([r.center() + vec2(2.0, 8.0), r.center() + vec2(10.0, -4.0)], Stroke::new(2.0_f32, Color32::from_rgb(200, 60, 60)));
                })
                .clicked()
                {
                    self.clear_all_formatting();
                }
            });
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("B").strong().size(14.0)).selected(bold).min_size(vec2(24.0, 24.0))).on_hover_text("Bold (Ctrl+B)").clicked() {
                    self.toggle_bold();
                }
                if ui.add(egui::Button::new(RichText::new("I").italics().size(14.0)).selected(italic).min_size(vec2(24.0, 24.0))).on_hover_text("Italic (Ctrl+I)").clicked() {
                    self.toggle_italic();
                }
                if ui.add(egui::Button::new(RichText::new("U").underline().size(14.0)).selected(underline).min_size(vec2(24.0, 24.0))).on_hover_text("Underline (Ctrl+U)").clicked() {
                    self.toggle_underline();
                }
                if ui.add(egui::Button::new(RichText::new("ab").strikethrough().size(13.0)).selected(strike).min_size(vec2(26.0, 24.0))).on_hover_text("Strikethrough").clicked() {
                    self.toggle_strike();
                }
                if icon_btn(ui, sub, "Subscript (Ctrl+=)", vec2(24.0, 24.0), |p, r, c| {
                    p.text(r.center() + vec2(-3.0, -1.0), Align2::CENTER_CENTER, "x", FontId::proportional(13.0), c);
                    p.text(r.center() + vec2(5.0, 4.0), Align2::CENTER_CENTER, "2", FontId::proportional(8.5), c);
                })
                .clicked()
                {
                    self.toggle_vert(VertAlign::Sub);
                }
                if icon_btn(ui, sup, "Superscript (Ctrl+Shift+=)", vec2(24.0, 24.0), |p, r, c| {
                    p.text(r.center() + vec2(-3.0, 2.0), Align2::CENTER_CENTER, "x", FontId::proportional(13.0), c);
                    p.text(r.center() + vec2(5.0, -5.0), Align2::CENTER_CENTER, "2", FontId::proportional(8.5), c);
                })
                .clicked()
                {
                    self.toggle_vert(VertAlign::Super);
                }
                ui.add_space(4.0);
                let last = self.last_color;
                if icon_btn(ui, false, "Font color", vec2(26.0, 24.0), |p, r, c| letter_a(p, r, c, rgb(last), false)).clicked() {
                    self.set_color(last);
                }
                let arrow = arrow_btn(ui, "Font color options");
                Popup::from_toggle_button_response(&arrow).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    if let Some(c) = palette(ui, "Automatic", false) {
                        match c {
                            Some(c) => self.set_color(c),
                            None => self.apply_chars(&|f| f.color = None),
                        }
                    }
                });
                let hl = self.last_highlight;
                if icon_btn(ui, false, "Text highlight color", vec2(26.0, 24.0), |p, r, c| letter_a(p, r, c, rgb(hl), true)).clicked() {
                    self.set_highlight(Some(hl));
                }
                let arrow = arrow_btn(ui, "Highlight options");
                Popup::from_toggle_button_response(&arrow).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    if let Some(c) = palette(ui, "No Color", true) {
                        self.set_highlight(c);
                    }
                });
                let _ = color;
            });
        });

        group(ui, "Paragraph", |ui| {
            ui.horizontal(|ui| {
                let bl = icon_btn(ui, has_list.map(|l| l.kind == ListKind::Bullet).unwrap_or(false), "Bullets (Ctrl+Shift+L)", vec2(26.0, 24.0), |p, r, c| list_icon(p, r, c, false));
                if bl.clicked() {
                    self.toggle_list(ListKind::Bullet);
                }
                let nl = icon_btn(ui, has_list.map(|l| l.kind != ListKind::Bullet).unwrap_or(false), "Numbering", vec2(26.0, 24.0), |p, r, c| list_icon(p, r, c, true));
                if nl.clicked() {
                    self.toggle_list(ListKind::Decimal);
                }
                let arrow = arrow_btn(ui, "Numbering library");
                Popup::menu(&arrow).show(|ui| {
                    for (label, k) in [("1.  2.  3.", ListKind::Decimal), ("a.  b.  c.", ListKind::LowerAlpha), ("A.  B.  C.", ListKind::UpperAlpha), ("i.  ii.  iii.", ListKind::LowerRoman), ("I.  II.  III.", ListKind::UpperRoman), ("•  Bullets", ListKind::Bullet)] {
                        if menu_item(ui, label) {
                            let (a, b) = self.sel.ordered();
                            self.checkpoint(None);
                            self.set_list_for(a.p, b.p, Some(k));
                        }
                    }
                    ui.separator();
                    if menu_item(ui, "Restart numbering") {
                        self.restart_numbering();
                    }
                });
                if icon_btn(ui, false, "Decrease indent", vec2(26.0, 24.0), |p, r, c| indent_icon(p, r, c, false)).clicked() {
                    self.change_indent(-1);
                }
                if icon_btn(ui, false, "Increase indent", vec2(26.0, 24.0), |p, r, c| indent_icon(p, r, c, true)).clicked() {
                    self.change_indent(1);
                }
            });
            ui.horizontal(|ui| {
                for (i, (al, tip)) in [(Align::Left, "Align left (Ctrl+L)"), (Align::Center, "Center (Ctrl+E)"), (Align::Right, "Align right (Ctrl+R)"), (Align::Justify, "Justify (Ctrl+J)")].into_iter().enumerate() {
                    if icon_btn(ui, rp.align == al, tip, vec2(26.0, 24.0), |p, r, c| align_icon(p, r, c, i)).clicked() {
                        self.set_align(al);
                    }
                }
                let sp = icon_btn(ui, false, "Line and paragraph spacing", vec2(26.0, 24.0), |p, r, c| spacing_icon(p, r, c));
                Popup::menu(&sp).show(|ui| {
                    for m in [1.0, 1.15, 1.5, 2.0, 2.5, 3.0] {
                        let cur = matches!(rp.line, LineSpacing::Multiple(x) if (x - m).abs() < 0.01);
                        if ui.selectable_label(cur, format!("{m}")).clicked() {
                            self.set_line_spacing(m);
                        }
                    }
                    ui.separator();
                    if menu_item(ui, "Add space before paragraph") {
                        self.set_para_spacing(Some(12.0), None);
                    }
                    if menu_item(ui, "Remove space before paragraph") {
                        self.set_para_spacing(Some(0.0), None);
                    }
                    if menu_item(ui, "Add space after paragraph") {
                        self.set_para_spacing(None, Some(10.0));
                    }
                    if menu_item(ui, "Remove space after paragraph") {
                        self.set_para_spacing(None, Some(0.0));
                    }
                });
                // paragraph shading
                let shade_c = rp.shade.unwrap_or([255, 255, 0]);
                let sh = icon_btn(ui, false, "Paragraph shading", vec2(26.0, 24.0), |p, r, _c| {
                    p.rect_filled(r.shrink2(vec2(6.0, 6.0)), 1.0, rgb(shade_c));
                    p.rect_stroke(r.shrink2(vec2(6.0, 6.0)), 1.0, Stroke::new(1.0_f32, Color32::from_gray(90)), egui::StrokeKind::Inside);
                });
                Popup::from_toggle_button_response(&sh).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                    if let Some(c) = palette(ui, "No Color", false) {
                        self.apply_paras(&move |p| p.fmt.shade = Some(c));
                    }
                });
                let bd = icon_btn(ui, false, "Borders", vec2(26.0, 24.0), |p, r, c| {
                    let rr = r.shrink2(vec2(6.0, 6.0));
                    p.rect_stroke(rr, 0.0, Stroke::new(1.0_f32, c), egui::StrokeKind::Inside);
                    p.line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(2.0_f32, c));
                });
                Popup::menu(&bd).show(|ui| {
                    let line = Some(Border { w: 0.75, color: [0, 0, 0] });
                    if menu_item(ui, "Bottom border") {
                        self.apply_paras(&move |p| p.fmt.borders = Some(ParaBorders { bottom: line, ..Default::default() }));
                    }
                    if menu_item(ui, "Top border") {
                        self.apply_paras(&move |p| p.fmt.borders = Some(ParaBorders { top: line, ..Default::default() }));
                    }
                    if menu_item(ui, "Box") {
                        self.apply_paras(&move |p| p.fmt.borders = Some(ParaBorders { top: line, bottom: line, left: line, right: line }));
                    }
                    if menu_item(ui, "No border") {
                        self.apply_paras(&|p| p.fmt.borders = Some(ParaBorders::default()));
                    }
                });
            });
        });

        group(ui, "Styles", |ui| {
            egui::ScrollArea::horizontal().id_salt("styles_scroll").max_width(420.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let styles: Vec<(String, f32, bool, bool, Rgb)> = self
                        .doc
                        .styles
                        .list
                        .iter()
                        .filter(|s| !s.name.starts_with("TOC") && s.name != "Header" && s.name != "Footer")
                        .map(|s| {
                            let (c, _) = self.doc.styles.resolve(&s.name);
                            (s.name.clone(), c.size, c.bold, c.italic, c.color)
                        })
                        .collect();
                    for (name, size, b, i, col) in styles {
                        let mut t = RichText::new(&name).size(size.clamp(11.0, 17.0)).color(rgb(col));
                        if b {
                            t = t.strong();
                        }
                        if i {
                            t = t.italics();
                        }
                        let resp = ui.add(egui::Button::new(t).selected(style_name == name).min_size(vec2(72.0, 46.0)));
                        if resp.clicked() {
                            self.apply_style(&name);
                        }
                        resp.on_hover_text(format!("Apply style \"{name}\""));
                    }
                });
            });
            if ui.small_button("Update style to match selection").clicked() {
                self.update_style_from_selection();
            }
        });

        group(ui, "Editing", |ui| {
            ui.vertical(|ui| {
                if ui.button("Find…").on_hover_text("Ctrl+F").clicked() {
                    self.open_find(false);
                }
                if ui.button("Replace…").on_hover_text("Ctrl+H").clicked() {
                    self.open_find(true);
                }
                if ui.button("Select All").on_hover_text("Ctrl+A").clicked() {
                    self.select_all();
                }
            });
        });
    }

    fn font_combo(&mut self, ui: &mut Ui, current: &str) {
        let mut chosen: Option<String> = None;
        let mut more = false;
        egui::ComboBox::from_id_salt("fontfam").width(150.0).selected_text(current).show_ui(ui, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.font_filter).hint_text("Search or type a font name…").desired_width(190.0));
            self.note_focus(&r);
            let q = self.font_filter.trim().to_lowercase();
            if !q.is_empty() && ui.button(format!("Use “{}”", self.font_filter.trim())).clicked() {
                chosen = Some(self.font_filter.trim().to_string());
            }
            ui.separator();
            let names: Vec<String> = self.fonts.family_names().into_iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)).collect();
            egui::ScrollArea::vertical().max_height(300.0).show_rows(ui, 20.0, names.len(), |ui, range| {
                for n in &names[range] {
                    let g = self.fonts.is_google_installed(n);
                    let label = if g { format!("{n}   (Google)") } else { n.clone() };
                    if ui.selectable_label(n == current, label).clicked() {
                        chosen = Some(n.clone());
                    }
                }
            });
            ui.separator();
            if ui.button("More fonts from Google Fonts…").clicked() {
                more = true;
            }
        });
        if let Some(c) = chosen {
            self.set_font(&c);
            self.font_filter.clear();
        }
        if more {
            self.dlg.gfonts_open = true;
        }
    }

    // ---------------------------------------------------------------- Insert

    fn insert_tab(&mut self, ui: &mut Ui) {
        group(ui, "Pages", |ui| {
            if ui.add(egui::Button::new("Page\nBreak").min_size(vec2(50.0, 48.0))).on_hover_text("Ctrl+Enter").clicked() {
                self.insert_page_break();
            }
        });
        group(ui, "Tables", |ui| {
            let r = ui.add(egui::Button::new("Table").min_size(vec2(54.0, 48.0)));
            let mut pick: Option<(usize, usize)> = None;
            Popup::from_toggle_button_response(&r).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                let mut hover = (0usize, 0usize);
                let (rows, cols) = (8usize, 10usize);
                let origin = ui.cursor().min;
                let ptr = ui.input(|i| i.pointer.hover_pos());
                for rr in 0..rows {
                    ui.horizontal(|ui| {
                        for cc in 0..cols {
                            let (rect, resp) = ui.allocate_exact_size(vec2(17.0, 17.0), Sense::click());
                            if let Some(p) = ptr {
                                if rect.contains(p) {
                                    hover = (rr + 1, cc + 1);
                                }
                            }
                            if resp.clicked() {
                                pick = Some((rr + 1, cc + 1));
                            }
                        }
                    });
                }
                let _ = origin;
                // paint highlight on top
                let top_left = ui.min_rect().min;
                for rr in 0..rows {
                    for cc in 0..cols {
                        let rect = Rect::from_min_size(top_left + vec2(cc as f32 * 19.0, rr as f32 * 19.0 + 0.0), vec2(17.0, 17.0));
                        let on = rr < hover.0 && cc < hover.1;
                        ui.painter().rect_stroke(rect, 0.0, Stroke::new(if on { 2.0_f32 } else { 1.0_f32 }, if on { ACCENT } else { Color32::from_gray(160) }), egui::StrokeKind::Inside);
                    }
                }
                ui.label(if hover.0 > 0 { format!("{} × {} Table", hover.1, hover.0) } else { "Insert Table".to_string() });
                ui.separator();
                if ui.button("Insert Table…").clicked() {
                    self.dlg.table_open = true;
                    ui.close();
                }
            });
            if let Some((r, c)) = pick {
                self.insert_table(r, c);
            }
        });
        group(ui, "Illustrations", |ui| {
            if ui.add(egui::Button::new("Picture…").min_size(vec2(60.0, 48.0))).clicked() {
                self.insert_image_dialog();
            }
        });
        group(ui, "Links", |ui| {
            if ui.add(egui::Button::new("Link…").min_size(vec2(50.0, 48.0))).on_hover_text("Ctrl+K").clicked() {
                self.open_link_dialog();
            }
        });
        group(ui, "Header & Footer", |ui| {
            ui.horizontal(|ui| {
                if ui.selectable_label(matches!(self.kind, StoryKind::Header | StoryKind::FirstHeader), "Header").on_hover_text("Edit the page header").clicked() {
                    self.toggle_header_footer(false);
                }
                if ui.selectable_label(matches!(self.kind, StoryKind::Footer | StoryKind::FirstFooter), "Footer").on_hover_text("Edit the page footer").clicked() {
                    self.toggle_header_footer(true);
                }
            });
            let r = ui.button("Page Number");
            Popup::menu(&r).show(|ui| {
                for (label, footer, al, total) in [
                    ("Top of page — Left", false, Align::Left, false),
                    ("Top of page — Center", false, Align::Center, false),
                    ("Top of page — Right", false, Align::Right, false),
                    ("Bottom of page — Left", true, Align::Left, false),
                    ("Bottom of page — Center", true, Align::Center, false),
                    ("Bottom of page — Right", true, Align::Right, false),
                    ("Bottom — “Page X of Y”", true, Align::Center, true),
                ] {
                    if menu_item(ui, label) {
                        self.insert_page_number(footer, al, total);
                    }
                }
            });
        });
        group(ui, "Text", |ui| {
            ui.horizontal(|ui| {
                let dt = ui.button("Date & Time");
                Popup::menu(&dt).show(|ui| {
                    for (label, f) in [("%A, %B %-d, %Y", "%A, %B %-d, %Y"), ("%-d %B %Y", "%-d %B %Y"), ("%Y-%m-%d", "%Y-%m-%d"), ("%d/%m/%Y", "%d/%m/%Y"), ("%m/%d/%Y", "%m/%d/%Y"), ("%-I:%M %p", "%-I:%M %p"), ("%H:%M", "%H:%M"), ("%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M")] {
                        let preview = chrono::Local::now().format(label).to_string();
                        if menu_item(ui, &preview) {
                            self.insert_datetime(f);
                        }
                    }
                });
                if ui.button("Symbol…").clicked() {
                    self.dlg.symbol_open = true;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Horizontal Line").clicked() {
                    self.insert_hr();
                }
                if ui.button("Table of Contents").clicked() {
                    self.insert_toc();
                }
            });
        });
    }

    // ---------------------------------------------------------------- Layout

    fn layout_tab(&mut self, ui: &mut Ui) {
        let u = self.settings.units;
        let page = self.doc.page;
        group(ui, "Page Setup", |ui| {
            ui.horizontal(|ui| {
                let m = ui.add(egui::Button::new("Margins").min_size(vec2(60.0, 24.0)));
                Popup::menu(&m).show(|ui| {
                    for (label, t, b, l, r) in [("Normal  (1\")", 72.0, 72.0, 72.0, 72.0), ("Narrow  (0.5\")", 36.0, 36.0, 36.0, 36.0), ("Moderate  (1\" / 0.75\")", 72.0, 72.0, 54.0, 54.0), ("Wide  (1\" / 2\")", 72.0, 72.0, 144.0, 144.0), ("Office 2003  (1\" / 1.25\")", 72.0, 72.0, 90.0, 90.0)] {
                        if menu_item(ui, label) {
                            self.checkpoint(None);
                            self.doc.page.top = t;
                            self.doc.page.bottom = b;
                            self.doc.page.left = l;
                            self.doc.page.right = r;
                        }
                    }
                });
                let o = ui.add(egui::Button::new("Orientation").min_size(vec2(76.0, 24.0)));
                Popup::menu(&o).show(|ui| {
                    if ui.selectable_label(!page.landscape(), "Portrait").clicked() && page.landscape() {
                        self.rotate_page();
                    }
                    if ui.selectable_label(page.landscape(), "Landscape").clicked() && !page.landscape() {
                        self.rotate_page();
                    }
                });
                let s = ui.add(egui::Button::new("Size").min_size(vec2(44.0, 24.0)));
                Popup::menu(&s).show(|ui| {
                    for (name, w, h) in PAPER_SIZES {
                        let cur = (page.w.min(page.h) - w).abs() < 1.0 && (page.w.max(page.h) - h).abs() < 1.0;
                        if ui.selectable_label(cur, format!("{name}   ({:.1} × {:.1} {})", u.from_pt(w), u.from_pt(h), u.suffix().trim())).clicked() {
                            self.checkpoint(None);
                            if page.landscape() {
                                self.doc.page.w = h;
                                self.doc.page.h = w;
                            } else {
                                self.doc.page.w = w;
                                self.doc.page.h = h;
                            }
                        }
                    }
                });
            });
            if ui.button("Page Setup…").clicked() {
                self.dlg.page_open = true;
                self.dlg.page_work = self.doc.page;
            }
        });
        group(ui, "Paragraph", |ui| {
            let head = self.story().clamp(self.sel.head);
            let rp = {
                let p = self.story().para(head.p);
                let mut r = self.sc.get(&p.style).1.clone();
                r.apply(&p.fmt);
                r
            };
            let (mut left, mut right, mut before, mut after) = (u.from_pt(rp.left), u.from_pt(rp.right), rp.before, rp.after);
            let step = if u == Units::Cm { 0.1 } else { 0.05 };
            ui.horizontal(|ui| {
                ui.label("Indent  Left");
                if ui.add(egui::DragValue::new(&mut left).speed(step).range(0.0..=20.0).suffix(u.suffix())).changed() {
                    let v = u.to_pt(left);
                    self.apply_paras(&move |p| p.fmt.left = Some(v));
                }
                ui.label("Right");
                if ui.add(egui::DragValue::new(&mut right).speed(step).range(0.0..=20.0).suffix(u.suffix())).changed() {
                    let v = u.to_pt(right);
                    self.apply_paras(&move |p| p.fmt.right = Some(v));
                }
            });
            ui.horizontal(|ui| {
                ui.label("Spacing  Before");
                if ui.add(egui::DragValue::new(&mut before).speed(0.5).range(0.0..=200.0).suffix(" pt")).changed() {
                    self.set_para_spacing(Some(before), None);
                }
                ui.label("After");
                if ui.add(egui::DragValue::new(&mut after).speed(0.5).range(0.0..=200.0).suffix(" pt")).changed() {
                    self.set_para_spacing(None, Some(after));
                }
            });
            if ui.button("Paragraph…").clicked() {
                self.open_para_dialog();
            }
        });
        group(ui, "Header & Footer", |ui| {
            let mut d = page.diff_first;
            if ui.checkbox(&mut d, "Different first page").changed() {
                self.checkpoint(None);
                self.doc.page.diff_first = d;
            }
        });
    }

    fn rotate_page(&mut self) {
        self.checkpoint(None);
        let p = &mut self.doc.page;
        std::mem::swap(&mut p.w, &mut p.h);
        let (t, b, l, r) = (p.top, p.bottom, p.left, p.right);
        // rotate the margins like Word does
        p.top = r;
        p.bottom = l;
        p.left = t;
        p.right = b;
    }

    // ---------------------------------------------------------------- Review / View

    fn review_tab(&mut self, ui: &mut Ui) {
        group(ui, "Proofing", |ui| {
            if ui.add(egui::Button::new("Word Count").min_size(vec2(80.0, 48.0))).clicked() {
                self.dlg.count_open = true;
            }
        });
        group(ui, "Find", |ui| {
            if ui.add(egui::Button::new("Find &\nReplace").min_size(vec2(70.0, 48.0))).clicked() {
                self.open_find(true);
            }
        });
    }

    fn view_tab(&mut self, ui: &mut Ui) {
        group(ui, "Zoom", |ui| {
            ui.horizontal(|ui| {
                if ui.button("100%").clicked() {
                    self.zoom = 100.0;
                }
                if ui.button("Page Width").clicked() {
                    self.zoom_to(ZoomMode::PageWidth);
                }
                if ui.button("Whole Page").clicked() {
                    self.zoom_to(ZoomMode::WholePage);
                }
            });
            ui.add(egui::Slider::new(&mut self.zoom, 25.0..=400.0).suffix("%").step_by(5.0));
        });
        group(ui, "Show", |ui| {
            ui.checkbox(&mut self.show_ruler, "Ruler");
            ui.checkbox(&mut self.show_nav, "Navigation Pane");
            ui.checkbox(&mut self.show_marks, "Formatting Marks");
        });
        group(ui, "Appearance", |ui| {
            let mut dark = self.settings.dark_ui;
            if ui.checkbox(&mut dark, "Dark interface").changed() {
                self.settings.dark_ui = dark;
                self.settings.save();
                let ctx = ui.ctx().clone();
                self.apply_theme(&ctx);
            }
            ui.horizontal(|ui| {
                ui.label("Units");
                egui::ComboBox::from_id_salt("units").selected_text(self.settings.units.name()).show_ui(ui, |ui| {
                    for u in [Units::Inch, Units::Cm, Units::Pt] {
                        if ui.selectable_label(self.settings.units == u, u.name()).clicked() {
                            self.settings.units = u;
                            self.settings.save();
                        }
                    }
                });
            });
        });
    }

    // ---------------------------------------------------------------- Table / Picture

    fn table_tab(&mut self, ui: &mut Ui) {
        let Some((tr, table)) = self.table_info() else { return };
        group(ui, "Rows & Columns", |ui| {
            ui.horizontal(|ui| {
                if ui.button("Insert Above").clicked() {
                    self.table_cmd(TableCmd::RowAbove);
                }
                if ui.button("Insert Below").clicked() {
                    self.table_cmd(TableCmd::RowBelow);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Insert Left").clicked() {
                    self.table_cmd(TableCmd::ColLeft);
                }
                if ui.button("Insert Right").clicked() {
                    self.table_cmd(TableCmd::ColRight);
                }
            });
            let d = ui.button("Delete");
            Popup::menu(&d).show(|ui| {
                if menu_item(ui, "Delete Row") {
                    self.table_cmd(TableCmd::DelRow);
                }
                if menu_item(ui, "Delete Column") {
                    self.table_cmd(TableCmd::DelCol);
                }
                if menu_item(ui, "Delete Table") {
                    self.table_cmd(TableCmd::DelTable);
                }
            });
        });
        group(ui, "Merge", |ui| {
            if ui.button("Merge Cells").on_hover_text("Select several cells first").clicked() {
                self.table_cmd(TableCmd::Merge);
            }
            if ui.button("Split Cell").clicked() {
                self.table_cmd(TableCmd::Split);
            }
        });
        group(ui, "Cell Size", |ui| {
            let mut h = table.rows[tr.r].height.unwrap_or(0.0);
            ui.horizontal(|ui| {
                ui.label("Row height");
                if ui.add(egui::DragValue::new(&mut h).speed(0.5).range(0.0..=500.0).suffix(" pt")).changed() {
                    self.table_cmd(TableCmd::RowHeight(h));
                }
            });
            if ui.button("Equal column widths").clicked() {
                self.table_cmd(TableCmd::EqualCols);
            }
        });
        group(ui, "Alignment", |ui| {
            ui.horizontal(|ui| {
                for (label, v) in [("Top", VAlign::Top), ("Middle", VAlign::Center), ("Bottom", VAlign::Bottom)] {
                    if ui.button(label).on_hover_text("Vertical cell alignment").clicked() {
                        self.table_cmd(TableCmd::VAlign(v));
                    }
                }
            });
            ui.horizontal(|ui| {
                for (label, a) in [("Table left", Align::Left), ("Center", Align::Center), ("Right", Align::Right)] {
                    if ui.selectable_label(table.align == a, label).clicked() {
                        self.table_cmd(TableCmd::Align(a));
                    }
                }
            });
        });
        group(ui, "Table Styles", |ui| {
            let sh = ui.button("Shading");
            Popup::from_toggle_button_response(&sh).close_behavior(PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                if let Some(c) = palette(ui, "No Color", false) {
                    self.table_cmd(TableCmd::Shade(c));
                }
            });
            let bd = ui.button("Borders");
            Popup::menu(&bd).show(|ui| {
                let b = Some(Border { w: 0.5, color: [0, 0, 0] });
                if menu_item(ui, "All Borders") {
                    self.table_cmd(TableCmd::Borders(TableBorders::all(0.5)));
                }
                if menu_item(ui, "Outside Borders") {
                    self.table_cmd(TableCmd::Borders(TableBorders { outer: b, inner_h: None, inner_v: None }));
                }
                if menu_item(ui, "Inside Horizontal Borders") {
                    self.table_cmd(TableCmd::Borders(TableBorders { outer: None, inner_h: b, inner_v: None }));
                }
                if menu_item(ui, "Inside Vertical Borders") {
                    self.table_cmd(TableCmd::Borders(TableBorders { outer: None, inner_h: None, inner_v: b }));
                }
                if menu_item(ui, "Thick Outside Border") {
                    self.table_cmd(TableCmd::Borders(TableBorders { outer: Some(Border { w: 2.25, color: [0, 0, 0] }), inner_h: b, inner_v: b }));
                }
                if menu_item(ui, "No Border") {
                    self.table_cmd(TableCmd::Borders(TableBorders::none()));
                }
            });
            let mut hdr = table.rows[0].header;
            if ui.checkbox(&mut hdr, "Header row").changed() {
                self.table_cmd(TableCmd::HeaderRow(hdr));
            }
        });
    }

    fn picture_tab(&mut self, ui: &mut Ui) {
        let Some((para, off)) = self.selected_image() else { return };
        // find the current size
        let mut size = (0.0f32, 0.0f32);
        let mut native = (0.0f32, 0.0f32);
        {
            let p = self.story().para(para);
            let mut acc = 0;
            for r in &p.runs {
                let n = r.text.chars().count();
                if off >= acc && off < acc + n {
                    if let Some(Obj::Image(img)) = &r.obj {
                        size = (img.w, img.h);
                        native = (img.data.px_w as f32 * 0.75, img.data.px_h as f32 * 0.75);
                    }
                }
                acc += n;
            }
        }
        let u = self.settings.units;
        group(ui, "Size", |ui| {
            let (mut w, mut h) = (u.from_pt(size.0), u.from_pt(size.1));
            let ratio = if size.0 > 0.0 { size.1 / size.0 } else { 1.0 };
            ui.horizontal(|ui| {
                ui.label("Width");
                if ui.add(egui::DragValue::new(&mut w).speed(0.05).range(0.1..=50.0).suffix(u.suffix())).changed() {
                    let nw = u.to_pt(w);
                    self.set_image_size(para, off, nw, nw * ratio);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Height");
                if ui.add(egui::DragValue::new(&mut h).speed(0.05).range(0.1..=50.0).suffix(u.suffix())).changed() {
                    let nh = u.to_pt(h);
                    self.set_image_size(para, off, nh / ratio.max(0.001), nh);
                }
            });
            if ui.button("Reset size").clicked() {
                self.checkpoint(None);
                let max_w = self.doc.page.content_w();
                let k = (max_w / native.0.max(1.0)).min(1.0);
                self.set_image_size(para, off, native.0 * k, native.1 * k);
            }
        });
        group(ui, "Arrange", |ui| {
            ui.horizontal(|ui| {
                for (i, (al, tip)) in [(Align::Left, "Left"), (Align::Center, "Center"), (Align::Right, "Right")].into_iter().enumerate() {
                    if icon_btn(ui, false, tip, vec2(26.0, 24.0), |p, r, c| align_icon(p, r, c, i)).clicked() {
                        let (a, b) = self.sel.ordered();
                        self.checkpoint(None);
                        self.story_mut().modify_paras(a.p, b.p, &move |p| p.fmt.align = Some(al));
                    }
                }
            });
            if ui.button("Delete picture").clicked() {
                self.delete_selection();
            }
        });
    }
}

fn fmt_size(s: f32) -> String {
    if (s - s.round()).abs() < 0.01 {
        format!("{}", s as i32)
    } else {
        format!("{s}")
    }
}

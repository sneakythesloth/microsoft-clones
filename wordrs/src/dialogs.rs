use eframe::egui::{self, vec2, Color32, Context, FontId, Key, RichText, Ui};

use crate::app::*;
use crate::model::*;

#[derive(Default)]
pub struct FindState {
    pub query: String,
    pub replace: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub use_regex: bool,
    pub replace_mode: bool,
    pub focus: bool,
    pub matches: Vec<(Pos, Pos)>,
    pub cur: Option<usize>,
    pub status: String,
    pub invalid_regex: bool,
    pub key: u64,
}

#[derive(Default, Clone)]
pub struct ParaDlg {
    pub align: usize,
    pub left: f32,
    pub right: f32,
    pub special: usize,
    pub by: f32,
    pub before: f32,
    pub after: f32,
    pub line_kind: usize,
    pub line_val: f32,
    pub keep_next: bool,
    pub keep_lines: bool,
    pub break_before: bool,
}

#[derive(Default)]
pub struct Dialogs {
    pub find_open: bool,
    pub find: FindState,
    pub gfonts_open: bool,
    pub gf_query: String,
    pub gf_cat: usize,
    pub gf_installed_only: bool,
    pub table_open: bool,
    pub tbl_rows: usize,
    pub tbl_cols: usize,
    pub page_open: bool,
    pub page_work: PageSetup,
    pub para_open: bool,
    pub para: ParaDlg,
    pub link_open: bool,
    pub link_text: String,
    pub link_url: String,
    pub symbol_open: bool,
    pub symbol_page: usize,
    pub about_open: bool,
    pub help_open: bool,
    pub settings_open: bool,
    pub props_open: bool,
    pub count_open: bool,
}

const CATS: [&str; 6] = ["All", "Sans Serif", "Serif", "Display", "Handwriting", "Monospace"];

const SYMBOLS: [(&str, &str); 6] = [
    ("Punctuation", "©®™§¶†‡•…–—‘’“”«»‹›¿¡·′″‰№℗℠"),
    ("Currency", "€£¥¢₹₽₩₪₫฿₺₴₦₱¤$"),
    ("Math", "±×÷≠≈≤≥∞√∑∏∫∂∆∇∈∉∩∪⊂⊃⊆⊇∀∃¬∧∨⊕⊗°′″µ²³¹½¼¾‰"),
    ("Arrows", "←→↑↓↔↕⇐⇒⇑⇓⇔↩↪↖↗↘↙➔➜"),
    ("Greek", "αβγδεζηθικλμνξοπρστυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩ"),
    ("Latin", "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏÐÑÒÓÔÕÖØÙÚÛÜÝÞßàáâãäåæçèéêëìíîïðñòóôõöøùúûüýþÿŒœŠšŽž"),
];

impl App {
    pub fn open_para_dialog(&mut self) {
        let head = self.story().clamp(self.sel.head);
        let rp = {
            let p = self.story().para(head.p);
            let mut r = self.sc.get(&p.style).1.clone();
            r.apply(&p.fmt);
            r
        };
        let (line_kind, line_val) = match rp.line {
            LineSpacing::Multiple(m) if (m - 1.0).abs() < 0.01 => (0, 1.0),
            LineSpacing::Multiple(m) if (m - 1.5).abs() < 0.01 => (1, 1.5),
            LineSpacing::Multiple(m) if (m - 2.0).abs() < 0.01 => (2, 2.0),
            LineSpacing::Multiple(m) => (5, m),
            LineSpacing::AtLeast(v) => (3, v),
            LineSpacing::Exact(v) => (4, v),
        };
        self.dlg.para = ParaDlg {
            align: match rp.align {
                Align::Left => 0,
                Align::Center => 1,
                Align::Right => 2,
                Align::Justify => 3,
            },
            left: rp.left,
            right: rp.right,
            special: if rp.first > 0.5 { 1 } else if rp.first < -0.5 { 2 } else { 0 },
            by: rp.first.abs(),
            before: rp.before,
            after: rp.after,
            line_kind,
            line_val,
            keep_next: rp.keep_next,
            keep_lines: rp.keep_lines,
            break_before: rp.page_break_before,
        };
        self.dlg.para_open = true;
    }

    fn apply_para_dialog(&mut self) {
        let d = self.dlg.para.clone();
        let first = match d.special {
            1 => d.by,
            2 => -d.by,
            _ => 0.0,
        };
        let line = match d.line_kind {
            0 => LineSpacing::Multiple(1.0),
            1 => LineSpacing::Multiple(1.5),
            2 => LineSpacing::Multiple(2.0),
            3 => LineSpacing::AtLeast(d.line_val),
            4 => LineSpacing::Exact(d.line_val),
            _ => LineSpacing::Multiple(d.line_val),
        };
        let al = [Align::Left, Align::Center, Align::Right, Align::Justify][d.align.min(3)];
        self.apply_paras(&move |p| {
            p.fmt.align = Some(al);
            p.fmt.left = Some(d.left);
            p.fmt.right = Some(d.right);
            p.fmt.first = Some(first);
            p.fmt.before = Some(d.before);
            p.fmt.after = Some(d.after);
            p.fmt.line = Some(line);
            p.fmt.keep_next = Some(d.keep_next);
            p.fmt.keep_lines = Some(d.keep_lines);
            p.fmt.page_break_before = Some(d.break_before);
        });
    }

    pub fn dialogs_ui(&mut self, ctx: &Context) {
        if self.dlg.tbl_rows == 0 {
            self.dlg.tbl_rows = 3;
            self.dlg.tbl_cols = 3;
        }
        self.find_dialog(ctx);
        self.gfonts_dialog(ctx);
        self.simple_dialogs(ctx);
    }

    // ---------------------------------------------------------------- find & replace

    fn find_dialog(&mut self, ctx: &Context) {
        if !self.dlg.find_open {
            return;
        }
        if self.dlg.find.key != self.doc.body.ver {
            self.find_recompute();
        }
        let mut open = true;
        egui::Window::new(if self.dlg.find.replace_mode { "Find and Replace" } else { "Find" }).open(&mut open).resizable(false).default_pos(egui::pos2(ctx.content_rect().right() - 360.0, 150.0)).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Find:    ");
                let r = ui.add(egui::TextEdit::singleline(&mut self.dlg.find.query).desired_width(220.0));
                self.note_focus(&r);
                if self.dlg.find.focus {
                    r.request_focus();
                    self.dlg.find.focus = false;
                    // keep the editor from taking focus back at the end of this frame
                    self.text_focus = true;
                }
                if r.has_focus() && ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.dlg.find_open = false;
                }
                if r.changed() {
                    self.find_recompute();
                    self.dlg.find.cur = None;
                    self.dlg.find.status = format!("{} results", self.dlg.find.matches.len());
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let back = ui.input(|i| i.modifiers.shift);
                    self.find_next(!back);
                    r.request_focus();
                }
            });
            if self.dlg.find.replace_mode {
                ui.horizontal(|ui| {
                    ui.label("Replace:");
                    let r = ui.add(egui::TextEdit::singleline(&mut self.dlg.find.replace).desired_width(220.0));
                    self.note_focus(&r);
                });
            }
            ui.horizontal(|ui| {
                let mut ch = false;
                ch |= ui.checkbox(&mut self.dlg.find.match_case, "Match case").changed();
                ch |= ui.checkbox(&mut self.dlg.find.whole_word, "Whole word").changed();
                ch |= ui.checkbox(&mut self.dlg.find.use_regex, "Regex").changed();
                if ch {
                    self.find_recompute();
                    self.dlg.find.cur = None;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Find Next").clicked() {
                    self.find_next(true);
                }
                if ui.button("Previous").clicked() {
                    self.find_next(false);
                }
                if self.dlg.find.replace_mode {
                    if ui.button("Replace").clicked() {
                        self.replace_current();
                    }
                    if ui.button("Replace All").clicked() {
                        self.replace_all();
                    }
                } else if ui.button("Replace…").clicked() {
                    self.dlg.find.replace_mode = true;
                }
            });
            if self.dlg.find.invalid_regex {
                ui.colored_label(Color32::from_rgb(200, 40, 40), "Invalid regular expression");
            } else if !self.dlg.find.status.is_empty() {
                ui.label(RichText::new(self.dlg.find.status.clone()).weak());
            } else if !self.dlg.find.query.is_empty() {
                ui.label(RichText::new(format!("{} results", self.dlg.find.matches.len())).weak());
            }
        });
        self.dlg.find_open = open && self.dlg.find_open;
    }

    // ---------------------------------------------------------------- Google Fonts

    fn gfonts_dialog(&mut self, ctx: &Context) {
        if !self.dlg.gfonts_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Google Fonts").open(&mut open).default_size(vec2(560.0, 520.0)).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.dlg.gf_query).hint_text("Search families…").desired_width(220.0));
                self.note_focus(&r);
                egui::ComboBox::from_id_salt("gf_cat").selected_text(CATS[self.dlg.gf_cat]).show_ui(ui, |ui| {
                    for (i, c) in CATS.iter().enumerate() {
                        ui.selectable_value(&mut self.dlg.gf_cat, i, *c);
                    }
                });
                ui.checkbox(&mut self.dlg.gf_installed_only, "Installed only");
            });
            ui.horizontal(|ui| {
                if self.fonts.google.loading_catalog {
                    ui.spinner();
                    ui.label("Updating catalog…");
                } else if ui.button("Refresh catalog").on_hover_text("Download the full list of Google Fonts families").clicked() {
                    self.fonts.refresh_catalog();
                }
                let n = self.fonts.google.catalog.len();
                ui.label(RichText::new(format!("{n} families{}", if self.fonts.google.catalog_online { "" } else { " (built-in list — refresh for all)" })).weak());
            });
            if !self.fonts.google.status.is_empty() {
                ui.label(RichText::new(self.fonts.google.status.clone()).small().color(Color32::from_gray(90)));
            }
            ui.separator();
            let q = self.dlg.gf_query.trim().to_lowercase();
            let cat = CATS[self.dlg.gf_cat];
            let list: Vec<(String, String)> = self
                .fonts
                .google
                .catalog
                .iter()
                .filter(|g| (q.is_empty() || g.family.to_lowercase().contains(&q)) && (cat == "All" || g.category == cat))
                .filter(|g| !self.dlg.gf_installed_only || self.fonts.is_google_installed(&g.family))
                .map(|g| (g.family.clone(), g.category.clone()))
                .collect();
            let mut download: Option<(String, bool)> = None;
            let mut use_now: Option<String> = None;
            egui::ScrollArea::vertical().auto_shrink([false, false]).max_height(ui.available_height() - 70.0).show_rows(ui, 44.0, list.len(), |ui, range| {
                for (name, category) in &list[range] {
                    let installed = self.fonts.is_google_installed(name);
                    let busy = self.fonts.google.in_flight.contains(name);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(300.0);
                            if installed {
                                let res = self.fonts.resolve(name, false, false);
                                let fam = self.fonts.egui_family(res.face);
                                ui.label(RichText::new(name).font(FontId::new(20.0, fam)));
                            } else {
                                ui.label(RichText::new(name).size(15.0));
                            }
                            ui.label(RichText::new(category).small().weak());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if busy {
                                ui.spinner();
                            } else if installed {
                                if ui.button("Use").on_hover_text("Apply to the selected text").clicked() {
                                    use_now = Some(name.clone());
                                }
                                ui.label(RichText::new("Installed").color(Color32::from_rgb(40, 140, 60)));
                            } else {
                                if ui.button("Download & use").clicked() {
                                    download = Some((name.clone(), true));
                                }
                                if ui.button("Download").clicked() {
                                    download = Some((name.clone(), false));
                                }
                            }
                        });
                    });
                    ui.separator();
                }
            });
            if let Some((name, apply)) = download {
                self.fonts.request_google(&name);
                if apply {
                    self.set_font(&name);
                }
            }
            if let Some(n) = use_now {
                self.set_font(&n);
            }
            ui.separator();
            ui.label(RichText::new(format!("Fonts are cached in {}", crate::google::cache_root().display())).small().weak());
        });
        self.dlg.gfonts_open = open && self.dlg.gfonts_open;
    }

    // ---------------------------------------------------------------- everything else

    fn simple_dialogs(&mut self, ctx: &Context) {
        let u = self.settings.units;
        // insert table
        if self.dlg.table_open {
            let mut open = true;
            egui::Window::new("Insert Table").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -60.0)).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Columns:");
                    ui.add(egui::DragValue::new(&mut self.dlg.tbl_cols).range(1..=30));
                    ui.label("Rows:");
                    ui.add(egui::DragValue::new(&mut self.dlg.tbl_rows).range(1..=200));
                });
                if ui.button("Insert").clicked() {
                    let (r, c) = (self.dlg.tbl_rows, self.dlg.tbl_cols);
                    self.insert_table(r, c);
                    self.dlg.table_open = false;
                }
            });
            self.dlg.table_open = open && self.dlg.table_open;
        }
        // page setup
        if self.dlg.page_open {
            let mut open = true;
            egui::Window::new("Page Setup").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
                let w = &mut self.dlg.page_work;
                egui::Grid::new("pgsetup").num_columns(4).spacing([10.0, 6.0]).show(ui, |ui| {
                    let field = |ui: &mut Ui, label: &str, v: &mut f32| {
                        ui.label(label);
                        let mut x = u.from_pt(*v);
                        if ui.add(egui::DragValue::new(&mut x).speed(0.05).range(0.0..=20.0).suffix(u.suffix())).changed() {
                            *v = u.to_pt(x);
                        }
                    };
                    field(ui, "Top", &mut w.top);
                    field(ui, "Bottom", &mut w.bottom);
                    ui.end_row();
                    field(ui, "Left", &mut w.left);
                    field(ui, "Right", &mut w.right);
                    ui.end_row();
                    field(ui, "Header", &mut w.header_dist);
                    field(ui, "Footer", &mut w.footer_dist);
                    ui.end_row();
                    field(ui, "Width", &mut w.w);
                    field(ui, "Height", &mut w.h);
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    ui.label("Paper:");
                    for (name, pw, ph) in PAPER_SIZES {
                        if ui.small_button(name).clicked() {
                            if w.landscape() {
                                w.w = ph;
                                w.h = pw;
                            } else {
                                w.w = pw;
                                w.h = ph;
                            }
                        }
                    }
                });
                ui.checkbox(&mut w.diff_first, "Different first page header/footer");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        let work = self.dlg.page_work;
                        if work.w - work.left - work.right >= 36.0 && work.h - work.top - work.bottom >= 36.0 {
                            self.checkpoint(None);
                            self.doc.page = work;
                            self.dlg.page_open = false;
                        } else {
                            self.say("Margins are too large for this page size");
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        self.dlg.page_open = false;
                    }
                });
            });
            self.dlg.page_open = open && self.dlg.page_open;
        }
        // paragraph
        if self.dlg.para_open {
            let mut open = true;
            let mut apply = false;
            egui::Window::new("Paragraph").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
                let d = &mut self.dlg.para;
                egui::Grid::new("paragrid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                    ui.label("Alignment");
                    egui::ComboBox::from_id_salt("pa_align").selected_text(["Left", "Centered", "Right", "Justified"][d.align.min(3)]).show_ui(ui, |ui| {
                        for (i, n) in ["Left", "Centered", "Right", "Justified"].iter().enumerate() {
                            ui.selectable_value(&mut d.align, i, *n);
                        }
                    });
                    ui.end_row();
                    let unit_field = |ui: &mut Ui, label: &str, v: &mut f32| {
                        ui.label(label);
                        let mut x = u.from_pt(*v);
                        if ui.add(egui::DragValue::new(&mut x).speed(0.05).range(0.0..=20.0).suffix(u.suffix())).changed() {
                            *v = u.to_pt(x);
                        }
                        ui.end_row();
                    };
                    unit_field(ui, "Indent left", &mut d.left);
                    unit_field(ui, "Indent right", &mut d.right);
                    ui.label("Special");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("pa_special").selected_text(["(none)", "First line", "Hanging"][d.special.min(2)]).show_ui(ui, |ui| {
                            for (i, n) in ["(none)", "First line", "Hanging"].iter().enumerate() {
                                ui.selectable_value(&mut d.special, i, *n);
                            }
                        });
                        if d.special > 0 {
                            let mut x = u.from_pt(d.by);
                            if ui.add(egui::DragValue::new(&mut x).speed(0.05).range(0.0..=20.0).suffix(u.suffix())).changed() {
                                d.by = u.to_pt(x);
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Spacing before");
                    ui.add(egui::DragValue::new(&mut d.before).speed(0.5).range(0.0..=300.0).suffix(" pt"));
                    ui.end_row();
                    ui.label("Spacing after");
                    ui.add(egui::DragValue::new(&mut d.after).speed(0.5).range(0.0..=300.0).suffix(" pt"));
                    ui.end_row();
                    ui.label("Line spacing");
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("pa_line").selected_text(["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"][d.line_kind.min(5)]).show_ui(ui, |ui| {
                            for (i, n) in ["Single", "1.5 lines", "Double", "At least", "Exactly", "Multiple"].iter().enumerate() {
                                ui.selectable_value(&mut d.line_kind, i, *n);
                            }
                        });
                        if d.line_kind >= 3 {
                            ui.add(egui::DragValue::new(&mut d.line_val).speed(0.05).range(0.1..=500.0));
                        }
                    });
                    ui.end_row();
                });
                ui.checkbox(&mut d.keep_next, "Keep with next");
                ui.checkbox(&mut d.keep_lines, "Keep lines together");
                ui.checkbox(&mut d.break_before, "Page break before");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        self.dlg.para_open = false;
                    }
                });
            });
            if apply {
                self.apply_para_dialog();
                self.dlg.para_open = false;
            }
            self.dlg.para_open = open && self.dlg.para_open;
        }
        // hyperlink
        if self.dlg.link_open {
            let mut open = true;
            let mut action = 0;
            egui::Window::new("Insert Hyperlink").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
                egui::Grid::new("linkgrid").num_columns(2).show(ui, |ui| {
                    ui.label("Text to display:");
                    let r = ui.add(egui::TextEdit::singleline(&mut self.dlg.link_text).desired_width(280.0));
                    self.note_focus(&r);
                    ui.end_row();
                    ui.label("Address:");
                    let r = ui.add(egui::TextEdit::singleline(&mut self.dlg.link_url).desired_width(280.0).hint_text("https://example.com"));
                    self.note_focus(&r);
                    ui.end_row();
                });
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        action = 1;
                    }
                    if ui.button("Remove link").clicked() {
                        action = 2;
                    }
                    if ui.button("Cancel").clicked() {
                        action = 3;
                    }
                });
            });
            match action {
                1 => {
                    let (t, url) = (self.dlg.link_text.clone(), self.dlg.link_url.clone());
                    self.apply_link(&t, &url);
                    self.dlg.link_open = false;
                }
                2 => {
                    self.apply_link("", "");
                    self.dlg.link_open = false;
                }
                3 => self.dlg.link_open = false,
                _ => {}
            }
            self.dlg.link_open = open && self.dlg.link_open;
        }
        // symbols
        if self.dlg.symbol_open {
            let mut open = true;
            let mut pick: Option<char> = None;
            egui::Window::new("Symbol").open(&mut open).resizable(false).default_width(420.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, (n, _)) in SYMBOLS.iter().enumerate() {
                        ui.selectable_value(&mut self.dlg.symbol_page, i, *n);
                    }
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
                    for c in SYMBOLS[self.dlg.symbol_page.min(SYMBOLS.len() - 1)].1.chars() {
                        if ui.add(egui::Button::new(RichText::new(c.to_string()).size(18.0)).min_size(vec2(30.0, 30.0))).clicked() {
                            pick = Some(c);
                        }
                    }
                });
                ui.label(RichText::new("Click a symbol to insert it at the cursor.").small().weak());
            });
            if let Some(c) = pick {
                self.insert_symbol(c);
            }
            self.dlg.symbol_open = open && self.dlg.symbol_open;
        }
        // word count
        if self.dlg.count_open {
            let (words, chars) = self.counts();
            let st = &self.doc.body;
            let mut chars_nospace = 0;
            let mut paras = 0;
            for i in 0..st.len() {
                let t = st.para(i).plain();
                chars_nospace += t.chars().filter(|c| !c.is_whitespace()).count();
                if !t.trim().is_empty() {
                    paras += 1;
                }
            }
            let pages = self.total_pages();
            let mut open = true;
            egui::Window::new("Word Count").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
                egui::Grid::new("wc").num_columns(2).spacing([24.0, 4.0]).show(ui, |ui| {
                    for (k, v) in [("Pages", pages), ("Words", words), ("Characters (no spaces)", chars_nospace), ("Characters (with spaces)", chars), ("Paragraphs", paras)] {
                        ui.label(k);
                        ui.label(RichText::new(v.to_string()).strong());
                        ui.end_row();
                    }
                });
            });
            self.dlg.count_open = open && self.dlg.count_open;
        }
        // properties
        if self.dlg.props_open {
            let mut open = true;
            egui::Window::new("Document Properties").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
                egui::Grid::new("props").num_columns(2).show(ui, |ui| {
                    ui.label("Title");
                    let r = ui.text_edit_singleline(&mut self.doc.meta.title);
                    self.note_focus(&r);
                    if r.changed() {
                        self.dirty = true;
                    }
                    ui.end_row();
                    ui.label("Author");
                    let r = ui.text_edit_singleline(&mut self.doc.meta.author);
                    self.note_focus(&r);
                    if r.changed() {
                        self.dirty = true;
                    }
                    ui.end_row();
                });
            });
            self.dlg.props_open = open && self.dlg.props_open;
        }
        // settings
        if self.dlg.settings_open {
            let mut open = true;
            egui::Window::new("Settings").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
                let mut changed = false;
                changed |= ui.checkbox(&mut self.settings.smart_quotes, "Replace straight quotes with smart quotes").changed();
                if ui.checkbox(&mut self.settings.auto_download, "Automatically download missing fonts from Google Fonts").on_hover_text("When a document uses a font that is not installed (for example Arial or Cambria), a metric-compatible Google font is downloaded and used.").changed() {
                    self.fonts.auto_download = self.settings.auto_download;
                    changed = true;
                }
                ui.separator();
                ui.label("Google Fonts API key (optional)");
                let r = ui.add(egui::TextEdit::singleline(&mut self.settings.api_key).password(true).desired_width(300.0).hint_text("AIza…"));
                self.note_focus(&r);
                if r.changed() {
                    self.fonts.google.api_key = self.settings.api_key.clone();
                    changed = true;
                }
                ui.label(RichText::new("Without a key the public CSS endpoint and the google/fonts GitHub mirror are used. A free key from the Google Cloud console enables the official Developer API.").small().weak());
                ui.separator();
                ui.label(format!("Config: {}", dirs::config_dir().map(|d| d.join("wordrs").display().to_string()).unwrap_or_default()));
                if changed {
                    self.settings.save();
                }
            });
            self.dlg.settings_open = open && self.dlg.settings_open;
        }
        // help
        if self.dlg.help_open {
            let mut open = true;
            egui::Window::new("Keyboard shortcuts").open(&mut open).resizable(false).show(ctx, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([24.0, 3.0]).show(ui, |ui| {
                    for (k, v) in [
                        ("Ctrl+N / O / S", "New / Open / Save"),
                        ("Ctrl+Shift+S", "Save as"),
                        ("Ctrl+P", "Print (opens a PDF)"),
                        ("Ctrl+Z / Ctrl+Y", "Undo / Redo"),
                        ("Ctrl+B / I / U", "Bold / Italic / Underline"),
                        ("Ctrl+[ / ]", "Shrink / grow font"),
                        ("Ctrl+= / Ctrl+Shift+=", "Subscript / Superscript"),
                        ("Ctrl+Space", "Clear character formatting"),
                        ("Ctrl+L / E / R / J", "Align left / center / right / justify"),
                        ("Ctrl+1 / 5 / 2", "Line spacing 1.0 / 1.5 / 2.0"),
                        ("Ctrl+Shift+L", "Bulleted list"),
                        ("Ctrl+Alt+1/2/3", "Heading 1 / 2 / 3"),
                        ("Tab / Shift+Tab", "Indent list item / next-previous table cell"),
                        ("Ctrl+Enter", "Page break"),
                        ("Shift+Enter", "Line break"),
                        ("Ctrl+F / Ctrl+H", "Find / Replace"),
                        ("Ctrl+K", "Insert hyperlink"),
                        ("Ctrl+click", "Open a hyperlink"),
                        ("Ctrl+Shift+8", "Show formatting marks"),
                        ("Ctrl+A", "Select all"),
                        ("Ctrl+←/→", "Move by word"),
                        ("Ctrl+scroll", "Zoom"),
                        ("Double-click header/footer area", "Edit header/footer"),
                    ] {
                        ui.label(RichText::new(k).strong());
                        ui.label(v);
                        ui.end_row();
                    }
                });
            });
            self.dlg.help_open = open && self.dlg.help_open;
        }
        // about
        if self.dlg.about_open {
            let mut open = true;
            egui::Window::new("About Word RS").open(&mut open).resizable(false).anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)).show(ctx, |ui| {
                ui.heading("Word RS");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A word processor written in Rust with egui.");
                ui.label("Reads and writes .docx, exports PDF / HTML / text / Markdown, and downloads fonts from Google Fonts.");
                ui.separator();
                ui.label(RichText::new("Bundled font: Carlito (SIL Open Font License) — metric-compatible with Calibri.").small().weak());
            });
            self.dlg.about_open = open && self.dlg.about_open;
        }
    }
}

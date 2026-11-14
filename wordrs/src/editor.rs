use std::cell::Cell;
use std::time::{Duration, Instant};

use eframe::egui::{self, Context, Key};

use crate::app::*;
use crate::model::*;
use crate::story::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    Home,
    End,
    DocStart,
    DocEnd,
    PageUp,
    PageDown,
    ParaStart,
    ParaEnd,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CaseMode {
    Upper,
    Lower,
    Title,
    Sentence,
    Toggle,
}

pub const SIZES: [f32; 17] = [8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0];

#[derive(Clone, Debug)]
pub enum TableCmd {
    RowAbove,
    RowBelow,
    ColLeft,
    ColRight,
    DelRow,
    DelCol,
    DelTable,
    Merge,
    Split,
    Shade(Option<Rgb>),
    VAlign(VAlign),
    Borders(TableBorders),
    Align(Align),
    HeaderRow(bool),
    EqualCols,
    RowHeight(f32),
}

fn smart_quote(c: char, prev: Option<char>) -> char {
    let open = match prev {
        None => true,
        Some(p) => p.is_whitespace() || "([{<\u{2014}\u{2013}-\u{201C}\u{2018}/".contains(p),
    };
    match c {
        '"' => {
            if open {
                '\u{201C}'
            } else {
                '\u{201D}'
            }
        }
        '\'' => {
            if open {
                '\u{2018}'
            } else {
                '\u{2019}'
            }
        }
        _ => c,
    }
}

impl App {
    // ------------------------------------------------------------ history

    pub fn checkpoint(&mut self, key: Option<(u8, usize)>) {
        let now = Instant::now();
        self.dirty = true;
        if let (Some(k), Some((lk, t))) = (key, self.hist.last) {
            if k == lk && now.duration_since(t) < Duration::from_millis(1500) {
                self.hist.last = Some((k, now));
                return;
            }
        }
        self.hist.undo.push(Snapshot { doc: self.doc.clone(), sel: self.sel, kind: self.kind });
        if self.hist.undo.len() > 300 {
            self.hist.undo.remove(0);
        }
        self.hist.redo.clear();
        self.hist.last = key.map(|k| (k, now));
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { doc: self.doc.clone(), sel: self.sel, kind: self.kind }
    }

    fn restore(&mut self, s: Snapshot) {
        if std::env::var("WORDRS_DEBUG").is_ok() {
        }
        self.doc = s.doc;
        self.sel = s.sel;
        self.kind = s.kind;
        self.pending = CharFmt::default();
        self.pag_key = None;
        self.hist.last = None;
        self.dirty = true;
        self.scroll_to_caret = true;
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.hist.undo.pop() {
            let cur = self.snapshot();
            self.hist.redo.push(cur);
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.hist.redo.pop() {
            let cur = self.snapshot();
            self.hist.undo.push(cur);
            self.restore(s);
        }
    }

    // ------------------------------------------------------------ events

    pub fn handle_events(&mut self, ctx: &Context) {
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            match ev {
                egui::Event::Text(t) => self.type_text(&t),
                egui::Event::Ime(egui::ImeEvent::Commit(t)) => self.type_text(&t),
                egui::Event::Paste(s) => self.paste_text(&s),
                egui::Event::Copy => self.copy(false, ctx),
                egui::Event::Cut => self.copy(true, ctx),
                egui::Event::Key { key, pressed: true, modifiers, .. } => self.on_key(key, modifiers, ctx),
                _ => {}
            }
        }
    }

    pub fn handle_dropped_files(&mut self, ctx: &Context) {
        let files = ctx.input(|i| i.raw.dropped_files.clone());
        for f in files {
            if let Some(p) = f.path {
                let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                if ["png", "jpg", "jpeg", "gif", "bmp", "webp"].contains(&ext.as_str()) {
                    self.insert_image_path(&p);
                } else if ["docx", "txt", "md"].contains(&ext.as_str()) {
                    if self.dirty {
                        self.confirm = Some(Confirm::Open(Some(p)));
                    } else {
                        self.open_path(p);
                    }
                }
            }
        }
    }

    fn on_key(&mut self, key: Key, m: egui::Modifiers, ctx: &Context) {
        let (cmd, shift, alt) = (m.command, m.shift, m.alt);
        if cmd && alt {
            match key {
                Key::Num1 => self.apply_style("Heading 1"),
                Key::Num2 => self.apply_style("Heading 2"),
                Key::Num3 => self.apply_style("Heading 3"),
                _ => {}
            }
            return;
        }
        if cmd {
            match key {
                Key::B => self.toggle_bold(),
                Key::I => self.toggle_italic(),
                Key::U => self.toggle_underline(),
                Key::Z => {
                    if shift {
                        self.redo()
                    } else {
                        self.undo()
                    }
                }
                Key::Y => self.redo(),
                Key::A => self.select_all(),
                Key::S => {
                    if shift {
                        self.save_as_dialog();
                    } else {
                        self.save();
                    }
                }
                Key::O => self.open_request(),
                Key::N => self.new_request(),
                Key::P => self.print(),
                Key::F => self.open_find(false),
                Key::H => self.open_find(true),
                Key::K => self.open_link_dialog(),
                Key::E => self.set_align(Align::Center),
                Key::L => {
                    if shift {
                        self.toggle_list(ListKind::Bullet)
                    } else {
                        self.set_align(Align::Left)
                    }
                }
                Key::R => self.set_align(Align::Right),
                Key::J => self.set_align(Align::Justify),
                Key::Enter => self.insert_page_break(),
                Key::Home => self.move_caret(Move::DocStart, shift),
                Key::End => self.move_caret(Move::DocEnd, shift),
                Key::ArrowLeft => self.move_caret(Move::WordLeft, shift),
                Key::ArrowRight => self.move_caret(Move::WordRight, shift),
                Key::ArrowUp => self.move_caret(Move::ParaStart, shift),
                Key::ArrowDown => self.move_caret(Move::ParaEnd, shift),
                Key::Backspace => self.backspace(true),
                Key::Delete => self.delete_fwd(true),
                Key::OpenBracket => self.size_step(-1),
                Key::CloseBracket => self.size_step(1),
                Key::Equals => {
                    if shift {
                        self.toggle_vert(VertAlign::Super)
                    } else {
                        self.toggle_vert(VertAlign::Sub)
                    }
                }
                Key::Plus => self.toggle_vert(VertAlign::Super),
                Key::Space => self.clear_char_formatting(),
                Key::Num1 => self.set_line_spacing(1.0),
                Key::Num2 => self.set_line_spacing(2.0),
                Key::Num5 => self.set_line_spacing(1.5),
                Key::Num8 => {
                    if shift {
                        self.show_marks = !self.show_marks;
                    }
                }
                _ => {}
            }
            return;
        }
        match key {
            Key::ArrowLeft => self.move_caret(Move::Left, shift),
            Key::ArrowRight => self.move_caret(Move::Right, shift),
            Key::ArrowUp => self.move_caret(Move::Up, shift),
            Key::ArrowDown => self.move_caret(Move::Down, shift),
            Key::Home => self.move_caret(Move::Home, shift),
            Key::End => self.move_caret(Move::End, shift),
            Key::PageUp => self.move_caret(Move::PageUp, shift),
            Key::PageDown => self.move_caret(Move::PageDown, shift),
            Key::Enter => self.enter(shift),
            Key::Backspace => self.backspace(false),
            Key::Delete => self.delete_fwd(false),
            Key::Tab => self.tab(shift),
            Key::Escape => {
                self.dlg.find_open = false;
                self.format_painter = None;
                let h = self.sel.head;
                self.sel = Sel::caret(h);
            }
            _ => {}
        }
        let _ = ctx;
    }

    // ------------------------------------------------------------ caret movement

    pub fn set_caret(&mut self, p: Pos) {
        let p = self.story().clamp(p);
        self.sel = Sel::caret(p);
        self.pending = CharFmt::default();
        self.scroll_to_caret = true;
    }

    pub fn select_all(&mut self) {
        let end = self.story().end_pos();
        self.sel = Sel { anchor: Pos::default(), head: end };
    }

    pub fn move_caret(&mut self, mv: Move, extend: bool) {
        self.ensure_layout();
        let head = self.story().clamp(self.sel.head);
        let (a, b) = self.sel.ordered();
        let collapse = !self.sel.is_empty() && !extend;
        let mut vertical = false;
        let mut goal = self.goal_x;
        let end_pos = self.story().end_pos();
        let story = self.story();
        let new = match mv {
            Move::Left => {
                if collapse {
                    a
                } else {
                    story.prev_pos(head)
                }
            }
            Move::Right => {
                if collapse {
                    b
                } else {
                    story.next_pos(head)
                }
            }
            Move::WordLeft => story.word_left(head),
            Move::WordRight => story.word_right(head),
            Move::DocStart => Pos::default(),
            Move::DocEnd => story.end_pos(),
            Move::ParaStart => {
                if head.o > 0 || head.p == 0 {
                    Pos::new(head.p, 0)
                } else {
                    Pos::new(head.p - 1, 0)
                }
            }
            Move::ParaEnd => {
                if head.p + 1 < story.len() {
                    Pos::new(head.p + 1, 0)
                } else {
                    story.end_pos()
                }
            }
            Move::Home | Move::End => {
                let pag = self.pag.as_ref().unwrap();
                match pag.line_of(self.kind, head, self.active_page) {
                    Some((_, pl)) => {
                        let ln = &pl.layout.lines[pl.line];
                        if mv == Move::Home {
                            Pos::new(head.p, ln.start)
                        } else {
                            Pos::new(head.p, ln.caret_end)
                        }
                    }
                    None => head,
                }
            }
            Move::Up | Move::Down | Move::PageUp | Move::PageDown => {
                vertical = true;
                let pag = self.pag.as_ref().unwrap();
                let down = matches!(mv, Move::Down | Move::PageDown);
                let count = if matches!(mv, Move::PageUp | Move::PageDown) { (((self.view_h / self.view_scale) - 40.0) / 15.0).max(3.0) as usize } else { 1 };
                match pag.line_of(self.kind, head, self.active_page) {
                    Some((pg, pl)) => {
                        let ln = &pl.layout.lines[pl.line];
                        let x = goal.unwrap_or(pl.x + ln.x_at(head.o));
                        goal = Some(x);
                        let (mut cpg, mut cpara, mut cline) = (pg, pl.para, pl.line);
                        let mut result = None;
                        for _ in 0..count {
                            match pag.neighbor_line(self.kind, cpg, cpara, cline, down, x) {
                                Some((npg, npos)) => {
                                    result = Some((npg, npos));
                                    match pag.line_of(self.kind, npos, npg) {
                                        Some((g2, pl2)) => {
                                            cpg = g2;
                                            cpara = pl2.para;
                                            cline = pl2.line;
                                        }
                                        None => break,
                                    }
                                }
                                None => break,
                            }
                        }
                        match result {
                            Some((npg, npos)) => {
                                if self.kind != StoryKind::Body {
                                    self.active_page = npg;
                                }
                                npos
                            }
                            None => {
                                if down {
                                    end_pos
                                } else {
                                    Pos::default()
                                }
                            }
                        }
                    }
                    None => head,
                }
            }
        };
        self.goal_x = if vertical { goal } else { None };
        self.sel.head = new;
        if !extend {
            self.sel.anchor = new;
        }
        self.pending = CharFmt::default();
        self.scroll_to_caret = true;
    }

    // ------------------------------------------------------------ text input

    fn fmt_for_typing(&self, at: Pos, selected: bool) -> CharFmt {
        let story = self.story();
        let p = story.clamp(at);
        let probe = if selected && p.o < story.plen(p.p) { Pos::new(p.p, p.o + 1) } else { p };
        let (mut f, _) = story.fmt_at(probe);
        f.merge(&self.pending);
        f
    }

    /// Delete the selection without recording history. Returns the caret afterwards.
    fn delete_selection_raw(&mut self) -> Pos {
        let (a, b) = self.sel.ordered();
        if a == b {
            return a;
        }
        let story = self.story_mut();
        let a = story.clamp(a);
        let b = story.clamp(b);
        // selection across table cells: clear the cell contents instead of merging
        let r = if let Some((blk, r0, c0, r1, c1)) = story.cell_range(a.p, b.p).filter(|_| !story.same_container(a.p, b.p)) {
            let paras = story.table_paras(blk);
            let mut targets = vec![];
            for p in paras {
                if let Loc::Cell { r, c, .. } = story.loc(p) {
                    if r >= r0 && r <= r1 && c >= c0 && c <= c1 {
                        targets.push(p);
                    }
                }
            }
            for p in targets {
                let n = story.plen(p);
                story.para_mut(p).delete(0, n);
            }
            story.cell_first_para(blk, r0, c0).map(|p| Pos::new(p, 0)).unwrap_or(a)
        } else {
            story.delete(a, b)
        };
        self.sel = Sel::caret(r);
        r
    }

    pub fn delete_selection(&mut self) {
        if self.sel.is_empty() {
            return;
        }
        self.checkpoint(None);
        self.delete_selection_raw();
        self.pending = CharFmt::default();
        self.scroll_to_caret = true;
    }

    pub fn type_text(&mut self, text: &str) {
        let mut t: String = text.chars().filter(|c| !c.is_control() || *c == '\t').collect();
        if t.is_empty() {
            return;
        }
        if self.selected_image().is_none() && self.sel.is_empty() {
            let p = self.sel.head.p;
            self.checkpoint(Some((1, p)));
        } else {
            self.checkpoint(None);
        }
        let had_sel = !self.sel.is_empty();
        let (a, _) = self.sel.ordered();
        let fmt = self.fmt_for_typing(a, had_sel);
        let at = self.delete_selection_raw();
        let at = self.story().clamp(at);
        if self.settings.smart_quotes && (t.contains('"') || t.contains('\'')) {
            let prev = if at.o > 0 { self.story().para(at.p).text().chars().nth(at.o - 1) } else { None };
            let mut out = String::new();
            let mut pv = prev;
            for c in t.chars() {
                let n = smart_quote(c, pv);
                out.push(n);
                pv = Some(n);
            }
            t = out;
        }
        let np = self.story_mut().insert_text(at, &t, Some(&fmt));
        self.sel = Sel::caret(np);
        self.goal_x = None;
        self.scroll_to_caret = true;
        if t == " " {
            self.autoformat(np);
        }
    }

    /// "1. ", "- " and "* " at the start of a paragraph start a list, like Word's AutoFormat.
    fn autoformat(&mut self, at: Pos) {
        let text = self.story().para(at.p).text();
        let before: String = text.chars().take(at.o).collect();
        let kind = match before.as_str() {
            "- " | "* " | "\u{2022} " => Some(ListKind::Bullet),
            s if s.len() >= 3 && s.ends_with(". ") && s[..s.len() - 2].chars().all(|c| c.is_ascii_digit()) => Some(ListKind::Decimal),
            "a. " => Some(ListKind::LowerAlpha),
            "i. " => Some(ListKind::LowerRoman),
            _ => None,
        };
        if let Some(k) = kind {
            if self.story().para(at.p).list.is_none() && self.story().para(at.p).len() == at.o {
                self.story_mut().para_mut(at.p).delete(0, at.o);
                self.sel = Sel::caret(Pos::new(at.p, 0));
                self.set_list_for(at.p, at.p, Some(k));
            }
        }
    }

    pub fn paste_text(&mut self, s: &str) {
        let s = s.replace("\r\n", "\n");
        if s.is_empty() {
            return;
        }
        self.checkpoint(None);
        let had_sel = !self.sel.is_empty();
        let (a, _) = self.sel.ordered();
        let fmt = self.fmt_for_typing(a, had_sel);
        let at = self.delete_selection_raw();
        let at = self.story().clamp(at);
        let rich = match &self.clip {
            Some((t, frag)) if t.replace("\r\n", "\n") == s => Some(frag.clone()),
            _ => None,
        };
        let np = match rich {
            Some(frag) => self.story_mut().insert_fragment(at, &frag),
            None => self.story_mut().insert_text(at, &s, Some(&fmt)),
        };
        self.sel = Sel::caret(np);
        self.scroll_to_caret = true;
    }

    pub fn copy(&mut self, cut: bool, ctx: &Context) {
        if self.sel.is_empty() {
            return;
        }
        let (a, b) = self.sel.ordered();
        let text = self.story().text_range(a, b);
        let frag = self.story().slice(a, b);
        self.clip = Some((text.clone(), frag));
        ctx.copy_text(text);
        if cut {
            self.delete_selection();
        }
    }

    pub fn enter(&mut self, soft: bool) {
        self.checkpoint(None);
        let had_sel = !self.sel.is_empty();
        let (a, _) = self.sel.ordered();
        let fmt = self.fmt_for_typing(a, had_sel);
        let at = self.delete_selection_raw();
        let at = self.story().clamp(at);
        if soft {
            let np = self.story_mut().insert_line_break(at, Some(&fmt));
            self.sel = Sel::caret(np);
            self.scroll_to_caret = true;
            return;
        }
        let (empty, is_list, style, len) = {
            let p = self.story().para(at.p);
            (p.is_empty(), p.list.is_some(), p.style.clone(), p.len())
        };
        if is_list && empty {
            // Enter on an empty list item ends the list
            let p = self.story_mut().para_mut(at.p);
            p.list = None;
            p.fmt.left = None;
            p.fmt.first = None;
            self.sel = Sel::caret(at);
            return;
        }
        let np = self.story_mut().split(at);
        if at.o == len {
            let next_style = self.doc.styles.get(&style).and_then(|s| s.next.clone()).filter(|n| *n != style);
            let p = self.story_mut().para_mut(np.p);
            match next_style {
                Some(next) => {
                    // e.g. Heading 1 -> Normal: the new paragraph starts clean
                    p.style = next;
                    p.fmt = ParaFmt::default();
                    p.runs.clear();
                }
                None => {
                    // the new paragraph continues with the current typing format (including pending toggles)
                    p.runs = vec![Run { text: String::new(), fmt: fmt.clone(), link: None, obj: None }];
                }
            }
        }
        self.sel = Sel::caret(np);
        self.pending = CharFmt::default();
        self.goal_x = None;
        self.scroll_to_caret = true;
    }

    pub fn backspace(&mut self, word: bool) {
        if !self.sel.is_empty() {
            self.delete_selection();
            return;
        }
        let at = self.story().clamp(self.sel.head);
        let (is_list, left) = {
            let p = self.story().para(at.p);
            (p.list.is_some(), p.fmt.left.is_some())
        };
        if at.o == 0 && (is_list || left) {
            self.checkpoint(None);
            let p = self.story_mut().para_mut(at.p);
            if p.list.is_some() {
                p.list = None;
                p.fmt.left = None;
                p.fmt.first = None;
            } else {
                p.fmt.left = None;
                p.fmt.first = None;
            }
            return;
        }
        if at.o == 0 && at.p == 0 {
            return;
        }
        self.checkpoint(Some((2, at.p)));
        let np = if word && at.o > 0 {
            let a = self.story().word_left(at);
            self.story_mut().delete(a, at)
        } else {
            self.story_mut().backspace(at)
        };
        self.sel = Sel::caret(np);
        self.pending = CharFmt::default();
        self.scroll_to_caret = true;
    }

    pub fn delete_fwd(&mut self, word: bool) {
        if !self.sel.is_empty() {
            self.delete_selection();
            return;
        }
        let at = self.story().clamp(self.sel.head);
        self.checkpoint(Some((3, at.p)));
        if word {
            let b = self.story().word_right(at);
            if b != at && self.story().same_container(at.p, b.p) {
                self.story_mut().delete(at, b);
            }
        } else {
            self.story_mut().delete_forward(at);
        }
        let np = self.story().clamp(at);
        self.sel = Sel::caret(np);
    }

    pub fn tab(&mut self, back: bool) {
        let at = self.story().clamp(self.sel.head);
        if let Some(tr) = self.story().table_at(at.p) {
            self.tab_in_table(tr, back);
            return;
        }
        let (a, b) = self.sel.ordered();
        let is_list = self.story().para(a.p).list.is_some();
        if is_list && (a.o == 0 || a != b || back) {
            self.change_indent(if back { -1 } else { 1 });
            return;
        }
        if back {
            return;
        }
        self.type_text("\t");
    }

    fn tab_in_table(&mut self, tr: TableRef, back: bool) {
        let t = self.story().tbl(tr.b);
        let (nr, nc) = (t.rows.len(), t.ncols());
        let mut order = vec![];
        for r in 0..nr {
            for c in 0..nc {
                if !t.rows[r].cells[c].covered {
                    order.push((r, c));
                }
            }
        }
        let idx = order.iter().position(|&x| x == (tr.r, tr.c)).unwrap_or(0);
        let target = if back {
            if idx == 0 {
                return;
            }
            order[idx - 1]
        } else if idx + 1 < order.len() {
            order[idx + 1]
        } else {
            self.checkpoint(None);
            let pos = self.story_mut().table_insert_row(tr, true);
            self.sel = Sel::caret(pos);
            self.scroll_to_caret = true;
            return;
        };
        let st = self.story();
        if let Some(p0) = st.cell_first_para(tr.b, target.0, target.1) {
            let cell = &st.tbl(tr.b).rows[target.0].cells[target.1];
            let p1 = p0 + cell.paras.len() - 1;
            self.sel = Sel { anchor: Pos::new(p0, 0), head: Pos::new(p1, st.plen(p1)) };
            self.pending = CharFmt::default();
            self.scroll_to_caret = true;
        }
    }

    // ------------------------------------------------------------ character formatting

    pub fn eff_chars(&self) -> Vec<RChar> {
        let story = self.story();
        let (a, b) = self.sel.ordered();
        let (a, b) = (story.clamp(a), story.clamp(b));
        let resolve = |para: &Paragraph, f: &CharFmt| {
            let mut r = self.sc.get(&para.style).0.clone();
            r.apply(f);
            r
        };
        if a == b {
            let (mut f, _) = story.fmt_at(a);
            f.merge(&self.pending);
            return vec![resolve(story.para(a.p), &f)];
        }
        let mut out = vec![];
        'outer: for p in a.p..=b.p {
            let para = story.para(p);
            let len = para.len();
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { len };
            let mut acc = 0;
            for r in &para.runs {
                let n = r.text.chars().count();
                if n > 0 && acc + n > s && acc < e {
                    out.push(resolve(para, &r.fmt));
                }
                acc += n;
            }
            if out.len() > 4000 {
                break 'outer;
            }
        }
        if out.is_empty() {
            out.push(self.sc.get(&story.para(a.p).style).0.clone());
        }
        out
    }

    pub fn apply_chars(&mut self, f: &dyn Fn(&mut CharFmt)) {
        let (a, b) = self.sel.ordered();
        if a == b {
            f(&mut self.pending);
            if self.story().plen(a.p) == 0 {
                self.checkpoint(None);
                self.story_mut().modify_chars(a, b, &|r| f(&mut r.fmt));
            }
            return;
        }
        self.checkpoint(None);
        self.story_mut().modify_chars(a, b, &|r| f(&mut r.fmt));
    }

    fn toggle(&mut self, get: fn(&RChar) -> bool, set: fn(&mut CharFmt, bool)) {
        let all = self.eff_chars().iter().all(|r| get(r));
        let v = !all;
        self.apply_chars(&move |f| set(f, v));
    }

    pub fn toggle_bold(&mut self) {
        self.toggle(|r| r.bold, |f, v| f.bold = Some(v));
    }
    pub fn toggle_italic(&mut self) {
        self.toggle(|r| r.italic, |f, v| f.italic = Some(v));
    }
    pub fn toggle_underline(&mut self) {
        self.toggle(|r| r.underline, |f, v| f.underline = Some(v));
    }
    pub fn toggle_strike(&mut self) {
        self.toggle(|r| r.strike, |f, v| f.strike = Some(v));
    }
    pub fn toggle_small_caps(&mut self) {
        self.toggle(|r| r.small_caps, |f, v| f.small_caps = Some(v));
    }
    pub fn toggle_all_caps(&mut self) {
        self.toggle(|r| r.all_caps, |f, v| f.all_caps = Some(v));
    }

    pub fn toggle_vert(&mut self, v: VertAlign) {
        let all = self.eff_chars().iter().all(|r| r.vert == v);
        let nv = if all { VertAlign::Baseline } else { v };
        self.apply_chars(&move |f| f.vert = Some(nv));
    }

    pub fn set_font(&mut self, name: &str) {
        let n = name.to_string();
        // fetch Google fonts that are not installed yet
        if !self.fonts.has_family(name) && self.fonts.catalog_entry(name).is_some() {
            self.fonts.request_google(name);
        }
        self.apply_chars(&move |f| f.font = Some(n.clone()));
    }

    pub fn set_size(&mut self, pt: f32) {
        let pt = pt.clamp(1.0, 1638.0);
        self.apply_chars(&move |f| f.size = Some(pt));
    }

    pub fn size_step(&mut self, dir: i32) {
        let cur = self.eff_chars().first().map(|r| r.size).unwrap_or(11.0);
        let next = if dir > 0 {
            SIZES.iter().copied().find(|&s| s > cur + 0.01).unwrap_or(cur + 10.0)
        } else {
            SIZES.iter().rev().copied().find(|&s| s < cur - 0.01).unwrap_or((cur - 1.0).max(1.0))
        };
        self.set_size(next);
    }

    pub fn set_color(&mut self, c: Rgb) {
        self.last_color = c;
        self.apply_chars(&move |f| f.color = Some(c));
    }

    pub fn set_highlight(&mut self, c: Option<Rgb>) {
        if let Some(c) = c {
            self.last_highlight = c;
        }
        self.apply_chars(&move |f| f.highlight = Some(c));
    }

    pub fn clear_char_formatting(&mut self) {
        let (a, b) = self.sel.ordered();
        self.pending = CharFmt::default();
        if a == b {
            return;
        }
        self.checkpoint(None);
        self.story_mut().modify_chars(a, b, &|r| r.fmt = CharFmt::default());
    }

    pub fn clear_all_formatting(&mut self) {
        let (a, b) = self.sel.ordered();
        self.checkpoint(None);
        self.pending = CharFmt::default();
        self.story_mut().modify_chars(a, b, &|r| r.fmt = CharFmt::default());
        self.story_mut().modify_paras(a.p, b.p, &|p| {
            p.style = "Normal".into();
            p.fmt = ParaFmt::default();
            p.list = None;
        });
    }

    pub fn change_case(&mut self, mode: CaseMode) {
        let (a, b) = self.sel.ordered();
        if a == b {
            return;
        }
        self.checkpoint(None);
        let prev_letter = Cell::new(false);
        let map1 = |c: char| -> char {
            let mut u = c.to_uppercase();
            match (u.next(), u.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        };
        let low1 = |c: char| -> char {
            let mut u = c.to_lowercase();
            match (u.next(), u.next()) {
                (Some(x), None) => x,
                _ => c,
            }
        };
        self.story_mut().modify_chars(a, b, &|r| {
            if r.obj.is_some() {
                return;
            }
            let t: String = r
                .text
                .chars()
                .map(|c| {
                    let out = match mode {
                        CaseMode::Upper => map1(c),
                        CaseMode::Lower => low1(c),
                        CaseMode::Title => {
                            if c.is_alphabetic() && !prev_letter.get() {
                                map1(c)
                            } else if c.is_alphabetic() {
                                low1(c)
                            } else {
                                c
                            }
                        }
                        CaseMode::Sentence => {
                            if c.is_alphabetic() && !prev_letter.get() {
                                map1(c)
                            } else {
                                low1(c)
                            }
                        }
                        CaseMode::Toggle => {
                            if c.is_uppercase() {
                                low1(c)
                            } else {
                                map1(c)
                            }
                        }
                    };
                    if mode == CaseMode::Sentence {
                        if c == '.' || c == '!' || c == '?' {
                            prev_letter.set(false);
                        } else if c.is_alphabetic() {
                            prev_letter.set(true);
                        }
                    } else {
                        prev_letter.set(c.is_alphabetic() || c == '\'');
                    }
                    out
                })
                .collect();
            r.text = t;
        });
    }

    pub fn apply_format_painter(&mut self) {
        if let Some(f) = self.format_painter.take() {
            let (a, b) = self.sel.ordered();
            if a == b {
                return;
            }
            self.checkpoint(None);
            self.story_mut().modify_chars(a, b, &|r| r.fmt = f.clone());
        }
    }

    pub fn start_format_painter(&mut self) {
        let (a, _) = self.sel.ordered();
        let (f, _) = self.story().fmt_at(a);
        self.format_painter = Some(f);
        self.say("Format Painter: select the text to format (Esc to cancel)");
    }

    // ------------------------------------------------------------ paragraph formatting

    pub fn apply_paras(&mut self, f: &dyn Fn(&mut Paragraph)) {
        let (a, b) = self.sel.ordered();
        self.checkpoint(None);
        self.story_mut().modify_paras(a.p, b.p, f);
    }

    pub fn set_align(&mut self, al: Align) {
        self.apply_paras(&move |p| p.fmt.align = Some(al));
    }

    pub fn set_line_spacing(&mut self, m: f32) {
        self.apply_paras(&move |p| p.fmt.line = Some(LineSpacing::Multiple(m)));
    }

    pub fn set_para_spacing(&mut self, before: Option<f32>, after: Option<f32>) {
        self.apply_paras(&move |p| {
            if let Some(b) = before {
                p.fmt.before = Some(b);
            }
            if let Some(a) = after {
                p.fmt.after = Some(a);
            }
        });
    }

    pub fn change_indent(&mut self, dir: i32) {
        let (a, b) = self.sel.ordered();
        self.checkpoint(None);
        for pi in a.p..=b.p.min(self.story().len() - 1) {
            let (list, style_left, direct) = {
                let p = self.story().para(pi);
                (p.list, self.sc.get(&p.style).1.left, p.fmt.left)
            };
            let p = self.story_mut().para_mut(pi);
            if let Some(mut li) = list {
                li.level = (li.level as i32 + dir).clamp(0, 8) as u8;
                p.list = Some(li);
            } else {
                let cur = direct.unwrap_or(style_left);
                p.fmt.left = Some((cur + 36.0 * dir as f32).max(0.0));
            }
        }
    }

    pub fn apply_style(&mut self, name: &str) {
        if self.doc.styles.get(name).is_none() {
            return;
        }
        let (a, b) = self.sel.ordered();
        self.checkpoint(None);
        let name = name.to_string();
        let sty = self.doc.styles.get(&name).cloned();
        for pi in a.p..=b.p.min(self.story().len() - 1) {
            let p = self.story_mut().para_mut(pi);
            p.style = name.clone();
            p.fmt = ParaFmt::default();
            // direct character formatting that covers most of the paragraph is dropped (like Word)
            let total = p.len().max(1);
            macro_rules! strip {
                ($f:ident) => {{
                    let covered: usize = p.runs.iter().filter(|r| r.fmt.$f.is_some()).map(|r| r.text.chars().count()).sum();
                    if covered * 2 > total {
                        for r in p.runs.iter_mut() {
                            r.fmt.$f = None;
                        }
                    }
                }};
            }
            if let Some(s) = &sty {
                if s.ch.font.is_some() {
                    strip!(font);
                }
                if s.ch.size.is_some() {
                    strip!(size);
                }
                if s.ch.color.is_some() {
                    strip!(color);
                }
                if s.ch.bold.is_some() {
                    strip!(bold);
                }
                if s.ch.italic.is_some() {
                    strip!(italic);
                }
            }
            p.normalize();
        }
        self.pending = CharFmt::default();
    }

    pub fn update_style_from_selection(&mut self) {
        let (a, _) = self.sel.ordered();
        let name = self.story().para(a.p).style.clone();
        let Some(mut s) = self.doc.styles.get(&name).cloned() else { return };
        let chars = self.eff_chars();
        let Some(r) = chars.first() else { return };
        s.ch = CharFmt { font: Some(r.font.to_string()), size: Some(r.size), bold: Some(r.bold), italic: Some(r.italic), underline: Some(r.underline), color: Some(r.color), ..Default::default() };
        let p = self.story().para(a.p);
        s.para = p.fmt.clone();
        self.checkpoint(None);
        self.doc.styles.upsert(s);
        self.doc.touch_styles();
        // direct formatting that now matches the style is redundant; clear it on this paragraph
        let (a, b) = self.sel.ordered();
        self.story_mut().modify_paras(a.p, b.p, &|p| p.fmt = ParaFmt::default());
        self.say(format!("Style \"{name}\" updated to match the selection"));
    }

    // ------------------------------------------------------------ lists

    pub fn toggle_list(&mut self, kind: ListKind) {
        let (a, b) = self.sel.ordered();
        let all = (a.p..=b.p).all(|p| self.story().para(p).list.map(|l| l.kind == kind).unwrap_or(false));
        self.checkpoint(None);
        self.set_list_for(a.p, b.p, if all { None } else { Some(kind) });
    }

    pub fn set_list_for(&mut self, p0: usize, p1: usize, kind: Option<ListKind>) {
        match kind {
            None => {
                self.story_mut().modify_paras(p0, p1, &|p| {
                    p.list = None;
                    p.fmt.left = None;
                    p.fmt.first = None;
                });
            }
            Some(k) => {
                // continue the list directly above when it has the same kind
                let id = if p0 > 0 {
                    match self.story().para(p0 - 1).list {
                        Some(l) if l.kind == k => l.id,
                        _ => self.new_list_id(),
                    }
                } else {
                    self.new_list_id()
                };
                self.story_mut().modify_paras(p0, p1, &move |p| {
                    let level = p.list.map(|l| l.level).unwrap_or(0);
                    p.list = Some(ListInfo { id, level, kind: k });
                });
            }
        }
    }

    fn new_list_id(&mut self) -> u32 {
        let mut max = self.next_list_id;
        for i in 0..self.doc.body.len() {
            if let Some(l) = self.doc.body.para(i).list {
                max = max.max(l.id + 1);
            }
        }
        self.next_list_id = max + 1;
        max
    }

    pub fn restart_numbering(&mut self) {
        let (a, b) = self.sel.ordered();
        let id = self.new_list_id();
        self.checkpoint(None);
        self.story_mut().modify_paras(a.p, b.p, &move |p| {
            if let Some(mut l) = p.list {
                l.id = id;
                p.list = Some(l);
            }
        });
    }

    // ------------------------------------------------------------ insertion

    pub fn insert_page_break(&mut self) {
        self.checkpoint(None);
        let at = self.delete_selection_raw();
        let np = self.story_mut().insert_page_break(at);
        self.sel = Sel::caret(np);
        self.scroll_to_caret = true;
    }

    pub fn insert_table(&mut self, rows: usize, cols: usize) {
        if self.kind != StoryKind::Body {
            self.say("Tables can only be inserted in the document body");
            return;
        }
        self.checkpoint(None);
        let at = self.delete_selection_raw();
        let width = self.doc.page.content_w();
        match self.story_mut().insert_table(at, rows, cols, width) {
            Some(p) => {
                self.sel = Sel::caret(p);
                self.scroll_to_caret = true;
            }
            None => self.say("Nested tables are not supported"),
        }
    }

    pub fn insert_text_plain(&mut self, t: &str) {
        self.type_text(t);
    }

    pub fn insert_symbol(&mut self, c: char) {
        let s = c.to_string();
        let was = self.settings.smart_quotes;
        self.settings.smart_quotes = false;
        self.type_text(&s);
        self.settings.smart_quotes = was;
    }

    pub fn insert_datetime(&mut self, fmt: &str) {
        let now = chrono::Local::now();
        let s = now.format(fmt).to_string();
        self.type_text(&s);
    }

    pub fn insert_hr(&mut self) {
        self.checkpoint(None);
        let at = self.delete_selection_raw();
        let np = self.story_mut().split(at);
        let p = self.story_mut().para_mut(at.p);
        p.fmt.borders = Some(ParaBorders { bottom: Some(Border { w: 1.0, color: [0, 0, 0] }), ..Default::default() });
        p.fmt.after = Some(6.0);
        self.sel = Sel::caret(np);
    }

    pub fn insert_image_path(&mut self, path: &std::path::Path) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.say(format!("Could not read image: {e}"));
                return;
            }
        };
        self.insert_image_bytes(bytes);
    }

    pub fn insert_image_bytes(&mut self, bytes: Vec<u8>) {
        let data = match ImageData::from_bytes(bytes) {
            Ok(d) => d,
            Err(e) => {
                self.say(format!("Could not insert picture: {e}"));
                return;
            }
        };
        let max_w = self.doc.page.content_w();
        let max_h = self.doc.page.content_h() * 0.9;
        let (mut w, mut h) = (data.px_w as f32 * 0.75, data.px_h as f32 * 0.75);
        let k = (max_w / w).min(max_h / h).min(1.0);
        w *= k;
        h *= k;
        self.checkpoint(None);
        let at = self.delete_selection_raw();
        let at = self.story().clamp(at);
        let np = self.story_mut().insert_obj(at, Obj::Image(ImageObj { data, w, h }), None);
        self.sel = Sel::caret(np);
        self.scroll_to_caret = true;
    }

    pub fn set_image_size(&mut self, para: usize, off: usize, w: f32, h: f32) {
        if para >= self.story().len() {
            return;
        }
        let p = self.story_mut().para_mut(para);
        let mut acc = 0;
        for r in p.runs.iter_mut() {
            let n = r.text.chars().count();
            if off >= acc && off < acc + n {
                if let Some(Obj::Image(img)) = &mut r.obj {
                    img.w = w;
                    img.h = h;
                }
                break;
            }
            acc += n;
        }
    }

    pub fn open_link_dialog(&mut self) {
        let (a, b) = self.sel.ordered();
        self.dlg.link_open = true;
        self.dlg.link_text = if a != b && a.p == b.p { self.story().text_range(a, b) } else { String::new() };
        let (_, link) = self.story().fmt_at(a);
        self.dlg.link_url = link.unwrap_or_default();
    }

    pub fn apply_link(&mut self, text: &str, url: &str) {
        self.checkpoint(None);
        let (a, b) = self.sel.ordered();
        if url.trim().is_empty() {
            // remove link
            self.story_mut().modify_chars(a, b, &|r| r.link = None);
            return;
        }
        let url = url.trim().to_string();
        let url = if url.contains("://") || url.starts_with("mailto:") { url } else { format!("https://{url}") };
        if a != b && a.p == b.p {
            let u = url.clone();
            self.story_mut().modify_chars(a, b, &move |r| {
                if r.obj.is_none() {
                    r.link = Some(u.clone());
                }
            });
            self.sel = Sel::caret(b);
        } else {
            let t = if text.trim().is_empty() { url.clone() } else { text.to_string() };
            let at = self.delete_selection_raw();
            let n = t.chars().count();
            self.story_mut().para_mut(at.p).insert(at.o, &t, None, Some(Some(url)));
            self.sel = Sel::caret(Pos::new(at.p, at.o + n));
        }
    }

    /// Insert a page number field into the header or footer story.
    pub fn insert_page_number(&mut self, footer: bool, align: Align, with_total: bool) {
        self.checkpoint(None);
        let kind = if footer { StoryKind::Footer } else { StoryKind::Header };
        let kinds: Vec<StoryKind> = if self.doc.page.diff_first { vec![kind, if footer { StoryKind::FirstFooter } else { StoryKind::FirstHeader }] } else { vec![kind] };
        for k in kinds {
            let st = self.doc.story_mut(k);
            let last = st.len() - 1;
            let n = st.plen(last);
            // replace content of the last paragraph with the page number text
            st.para_mut(last).delete(0, n);
            let p = st.para_mut(last);
            p.fmt.align = Some(align);
            let mut at = 0;
            if with_total {
                p.insert(at, "Page ", None, None);
                at += 5;
            }
            p.insert_obj(at, Obj::Field(Field::Page), None);
            at += 1;
            if with_total {
                p.insert(at, " of ", None, None);
                at += 4;
                p.insert_obj(at, Obj::Field(Field::NumPages), None);
            }
        }
        self.pag_key = None;
        self.say(if footer { "Page number added to the footer" } else { "Page number added to the header" });
    }

    pub fn toggle_header_footer(&mut self, footer: bool) {
        self.ensure_layout();
        let page = self.pag.as_ref().and_then(|p| p.pages.get(self.active_page)).map(|p| (p.header_kind, p.footer_kind));
        let (hk, fk) = page.unwrap_or((StoryKind::Header, StoryKind::Footer));
        let k = if footer { fk } else { hk };
        if self.kind == k {
            self.kind = StoryKind::Body;
            self.sel = Sel::caret(Pos::default());
        } else {
            self.kind = k;
            self.sel = Sel::caret(Pos::new(0, self.doc.story(k).plen(0)));
        }
        self.scroll_to_caret = true;
    }

    /// Insert a static table of contents built from the heading styles.
    pub fn insert_toc(&mut self) {
        self.ensure_layout();
        let mut entries: Vec<(u8, String, usize)> = vec![];
        {
            let pag = self.pag.as_ref().unwrap();
            let st = &self.doc.body;
            for i in 0..st.len() {
                let p = st.para(i);
                if p.style.starts_with("TOC") {
                    continue;
                }
                if let Some(l) = self.doc.styles.outline_level(&p.style) {
                    let t = p.plain();
                    if l <= 3 && !t.trim().is_empty() {
                        entries.push((l, t, pag.page_of(Pos::new(i, 0)) + 1));
                    }
                }
            }
        }
        if entries.is_empty() {
            self.say("No headings found. Apply Heading 1–3 styles first.");
            return;
        }
        self.checkpoint(None);
        let cw = self.doc.page.content_w();
        for lvl in 1..=3u8 {
            let name = format!("TOC {lvl}");
            let st = Style {
                name: name.clone(),
                based_on: Some("Normal".into()),
                next: None,
                ch: CharFmt::default(),
                para: ParaFmt { left: Some(11.0 * (lvl as f32 - 1.0)), after: Some(5.0), tabs: Some(vec![TabStop { pos: cw, kind: TabKind::Right, leader: Leader::Dots }]), ..Default::default() },
                outline: None,
            };
            self.doc.styles.upsert(st);
        }
        self.doc.touch_styles();
        let at = self.delete_selection_raw();
        let mut paras: Vec<Paragraph> = vec![];
        let mut title = Paragraph::styled("Normal", "Contents");
        title.runs[0].fmt = CharFmt { bold: Some(true), size: Some(16.0), color: Some([0x2F, 0x54, 0x96]), ..Default::default() };
        paras.push(title);
        for (l, t, pg) in entries {
            paras.push(Paragraph::styled(&format!("TOC {l}"), &format!("{t}\t{pg}")));
        }
        paras.push(Paragraph::new());
        let np = self.story_mut().insert_fragment(at, &paras);
        self.sel = Sel::caret(np);
    }

    // ------------------------------------------------------------ tables

    pub fn cell_selection(&self) -> Option<(usize, usize, usize, usize, usize)> {
        let (a, b) = self.sel.ordered();
        if self.kind != StoryKind::Body {
            return None;
        }
        let st = self.story();
        if a.p >= st.len() || b.p >= st.len() {
            return None;
        }
        st.cell_range(a.p, b.p)
    }

    pub fn in_table(&self) -> bool {
        self.kind == StoryKind::Body && self.sel.head.p < self.story().len() && self.story().table_at(self.sel.head.p).is_some()
    }

    pub fn table_cmd(&mut self, cmd: TableCmd) {
        let head = self.story().clamp(self.sel.head);
        let Some(tr) = self.story().table_at(head.p) else { return };
        let range = self.cell_selection();
        self.checkpoint(None);
        let b = tr.b;
        match cmd {
            TableCmd::RowAbove => {
                let p = self.story_mut().table_insert_row(tr, false);
                self.set_caret(p);
            }
            TableCmd::RowBelow => {
                let p = self.story_mut().table_insert_row(tr, true);
                self.set_caret(p);
            }
            TableCmd::ColLeft => {
                let p = self.story_mut().table_insert_col(tr, false);
                self.set_caret(p);
            }
            TableCmd::ColRight => {
                let p = self.story_mut().table_insert_col(tr, true);
                self.set_caret(p);
            }
            TableCmd::DelRow => {
                let p = self.story_mut().table_delete_row(tr);
                self.set_caret(p);
            }
            TableCmd::DelCol => {
                let p = self.story_mut().table_delete_col(tr);
                self.set_caret(p);
            }
            TableCmd::DelTable => {
                let p = self.story_mut().table_delete(b);
                self.set_caret(p);
            }
            TableCmd::Merge => {
                if let Some((blk, r0, c0, r1, c1)) = range {
                    if (r1, c1) != (r0, c0) {
                        let p = self.story_mut().table_merge(blk, r0, c0, r1, c1);
                        self.set_caret(p);
                    }
                }
            }
            TableCmd::Split => {
                let p = self.story_mut().table_split(b, tr.r, tr.c);
                self.set_caret(p);
            }
            TableCmd::Shade(c) => {
                let (blk, r0, c0, r1, c1) = range.unwrap_or((b, tr.r, tr.c, tr.r, tr.c));
                self.story_mut().table_set_shade(blk, r0, c0, r1, c1, c);
            }
            TableCmd::VAlign(v) => {
                let (blk, r0, c0, r1, c1) = range.unwrap_or((b, tr.r, tr.c, tr.r, tr.c));
                self.story_mut().table_set_valign(blk, r0, c0, r1, c1, v);
            }
            TableCmd::Borders(bd) => self.story_mut().tbl_mut(b).borders = bd,
            TableCmd::Align(a) => self.story_mut().tbl_mut(b).align = a,
            TableCmd::HeaderRow(on) => self.story_mut().tbl_mut(b).rows[0].header = on,
            TableCmd::EqualCols => {
                let cw = self.doc.page.content_w();
                let t = self.story_mut().tbl_mut(b);
                let total = t.width().min(cw).max(36.0);
                let n = t.ncols() as f32;
                for w in t.cols.iter_mut() {
                    *w = total / n;
                }
            }
            TableCmd::RowHeight(h) => {
                let t = self.story_mut().tbl_mut(b);
                t.rows[tr.r].height = if h > 0.0 { Some(h) } else { None };
            }
        }
    }

    pub fn table_info(&self) -> Option<(TableRef, Table)> {
        let head = self.story().clamp(self.sel.head);
        let tr = self.story().table_at(head.p)?;
        Some((tr, self.story().tbl(tr.b).clone()))
    }

    // ------------------------------------------------------------ find / replace

    pub fn open_find(&mut self, replace: bool) {
        self.dlg.find_open = true;
        self.dlg.find.replace_mode = replace;
        self.dlg.find.focus = true;
        let (a, b) = self.sel.ordered();
        if a != b && a.p == b.p {
            self.dlg.find.query = self.story().text_range(a, b);
        }
        self.find_recompute();
    }

    fn build_regex(&self) -> Option<regex::Regex> {
        let f = &self.dlg.find;
        if f.query.is_empty() {
            return None;
        }
        let mut pat = if f.use_regex { f.query.clone() } else { regex::escape(&f.query) };
        if f.whole_word {
            pat = format!(r"\b(?:{pat})\b");
        }
        regex::RegexBuilder::new(&pat).case_insensitive(!f.match_case).build().ok()
    }

    pub fn find_recompute(&mut self) {
        let re = self.build_regex();
        let mut out = vec![];
        if let Some(re) = &re {
            let st = self.doc.story(StoryKind::Body);
            for p in 0..st.len() {
                let text = st.para(p).text();
                let mut char_pos = 0usize;
                let mut byte_pos = 0usize;
                for m in re.find_iter(&text) {
                    if m.start() == m.end() {
                        continue;
                    }
                    char_pos += text[byte_pos..m.start()].chars().count();
                    let len = text[m.start()..m.end()].chars().count();
                    out.push((Pos::new(p, char_pos), Pos::new(p, char_pos + len)));
                    char_pos += len;
                    byte_pos = m.end();
                    if out.len() > 20000 {
                        break;
                    }
                }
            }
        }
        self.dlg.find.invalid_regex = re.is_none() && !self.dlg.find.query.is_empty();
        self.dlg.find.matches = out;
        self.dlg.find.key = self.doc.body.ver;
        if self.dlg.find.cur.map(|c| c >= self.dlg.find.matches.len()).unwrap_or(false) {
            self.dlg.find.cur = None;
        }
    }

    pub fn find_next(&mut self, forward: bool) {
        self.find_recompute();
        let n = self.dlg.find.matches.len();
        if n == 0 {
            self.dlg.find.status = "No results".into();
            return;
        }
        self.kind = StoryKind::Body;
        let (a, b) = self.sel.ordered();
        let idx = if forward {
            self.dlg.find.matches.iter().position(|(s, _)| *s >= b && (*s > a || a == b || self.dlg.find.cur.is_some())).filter(|&i| self.dlg.find.matches[i].0 >= b || a == b).unwrap_or(0)
        } else {
            self.dlg.find.matches.iter().rposition(|(s, _)| *s < a).unwrap_or(n - 1)
        };
        let (s, e) = self.dlg.find.matches[idx];
        self.dlg.find.cur = Some(idx);
        self.sel = Sel { anchor: s, head: e };
        self.dlg.find.status = format!("{} of {}", idx + 1, n);
        self.scroll_to_caret = true;
    }

    fn replacement_for(&self, matched: &str) -> String {
        let f = &self.dlg.find;
        if f.use_regex {
            if let Some(re) = self.build_regex() {
                return re.replace(matched, f.replace.as_str()).to_string();
            }
        }
        f.replace.clone()
    }

    pub fn replace_current(&mut self) {
        self.find_recompute();
        let (a, b) = self.sel.ordered();
        let is_match = self.dlg.find.matches.iter().any(|&(s, e)| s == a && e == b);
        if !is_match {
            self.find_next(true);
            return;
        }
        let matched = self.story().text_range(a, b);
        let rep = self.replacement_for(&matched);
        self.checkpoint(None);
        let (fmt, _) = self.story().fmt_at(Pos::new(a.p, a.o + 1));
        let at = self.story_mut().delete(a, b);
        let np = self.story_mut().insert_text(at, &rep, Some(&fmt));
        self.sel = Sel::caret(np);
        self.find_next(true);
    }

    pub fn replace_all(&mut self) {
        self.find_recompute();
        let matches = self.dlg.find.matches.clone();
        if matches.is_empty() {
            self.dlg.find.status = "No results".into();
            return;
        }
        self.checkpoint(None);
        self.kind = StoryKind::Body;
        for &(a, b) in matches.iter().rev() {
            let matched = self.story().text_range(a, b);
            let rep = self.replacement_for(&matched);
            let (fmt, _) = self.story().fmt_at(Pos::new(a.p, a.o + 1));
            let at = self.story_mut().delete(a, b);
            self.story_mut().insert_text(at, &rep, Some(&fmt));
        }
        self.dlg.find.status = format!("Replaced {} occurrence{}", matches.len(), if matches.len() == 1 { "" } else { "s" });
        self.sel = Sel::caret(Pos::default());
        self.find_recompute();
    }

    // ------------------------------------------------------------ spelling (hook)

    pub fn spell_spans_for_view(&self, _pag: &crate::layout::Pagination) -> Vec<(usize, usize, usize)> {
        vec![]
    }
}

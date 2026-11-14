//! A `Story` is a sequence of blocks (paragraphs and tables) with a flat paragraph
//! index, so editing code can address any paragraph (including those inside table
//! cells) as `Pos { p, o }`. Blocks are `Arc`-shared so undo snapshots are cheap.

use std::collections::BTreeSet;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;

use crate::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loc {
    Body(usize),
    Cell { b: usize, r: usize, c: usize, p: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableRef {
    pub b: usize,
    pub r: usize,
    pub c: usize,
}

#[derive(Clone)]
pub struct Story {
    pub blocks: Vec<Arc<Block>>,
    pub locs: Vec<Loc>,
    /// Changes whenever anything in the story changes.
    pub ver: u64,
}

impl Default for Story {
    fn default() -> Self {
        Story::new()
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == OBJ_CHAR
}

fn char_class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if is_word_char(c) {
        1
    } else {
        2
    }
}

impl Story {
    pub fn new() -> Story {
        let mut s = Story { blocks: vec![Arc::new(Block::Para(Paragraph::new()))], locs: vec![], ver: next_rev() };
        s.reindex();
        s
    }

    pub fn from_blocks(blocks: Vec<Block>) -> Story {
        let mut s = Story { blocks: blocks.into_iter().map(Arc::new).collect(), locs: vec![], ver: next_rev() };
        s.normalize_blocks();
        s.reindex();
        s
    }

    /// A table must be followed by a paragraph, and the story must be non-empty.
    pub fn normalize_blocks(&mut self) {
        if self.blocks.is_empty() {
            self.blocks.push(Arc::new(Block::Para(Paragraph::new())));
        }
        let mut i = 0;
        while i < self.blocks.len() {
            if matches!(&*self.blocks[i], Block::Table(_)) {
                let next_is_table_or_end = i + 1 >= self.blocks.len() || matches!(&*self.blocks[i + 1], Block::Table(_));
                if next_is_table_or_end {
                    self.blocks.insert(i + 1, Arc::new(Block::Para(Paragraph::new())));
                }
            }
            i += 1;
        }
    }

    pub fn reindex(&mut self) {
        self.ver = next_rev();
        self.locs.clear();
        for (b, blk) in self.blocks.iter().enumerate() {
            match &**blk {
                Block::Para(_) => self.locs.push(Loc::Body(b)),
                Block::Table(t) => {
                    for (r, row) in t.rows.iter().enumerate() {
                        for (c, cell) in row.cells.iter().enumerate() {
                            if cell.covered {
                                continue;
                            }
                            for p in 0..cell.paras.len() {
                                self.locs.push(Loc::Cell { b, r, c, p });
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.locs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.locs.is_empty()
    }
    pub fn loc(&self, i: usize) -> Loc {
        self.locs[i]
    }

    pub fn para(&self, i: usize) -> &Paragraph {
        match self.locs[i] {
            Loc::Body(b) => match &*self.blocks[b] {
                Block::Para(p) => p,
                _ => unreachable!(),
            },
            Loc::Cell { b, r, c, p } => match &*self.blocks[b] {
                Block::Table(t) => &t.rows[r].cells[c].paras[p],
                _ => unreachable!(),
            },
        }
    }

    pub fn para_mut(&mut self, i: usize) -> &mut Paragraph {
        self.ver = next_rev();
        let p = match self.locs[i] {
            Loc::Body(b) => match Arc::make_mut(&mut self.blocks[b]) {
                Block::Para(p) => p,
                _ => unreachable!(),
            },
            Loc::Cell { b, r, c, p } => match Arc::make_mut(&mut self.blocks[b]) {
                Block::Table(t) => &mut t.rows[r].cells[c].paras[p],
                _ => unreachable!(),
            },
        };
        p.rev = next_rev();
        p
    }

    pub fn tbl(&self, b: usize) -> &Table {
        match &*self.blocks[b] {
            Block::Table(t) => t,
            _ => panic!("not a table"),
        }
    }

    pub fn tbl_mut(&mut self, b: usize) -> &mut Table {
        self.ver = next_rev();
        match Arc::make_mut(&mut self.blocks[b]) {
            Block::Table(t) => t,
            _ => panic!("not a table"),
        }
    }

    pub fn plen(&self, i: usize) -> usize {
        self.para(i).len()
    }

    pub fn end_pos(&self) -> Pos {
        let p = self.len() - 1;
        Pos { p, o: self.plen(p) }
    }

    pub fn clamp(&self, pos: Pos) -> Pos {
        let p = pos.p.min(self.len() - 1);
        Pos { p, o: pos.o.min(self.plen(p)) }
    }

    pub fn same_container(&self, a: usize, b: usize) -> bool {
        match (self.locs[a], self.locs[b]) {
            (Loc::Body(_), Loc::Body(_)) => true,
            (Loc::Cell { b: b1, r: r1, c: c1, .. }, Loc::Cell { b: b2, r: r2, c: c2, .. }) => (b1, r1, c1) == (b2, r2, c2),
            _ => false,
        }
    }

    pub fn table_at(&self, p: usize) -> Option<TableRef> {
        match self.locs.get(p)? {
            Loc::Cell { b, r, c, .. } => Some(TableRef { b: *b, r: *r, c: *c }),
            _ => None,
        }
    }

    pub fn cell_first_para(&self, b: usize, r: usize, c: usize) -> Option<usize> {
        self.locs.iter().position(|l| matches!(l, Loc::Cell { b: bb, r: rr, c: cc, p: 0 } if *bb == b && *rr == r && *cc == c))
    }

    /// All flat paragraph indices belonging to a table block.
    pub fn table_paras(&self, b: usize) -> Vec<usize> {
        self.locs.iter().enumerate().filter(|(_, l)| matches!(l, Loc::Cell { b: bb, .. } if *bb == b)).map(|(i, _)| i).collect()
    }

    // ------------------------------------------------------------ text editing

    pub fn insert_text(&mut self, mut at: Pos, text: &str, fmt: Option<&CharFmt>) -> Pos {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut first = true;
        for line in text.split('\n') {
            if !first {
                at = self.split(at);
            }
            first = false;
            let clean: String = line.chars().filter(|c| (!c.is_control() || *c == '\t') && *c != OBJ_CHAR).collect();
            if !clean.is_empty() {
                let n = clean.chars().count();
                self.para_mut(at.p).insert(at.o, &clean, fmt, None);
                at.o += n;
            }
        }
        at
    }

    /// Insert a soft line break (Shift+Enter) inside the paragraph.
    pub fn insert_line_break(&mut self, at: Pos, fmt: Option<&CharFmt>) -> Pos {
        self.para_mut(at.p).insert(at.o, "\n", fmt, None);
        Pos { p: at.p, o: at.o + 1 }
    }

    pub fn insert_obj(&mut self, at: Pos, obj: Obj, fmt: Option<&CharFmt>) -> Pos {
        self.para_mut(at.p).insert_obj(at.o, obj, fmt);
        Pos { p: at.p, o: at.o + 1 }
    }

    pub fn insert_page_break(&mut self, at: Pos) -> Pos {
        self.para_mut(at.p).insert(at.o, &PAGE_BREAK.to_string(), None, None);
        let p = self.split(Pos { p: at.p, o: at.o + 1 });
        p
    }

    pub fn split(&mut self, at: Pos) -> Pos {
        let right = self.para_mut(at.p).split_off(at.o);
        match self.locs[at.p] {
            Loc::Body(b) => self.blocks.insert(b + 1, Arc::new(Block::Para(right))),
            Loc::Cell { b, r, c, p } => {
                self.tbl_mut(b).rows[r].cells[c].paras.insert(p + 1, right);
            }
        }
        self.reindex();
        Pos { p: at.p + 1, o: 0 }
    }

    pub fn delete(&mut self, a: Pos, b: Pos) -> Pos {
        if a >= b {
            return a;
        }
        if a.p == b.p {
            self.para_mut(a.p).delete(a.o, b.o);
            return a;
        }
        let alen = self.plen(a.p);
        let a_empty = alen == 0;
        if self.same_container(a.p, b.p) {
            let (bstyle, bfmt, blist) = {
                let pb = self.para(b.p);
                (pb.style.clone(), pb.fmt.clone(), pb.list)
            };
            self.para_mut(a.p).delete(a.o, alen);
            self.para_mut(b.p).delete(0, b.o);
            let tail = self.para(b.p).clone();
            {
                let pa = self.para_mut(a.p);
                pa.append(tail);
                if a_empty {
                    pa.style = bstyle;
                    pa.fmt = bfmt;
                    pa.list = blist;
                }
            }
            match (self.locs[a.p], self.locs[b.p]) {
                (Loc::Body(ba), Loc::Body(bb)) => {
                    self.blocks.drain(ba + 1..=bb);
                }
                (Loc::Cell { b: blk, r, c, p: pa }, Loc::Cell { p: pb, .. }) => {
                    self.tbl_mut(blk).rows[r].cells[c].paras.drain(pa + 1..=pb);
                }
                _ => {}
            }
            self.normalize_blocks();
            self.reindex();
            return a;
        }
        // Different containers: trim the ends, drop or clear what lies between.
        self.para_mut(a.p).delete(a.o, alen);
        let blen_head = b.o;
        self.para_mut(b.p).delete(0, blen_head);
        let mut remove: BTreeSet<usize> = BTreeSet::new();
        let mut tables: BTreeSet<usize> = BTreeSet::new();
        for i in a.p + 1..b.p {
            match self.locs[i] {
                Loc::Body(blk) => {
                    remove.insert(blk);
                }
                Loc::Cell { b: blk, .. } => {
                    tables.insert(blk);
                }
            }
        }
        for blk in tables {
            let all = self.table_paras(blk);
            let first = *all.first().unwrap();
            let last = *all.last().unwrap();
            if first > a.p && last < b.p {
                remove.insert(blk);
            } else {
                for i in all {
                    if i > a.p && i < b.p {
                        let n = self.plen(i);
                        self.para_mut(i).delete(0, n);
                    }
                }
            }
        }
        for blk in remove.into_iter().rev() {
            self.blocks.remove(blk);
        }
        self.normalize_blocks();
        self.reindex();
        a
    }

    pub fn backspace(&mut self, at: Pos) -> Pos {
        if at.o > 0 {
            let prev = self.prev_pos(at);
            self.delete(prev, at);
            prev
        } else if at.p > 0 && self.same_container(at.p - 1, at.p) {
            let prev = Pos { p: at.p - 1, o: self.plen(at.p - 1) };
            self.delete(prev, at);
            prev
        } else {
            at
        }
    }

    pub fn delete_forward(&mut self, at: Pos) -> Pos {
        let len = self.plen(at.p);
        if at.o < len {
            let next = self.next_pos(at);
            self.delete(at, next);
        } else if at.p + 1 < self.len() && self.same_container(at.p, at.p + 1) {
            self.delete(at, Pos { p: at.p + 1, o: 0 });
        }
        at
    }

    pub fn fmt_at(&self, at: Pos) -> (CharFmt, Option<String>) {
        self.para(at.p).fmt_at(at.o)
    }

    pub fn modify_chars(&mut self, a: Pos, b: Pos, f: &dyn Fn(&mut Run)) {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        for p in a.p..=b.p {
            let len = self.plen(p);
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { len };
            self.para_mut(p).modify(s, e, f);
        }
    }

    pub fn modify_paras(&mut self, p0: usize, p1: usize, f: &dyn Fn(&mut Paragraph)) {
        let (p0, p1) = if p0 <= p1 { (p0, p1) } else { (p1, p0) };
        for p in p0..=p1.min(self.len() - 1) {
            f(self.para_mut(p));
        }
    }

    // ------------------------------------------------------------ fragments (clipboard)

    pub fn slice(&self, a: Pos, b: Pos) -> Vec<Paragraph> {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a.p == b.p {
            return vec![self.para(a.p).slice(a.o, b.o)];
        }
        let mut out = vec![self.para(a.p).slice(a.o, self.plen(a.p))];
        for p in a.p + 1..b.p {
            out.push(self.para(p).clone());
        }
        out.push(self.para(b.p).slice(0, b.o));
        out
    }

    pub fn insert_fragment(&mut self, at: Pos, frag: &[Paragraph]) -> Pos {
        if frag.is_empty() {
            return at;
        }
        if frag.len() == 1 {
            let mut o = at.o;
            for r in &frag[0].runs {
                if r.text.is_empty() {
                    continue;
                }
                let n = r.text.chars().count();
                if let Some(obj) = &r.obj {
                    self.para_mut(at.p).insert_obj(o, obj.clone(), Some(&r.fmt));
                } else {
                    self.para_mut(at.p).insert(o, &r.text, Some(&r.fmt), Some(r.link.clone()));
                }
                o += n;
            }
            return Pos { p: at.p, o };
        }
        let target_was_empty = self.plen(at.p) == 0;
        let head_empty = at.o == 0;
        // split the target paragraph in two
        let mut tail = self.para_mut(at.p).split_off(at.o);
        {
            let head = self.para_mut(at.p);
            let first = &frag[0];
            head.runs.retain(|r| !r.text.is_empty());
            head.runs.extend(first.runs.iter().filter(|r| !r.text.is_empty()).cloned());
            head.normalize();
            if head_empty {
                head.style = first.style.clone();
                head.fmt = first.fmt.clone();
                head.list = first.list;
            }
        }
        let last = frag.last().unwrap();
        let mut new_paras: Vec<Paragraph> = frag[1..frag.len() - 1].iter().map(|p| {
            let mut c = p.clone();
            c.touch();
            c
        }).collect();
        let last_len = last.len();
        let mut runs: Vec<Run> = last.runs.iter().filter(|r| !r.text.is_empty()).cloned().collect();
        runs.extend(tail.runs.drain(..).filter(|r| !r.text.is_empty()));
        tail.runs = runs;
        tail.normalize();
        if target_was_empty {
            tail.style = last.style.clone();
            tail.fmt = last.fmt.clone();
            tail.list = last.list;
        }
        tail.touch();
        new_paras.push(tail);
        let n_new = new_paras.len();
        match self.locs[at.p] {
            Loc::Body(b) => {
                for (i, p) in new_paras.into_iter().enumerate() {
                    self.blocks.insert(b + 1 + i, Arc::new(Block::Para(p)));
                }
            }
            Loc::Cell { b, r, c, p } => {
                let cell = &mut self.tbl_mut(b).rows[r].cells[c];
                for (i, np) in new_paras.into_iter().enumerate() {
                    cell.paras.insert(p + 1 + i, np);
                }
            }
        }
        self.reindex();
        Pos { p: at.p + n_new, o: last_len }
    }

    pub fn text_range(&self, a: Pos, b: Pos) -> String {
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        let mut out = String::new();
        for p in a.p..=b.p {
            let len = self.plen(p);
            let s = if p == a.p { a.o } else { 0 };
            let e = if p == b.p { b.o } else { len };
            if p > a.p {
                out.push('\n');
            }
            out.push_str(&self.para(p).slice(s, e).plain());
        }
        out
    }

    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for p in 0..self.len() {
            if p > 0 {
                out.push('\n');
            }
            out.push_str(&self.para(p).plain());
        }
        out
    }

    // ------------------------------------------------------------ navigation helpers

    fn grapheme_bounds(text: &str) -> Vec<usize> {
        let mut v = vec![0];
        let mut n = 0;
        for g in text.graphemes(true) {
            n += g.chars().count();
            v.push(n);
        }
        v
    }

    pub fn prev_pos(&self, at: Pos) -> Pos {
        if at.o > 0 {
            let text = self.para(at.p).text();
            let b = Self::grapheme_bounds(&text);
            let o = b.iter().rev().find(|&&x| x < at.o).copied().unwrap_or(0);
            Pos { p: at.p, o }
        } else if at.p > 0 {
            Pos { p: at.p - 1, o: self.plen(at.p - 1) }
        } else {
            at
        }
    }

    pub fn next_pos(&self, at: Pos) -> Pos {
        let len = self.plen(at.p);
        if at.o < len {
            let text = self.para(at.p).text();
            let b = Self::grapheme_bounds(&text);
            let o = b.iter().find(|&&x| x > at.o).copied().unwrap_or(len);
            Pos { p: at.p, o }
        } else if at.p + 1 < self.len() {
            Pos { p: at.p + 1, o: 0 }
        } else {
            at
        }
    }

    pub fn word_left(&self, at: Pos) -> Pos {
        if at.o == 0 {
            return self.prev_pos(at);
        }
        let chars: Vec<char> = self.para(at.p).text().chars().collect();
        let mut i = at.o.min(chars.len());
        while i > 0 && char_class(chars[i - 1]) == 0 {
            i -= 1;
        }
        if i > 0 {
            let cl = char_class(chars[i - 1]);
            while i > 0 && char_class(chars[i - 1]) == cl {
                i -= 1;
            }
        }
        Pos { p: at.p, o: i }
    }

    pub fn word_right(&self, at: Pos) -> Pos {
        let chars: Vec<char> = self.para(at.p).text().chars().collect();
        if at.o >= chars.len() {
            return self.next_pos(at);
        }
        let mut i = at.o;
        let cl = char_class(chars[i]);
        if cl != 0 {
            while i < chars.len() && char_class(chars[i]) == cl {
                i += 1;
            }
        }
        while i < chars.len() && char_class(chars[i]) == 0 {
            i += 1;
        }
        Pos { p: at.p, o: i }
    }

    /// Word under `at` (for double-click); includes trailing spaces like Word.
    pub fn word_range(&self, at: Pos) -> (Pos, Pos) {
        let chars: Vec<char> = self.para(at.p).text().chars().collect();
        if chars.is_empty() {
            return (at, at);
        }
        let i = at.o.min(chars.len() - 1);
        let cl = char_class(chars[i]);
        let mut s = i;
        while s > 0 && char_class(chars[s - 1]) == cl {
            s -= 1;
        }
        let mut e = i;
        while e < chars.len() && char_class(chars[e]) == cl {
            e += 1;
        }
        if cl != 0 {
            while e < chars.len() && chars[e] == ' ' {
                e += 1;
            }
        }
        (Pos { p: at.p, o: s }, Pos { p: at.p, o: e })
    }

    // ------------------------------------------------------------ tables

    pub fn insert_table(&mut self, at: Pos, rows: usize, cols: usize, width: f32) -> Option<Pos> {
        let b = match self.locs[at.p] {
            Loc::Body(b) => b,
            _ => return None,
        };
        let table = Table::new(rows.max(1), cols.max(1), width);
        let len = self.plen(at.p);
        let insert_at = if at.o == 0 {
            b
        } else if at.o >= len {
            b + 1
        } else {
            self.split(at);
            // the paragraph block has been split; the table goes between the halves
            let nb = match self.locs[at.p] {
                Loc::Body(x) => x,
                _ => b,
            };
            nb + 1
        };
        self.blocks.insert(insert_at, Arc::new(Block::Table(table)));
        self.normalize_blocks();
        self.reindex();
        self.cell_first_para(insert_at, 0, 0).map(|p| Pos { p, o: 0 })
    }

    pub fn origin_of(t: &Table, r: usize, c: usize) -> (usize, usize) {
        if !t.rows[r].cells[c].covered {
            return (r, c);
        }
        for rr in (0..=r).rev() {
            for cc in (0..=c).rev() {
                let cell = &t.rows[rr].cells[cc];
                if !cell.covered && rr + cell.rs > r && cc + cell.cs > c {
                    return (rr, cc);
                }
            }
        }
        (r, c)
    }

    fn caret_in_cell(&self, b: usize, r: usize, c: usize) -> Pos {
        let t = self.tbl(b);
        let r = r.min(t.rows.len() - 1);
        let c = c.min(t.ncols() - 1);
        let (r, c) = Self::origin_of(t, r, c);
        self.cell_first_para(b, r, c).map(|p| Pos { p, o: 0 }).unwrap_or_default()
    }

    pub fn table_insert_row(&mut self, tr: TableRef, below: bool) -> Pos {
        let idx = if below { tr.r + 1 } else { tr.r };
        {
            let t = self.tbl_mut(tr.b);
            let n = t.ncols();
            let mut covered_cols = vec![false; n];
            let mut extend: BTreeSet<(usize, usize)> = BTreeSet::new();
            if idx > 0 && idx < t.rows.len() {
                for c in 0..n {
                    if t.rows[idx].cells[c].covered {
                        let (r0, c0) = Self::origin_of(t, idx, c);
                        if r0 < idx {
                            covered_cols[c] = true;
                            extend.insert((r0, c0));
                        }
                    }
                }
            }
            for (r0, c0) in extend {
                t.rows[r0].cells[c0].rs += 1;
            }
            let cells = (0..n).map(|c| if covered_cols[c] { Cell::covered() } else { Cell::new() }).collect();
            t.rows.insert(idx, TRow { cells, height: None, header: false });
        }
        self.reindex();
        self.caret_in_cell(tr.b, idx, tr.c)
    }

    pub fn table_insert_col(&mut self, tr: TableRef, right: bool) -> Pos {
        let j = if right {
            let t = self.tbl(tr.b);
            tr.c + t.rows[tr.r].cells[tr.c].cs
        } else {
            tr.c
        };
        {
            let t = self.tbl_mut(tr.b);
            let n = t.ncols();
            let refc = if j < n { j } else { n - 1 };
            let w = t.cols[refc] / 2.0;
            t.cols[refc] = w;
            t.cols.insert(j, w);
            let rows = t.rows.len();
            let mut flags = vec![false; rows];
            let mut extend: BTreeSet<(usize, usize)> = BTreeSet::new();
            if j > 0 && j < n {
                for r in 0..rows {
                    if t.rows[r].cells[j].covered {
                        let (r0, c0) = Self::origin_of(t, r, j);
                        if c0 < j {
                            flags[r] = true;
                            extend.insert((r0, c0));
                        }
                    }
                }
            }
            for (r0, c0) in extend {
                t.rows[r0].cells[c0].cs += 1;
            }
            for r in 0..rows {
                t.rows[r].cells.insert(j, if flags[r] { Cell::covered() } else { Cell::new() });
            }
        }
        self.reindex();
        self.caret_in_cell(tr.b, tr.r, j)
    }

    pub fn table_delete_row(&mut self, tr: TableRef) -> Pos {
        let nrows = self.tbl(tr.b).rows.len();
        if nrows <= 1 {
            return self.table_delete(tr.b);
        }
        // delete the whole span of rows covered by the origin cell's row only (the caret's row)
        let r = tr.r;
        {
            let t = self.tbl_mut(tr.b);
            let n = t.ncols();
            for c in 0..n {
                let cell = t.rows[r].cells[c].clone();
                if cell.covered {
                    let (r0, c0) = Self::origin_of(t, r, c);
                    if r0 < r && c == c0 {
                        t.rows[r0].cells[c0].rs -= 1;
                    }
                } else if cell.rs > 1 {
                    t.rows[r + 1].cells[c] = Cell { paras: cell.paras.clone(), cs: cell.cs, rs: cell.rs - 1, covered: false, shade: cell.shade, valign: cell.valign };
                }
            }
            t.rows.remove(r);
        }
        self.reindex();
        self.caret_in_cell(tr.b, r.min(nrows - 2), tr.c)
    }

    pub fn table_delete_col(&mut self, tr: TableRef) -> Pos {
        let ncols = self.tbl(tr.b).ncols();
        if ncols <= 1 {
            return self.table_delete(tr.b);
        }
        let j = tr.c;
        {
            let t = self.tbl_mut(tr.b);
            let rows = t.rows.len();
            enum Op {
                Dec(usize, usize),
                Move(usize),
            }
            let mut ops: Vec<(usize, Op)> = vec![];
            for r in 0..rows {
                let cell = &t.rows[r].cells[j];
                if cell.covered {
                    let (r0, c0) = Self::origin_of(t, r, j);
                    if c0 < j && r == r0 {
                        ops.push((r, Op::Dec(r0, c0)));
                    }
                } else if cell.cs > 1 {
                    ops.push((r, Op::Move(r)));
                }
            }
            for (_, op) in ops {
                match op {
                    Op::Dec(r0, c0) => t.rows[r0].cells[c0].cs -= 1,
                    Op::Move(r) => {
                        let cell = t.rows[r].cells[j].clone();
                        t.rows[r].cells[j + 1] = Cell { paras: cell.paras, cs: cell.cs - 1, rs: cell.rs, covered: false, shade: cell.shade, valign: cell.valign };
                    }
                }
            }
            for r in 0..rows {
                t.rows[r].cells.remove(j);
            }
            let w = t.cols.remove(j);
            let k = if j > 0 { j - 1 } else { 0 };
            t.cols[k] += w;
        }
        self.reindex();
        self.caret_in_cell(tr.b, tr.r, j.min(ncols - 2))
    }

    pub fn table_delete(&mut self, b: usize) -> Pos {
        let first = self.table_paras(b).first().copied().unwrap_or(0);
        self.blocks.remove(b);
        self.normalize_blocks();
        self.reindex();
        let p = first.min(self.len() - 1);
        Pos { p, o: 0 }
    }

    pub fn table_merge(&mut self, b: usize, r0: usize, c0: usize, r1: usize, c1: usize) -> Pos {
        let (mut r0, mut c0, mut r1, mut c1) = (r0.min(r1), c0.min(c1), r0.max(r1), c0.max(c1));
        {
            let t = self.tbl_mut(b);
            r1 = r1.min(t.rows.len() - 1);
            c1 = c1.min(t.ncols() - 1);
            loop {
                let mut changed = false;
                for r in r0..=r1 {
                    for c in c0..=c1 {
                        let (or, oc) = Self::origin_of(t, r, c);
                        let cell = &t.rows[or].cells[oc];
                        let (er, ec) = (or + cell.rs - 1, oc + cell.cs - 1);
                        if or < r0 {
                            r0 = or;
                            changed = true;
                        }
                        if oc < c0 {
                            c0 = oc;
                            changed = true;
                        }
                        if er > r1 {
                            r1 = er;
                            changed = true;
                        }
                        if ec > c1 {
                            c1 = ec;
                            changed = true;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            let mut merged: Vec<Paragraph> = vec![];
            for r in r0..=r1 {
                for c in c0..=c1 {
                    let cell = &t.rows[r].cells[c];
                    if cell.covered {
                        continue;
                    }
                    for p in &cell.paras {
                        if !p.is_empty() || cell.paras.len() > 1 {
                            if !(p.is_empty() && cell.paras.len() == 1) {
                                merged.push(p.clone());
                            }
                        }
                    }
                }
            }
            if merged.is_empty() {
                merged.push(Paragraph::new());
            }
            let shade = t.rows[r0].cells[c0].shade;
            let valign = t.rows[r0].cells[c0].valign;
            for r in r0..=r1 {
                for c in c0..=c1 {
                    t.rows[r].cells[c] = Cell::covered();
                }
            }
            t.rows[r0].cells[c0] = Cell { paras: merged, cs: c1 - c0 + 1, rs: r1 - r0 + 1, covered: false, shade, valign };
        }
        self.reindex();
        self.caret_in_cell(b, r0, c0)
    }

    pub fn table_split(&mut self, b: usize, r: usize, c: usize) -> Pos {
        let (or, oc);
        {
            let t = self.tbl_mut(b);
            let o = Self::origin_of(t, r, c);
            or = o.0;
            oc = o.1;
            let (rs, cs) = (t.rows[or].cells[oc].rs, t.rows[or].cells[oc].cs);
            for rr in or..or + rs {
                for cc in oc..oc + cs {
                    if (rr, cc) != (or, oc) {
                        t.rows[rr].cells[cc] = Cell::new();
                    }
                }
            }
            t.rows[or].cells[oc].rs = 1;
            t.rows[or].cells[oc].cs = 1;
        }
        self.reindex();
        self.caret_in_cell(b, or, oc)
    }

    pub fn table_set_shade(&mut self, b: usize, r0: usize, c0: usize, r1: usize, c1: usize, shade: Option<Rgb>) {
        let t = self.tbl_mut(b);
        for r in r0.min(r1)..=r0.max(r1).min(t.rows.len() - 1) {
            for c in c0.min(c1)..=c0.max(c1).min(t.ncols() - 1) {
                let (or, oc) = Self::origin_of(t, r, c);
                t.rows[or].cells[oc].shade = shade;
            }
        }
    }

    pub fn table_set_valign(&mut self, b: usize, r0: usize, c0: usize, r1: usize, c1: usize, v: VAlign) {
        let t = self.tbl_mut(b);
        for r in r0.min(r1)..=r0.max(r1).min(t.rows.len() - 1) {
            for c in c0.min(c1)..=c0.max(c1).min(t.ncols() - 1) {
                let (or, oc) = Self::origin_of(t, r, c);
                t.rows[or].cells[oc].valign = v;
            }
        }
    }

    /// Cell rectangle (r0,c0,r1,c1) spanned by two positions inside the same table.
    pub fn cell_range(&self, a: usize, b: usize) -> Option<(usize, usize, usize, usize, usize)> {
        let ta = self.table_at(a)?;
        let tb = self.table_at(b)?;
        if ta.b != tb.b {
            return None;
        }
        let t = self.tbl(ta.b);
        let ca = &t.rows[ta.r].cells[ta.c];
        let cb = &t.rows[tb.r].cells[tb.c];
        let r0 = ta.r.min(tb.r);
        let c0 = ta.c.min(tb.c);
        let r1 = (ta.r + ca.rs - 1).max(tb.r + cb.rs - 1);
        let c1 = (ta.c + ca.cs - 1).max(tb.c + cb.cs - 1);
        Some((ta.b, r0, c0, r1, c1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn story_with(texts: &[&str]) -> Story {
        Story::from_blocks(texts.iter().map(|t| Block::Para(Paragraph::with_text(t))).collect())
    }

    #[test]
    fn type_split_and_merge() {
        let mut s = Story::new();
        let p = s.insert_text(Pos::new(0, 0), "Hello world", None);
        assert_eq!(p, Pos::new(0, 11));
        let p = s.split(Pos::new(0, 5));
        assert_eq!(p, Pos::new(1, 0));
        assert_eq!(s.para(0).text(), "Hello");
        assert_eq!(s.para(1).text(), " world");
        let p = s.backspace(Pos::new(1, 0));
        assert_eq!(p, Pos::new(0, 5));
        assert_eq!(s.len(), 1);
        assert_eq!(s.para(0).text(), "Hello world");
    }

    #[test]
    fn paste_multiline() {
        let mut s = Story::new();
        let p = s.insert_text(Pos::new(0, 0), "a\nb\r\nc", None);
        assert_eq!(s.len(), 3);
        assert_eq!(p, Pos::new(2, 1));
        assert_eq!(s.plain_text(), "a\nb\nc");
    }

    #[test]
    fn formatting_splits_runs() {
        let mut s = story_with(&["abcdef"]);
        s.modify_chars(Pos::new(0, 2), Pos::new(0, 4), &|r| r.fmt.bold = Some(true));
        let p = s.para(0);
        assert_eq!(p.runs.len(), 3);
        assert_eq!(p.runs[1].text, "cd");
        assert_eq!(p.runs[1].fmt.bold, Some(true));
        s.modify_chars(Pos::new(0, 2), Pos::new(0, 4), &|r| r.fmt.bold = None);
        assert_eq!(s.para(0).runs.len(), 1);
    }

    #[test]
    fn delete_across_paragraphs() {
        let mut s = story_with(&["one two", "three", "four five"]);
        let p = s.delete(Pos::new(0, 4), Pos::new(2, 5));
        assert_eq!(p, Pos::new(0, 4));
        assert_eq!(s.len(), 1);
        assert_eq!(s.para(0).text(), "one five");
    }

    #[test]
    fn insert_fragment_roundtrip() {
        let mut s = story_with(&["alpha beta", "gamma"]);
        let frag = s.slice(Pos::new(0, 6), Pos::new(1, 3));
        assert_eq!(frag.len(), 2);
        let end = s.insert_fragment(Pos::new(1, 5), &frag);
        assert_eq!(s.plain_text(), "alpha beta\ngammabeta\ngam");
        assert_eq!(end, Pos::new(2, 3));
    }

    #[test]
    fn cow_snapshots_are_independent() {
        let mut s = story_with(&["x", "y"]);
        let snap = s.clone();
        s.insert_text(Pos::new(0, 1), "123", None);
        assert_eq!(snap.para(0).text(), "x");
        assert_eq!(s.para(0).text(), "x123");
    }

    #[test]
    fn tables_insert_edit_delete() {
        let mut s = story_with(&["before", "after"]);
        let p = s.insert_table(Pos::new(0, 6), 2, 3, 300.0).unwrap();
        assert!(s.table_at(p.p).is_some());
        assert_eq!(s.len(), 2 + 6);
        // typing in a cell
        let end = s.insert_text(p, "cell", None);
        assert_eq!(s.para(p.p).text(), "cell");
        assert_eq!(end.p, p.p);
        // can't backspace out of the cell into the paragraph before
        assert_eq!(s.backspace(Pos::new(p.p, 0)), Pos::new(p.p, 0));
        // add row & column
        let tr = s.table_at(p.p).unwrap();
        s.table_insert_row(tr, true);
        assert_eq!(s.len(), 2 + 9);
        let tr = s.table_at(p.p).unwrap();
        s.table_insert_col(tr, true);
        assert_eq!(s.len(), 2 + 12);
        assert_eq!(s.tbl(tr.b).cols.len(), 4);
        assert!((s.tbl(tr.b).width() - 300.0).abs() < 0.01);
        // delete spanning from before the table to after it clears everything between
        let last = s.end_pos();
        s.delete(Pos::new(0, 2), Pos::new(last.p, 2));
        assert!(s.blocks.iter().all(|b| matches!(&**b, Block::Para(_))));
        assert_eq!(s.plain_text(), "beter");
    }

    #[test]
    fn table_merge_and_split_and_row_col_deletes() {
        let mut s = Story::new();
        let p = s.insert_table(Pos::new(0, 0), 3, 3, 300.0).unwrap();
        let tr = s.table_at(p.p).unwrap();
        s.insert_text(p, "A", None);
        s.table_merge(tr.b, 0, 0, 1, 1);
        {
            let t = s.tbl(tr.b);
            assert_eq!((t.rows[0].cells[0].rs, t.rows[0].cells[0].cs), (2, 2));
            assert!(t.rows[1].cells[1].covered);
        }
        // covered cells have no caret positions: 9 - 3 covered = 6 cells + trailing paragraph
        assert_eq!(s.len(), 6 + 1);
        // inserting a row inside the span extends it
        let tr0 = TableRef { b: tr.b, r: 0, c: 0 };
        s.table_insert_row(tr0, true);
        assert_eq!(s.tbl(tr.b).rows[0].cells[0].rs, 3);
        // delete that row again
        let tr1 = TableRef { b: tr.b, r: 1, c: 2 };
        s.table_delete_row(tr1);
        assert_eq!(s.tbl(tr.b).rows[0].cells[0].rs, 2);
        // delete column inside span shrinks it
        s.table_delete_col(TableRef { b: tr.b, r: 2, c: 1 });
        assert_eq!(s.tbl(tr.b).rows[0].cells[0].cs, 1);
        s.table_split(tr.b, 0, 0);
        assert_eq!(s.tbl(tr.b).rows[0].cells[0].rs, 1);
        assert!(!s.tbl(tr.b).rows[1].cells[0].covered);
    }

    #[test]
    fn word_navigation() {
        let s = story_with(&["hello  big-world"]);
        assert_eq!(s.word_right(Pos::new(0, 0)), Pos::new(0, 7));
        assert_eq!(s.word_right(Pos::new(0, 7)), Pos::new(0, 10));
        assert_eq!(s.word_left(Pos::new(0, 7)), Pos::new(0, 0));
        let (a, b) = s.word_range(Pos::new(0, 2));
        assert_eq!((a.o, b.o), (0, 7));
    }

    #[test]
    fn page_break_ends_paragraph() {
        let mut s = story_with(&["abcd"]);
        let p = s.insert_page_break(Pos::new(0, 2));
        assert_eq!(p, Pos::new(1, 0));
        assert!(s.para(0).text().ends_with(PAGE_BREAK));
        assert_eq!(s.para(1).text(), "cd");
    }
}

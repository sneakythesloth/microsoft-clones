//! File operations: open / save / export / print.

use std::path::{Path, PathBuf};

use crate::app::*;
use crate::model::*;
use crate::story::Story;

pub fn open_url(url: &str) {
    use std::process::Command;
    #[cfg(target_os = "windows")]
    let _ = Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

pub fn text_to_doc(text: &str) -> Document {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks: Vec<Block> = vec![];
    for line in text.split('\n') {
        let clean: String = line.chars().filter(|c| !c.is_control() || *c == '\t').collect();
        blocks.push(Block::Para(Paragraph::with_text(&clean)));
    }
    let mut doc = Document::new();
    doc.body = Story::from_blocks(blocks);
    doc
}

/// Load any supported file into a Document.
pub fn load_file(path: &Path) -> Result<Document, String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    match ext.as_str() {
        "docx" | "dotx" | "docm" => crate::docx::read(&bytes),
        _ => Ok(text_to_doc(&String::from_utf8_lossy(&bytes))),
    }
}

/// Write a Document in the format implied by the extension.
pub fn export_bytes(doc: &Document, ext: &str, fonts: &crate::fonts::FontStore, pag: &crate::layout::Pagination) -> Result<Vec<u8>, String> {
    match ext {
        "docx" => crate::docx::write(doc),
        "pdf" => crate::pdf::export(doc, fonts, pag),
        "html" | "htm" => Ok(crate::export::html(doc, &|f| fonts.is_google_installed(f)).into_bytes()),
        "md" => Ok(crate::export::markdown(doc).into_bytes()),
        "txt" => Ok(crate::export::text(doc).into_bytes()),
        _ => Err(format!("Unsupported format: .{ext}")),
    }
}

impl App {
    fn load_document(&mut self, doc: Document, path: Option<PathBuf>) {
        self.doc = doc;
        self.kind = StoryKind::Body;
        self.sel = Sel::default();
        self.pending = CharFmt::default();
        self.hist = History::default();
        self.pag = None;
        self.pag_key = None;
        self.path = path;
        self.dirty = false;
        self.dlg.find.matches.clear();
        self.dlg.find.cur = None;
        self.scroll_to_caret = true;
        self.format_painter = None;
        self.next_list_id = 1;
    }

    pub fn new_doc(&mut self) {
        self.load_document(Document::new(), None);
        self.say("New document");
    }

    pub fn new_request(&mut self) {
        if self.dirty {
            self.confirm = Some(Confirm::New);
        } else {
            self.new_doc();
        }
    }

    pub fn open_request(&mut self) {
        if self.dirty {
            self.confirm = Some(Confirm::Open(None));
        } else {
            self.open_dialog();
        }
    }

    pub fn open_dialog(&mut self) {
        let picked = rfd::FileDialog::new().add_filter("Documents", &["docx", "txt", "md"]).add_filter("Word document", &["docx"]).add_filter("Text", &["txt", "md"]).add_filter("All files", &["*"]).pick_file();
        if let Some(p) = picked {
            self.open_path(p);
        }
    }

    pub fn open_path(&mut self, p: PathBuf) {
        match load_file(&p) {
            Ok(doc) => {
                let is_docx = p.extension().map(|e| e.eq_ignore_ascii_case("docx")).unwrap_or(false);
                self.load_document(doc, if is_docx { Some(p.clone()) } else { None });
                self.settings.add_recent(&p);
                self.say(format!("Opened {}", p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                if !is_docx {
                    self.dirty = true; // will need "Save As" to become a .docx
                }
            }
            Err(e) => self.say(format!("Could not open file: {e}")),
        }
    }

    pub fn save(&mut self) -> bool {
        match self.path.clone() {
            Some(p) => self.save_to(p),
            None => self.save_as_dialog(),
        }
    }

    pub fn save_as_dialog(&mut self) -> bool {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Document1".into());
        let picked = rfd::FileDialog::new().add_filter("Word document (*.docx)", &["docx"]).set_file_name(format!("{name}.docx")).save_file();
        match picked {
            Some(mut p) => {
                if p.extension().is_none() {
                    p.set_extension("docx");
                }
                self.save_to(p)
            }
            None => false,
        }
    }

    pub fn save_to(&mut self, p: PathBuf) -> bool {
        self.ensure_layout();
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "docx".into());
        let pag = self.pag.as_ref().unwrap();
        match export_bytes(&self.doc, &ext, &self.fonts, pag).and_then(|b| std::fs::write(&p, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                self.path = Some(p.clone());
                self.dirty = false;
                self.settings.add_recent(&p);
                self.say(format!("Saved {}", p.display()));
                true
            }
            Err(e) => {
                self.say(format!("Save failed: {e}"));
                false
            }
        }
    }

    pub fn export_dialog(&mut self, ext: &str) {
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Document1".into());
        let label = match ext {
            "pdf" => "PDF",
            "html" => "Web page",
            "md" => "Markdown",
            _ => "Text",
        };
        let picked = rfd::FileDialog::new().add_filter(label, &[ext]).set_file_name(format!("{name}.{ext}")).save_file();
        if let Some(mut p) = picked {
            if p.extension().is_none() {
                p.set_extension(ext);
            }
            self.ensure_layout();
            let pag = self.pag.as_ref().unwrap();
            match export_bytes(&self.doc, ext, &self.fonts, pag).and_then(|b| std::fs::write(&p, b).map_err(|e| e.to_string())) {
                Ok(()) => self.say(format!("Exported {}", p.display())),
                Err(e) => self.say(format!("Export failed: {e}")),
            }
        }
    }

    /// Printing = render a PDF and hand it to the system viewer, which has the print dialog.
    pub fn print(&mut self) {
        self.ensure_layout();
        let pag = self.pag.as_ref().unwrap();
        let path = std::env::temp_dir().join(format!("wordrs-print-{}.pdf", std::process::id()));
        match export_bytes(&self.doc, "pdf", &self.fonts, pag).and_then(|b| std::fs::write(&path, b).map_err(|e| e.to_string())) {
            Ok(()) => {
                open_url(&path.to_string_lossy());
                self.say("Opened a PDF in your viewer — use its Print command");
            }
            Err(e) => self.say(format!("Print failed: {e}")),
        }
    }

    pub fn insert_image_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "gif", "bmp", "webp"]).pick_file() {
            self.insert_image_path(&p);
        }
    }
}

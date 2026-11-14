#![allow(dead_code)]
mod app;
mod dialogs;
mod docx;
mod editor;
mod export;
mod fonts;
mod google;
mod io;
mod layout;
mod model;
mod pdf;
mod render;
mod ribbon;
mod story;

use std::path::PathBuf;

/// Headless conversion: `wordrs --convert input.docx output.pdf|docx|html|md|txt`
fn convert(input: &str, output: &str) -> Result<(), String> {
    let doc = io::load_file(std::path::Path::new(input))?;
    let ext = std::path::Path::new(output).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut fonts = fonts::FontStore::new();
    let sc = layout::StyleCache::build(&doc.styles, doc.styles_ver);
    let mut engine = layout::LayoutEngine::default();
    let pag = engine.paginate(&mut fonts, &doc, &sc);
    let bytes = io::export_bytes(&doc, &ext, &fonts, &pag)?;
    std::fs::write(output, bytes).map_err(|e| e.to_string())?;
    println!("{} → {} ({} pages)", input, output, pag.pages.len());
    Ok(())
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|a| a == "--convert").unwrap_or(false) {
        if args.len() < 3 {
            eprintln!("usage: wordrs --convert <input> <output.(pdf|docx|html|md|txt)>");
            std::process::exit(2);
        }
        if let Err(e) = convert(&args[1], &args[2]) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let initial = args.iter().find(|a| !a.starts_with("--")).map(PathBuf::from);
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1280.0, 860.0]).with_min_inner_size([760.0, 480.0]).with_title("Word RS").with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Word RS", opts, Box::new(|cc| Ok(Box::new(app::App::new(cc, initial)))))
}

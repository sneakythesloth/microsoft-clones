//! Google Fonts support: catalog (official API / metadata endpoint / bundled list)
//! and a downloader that gets static TTF files for Regular / Bold / Italic / Bold Italic.
//!
//! Download strategy (first that works wins):
//!   1. Developer API `files` map (needs a free API key)
//!   2. The public CSS2 endpoint (served as TTF to non-browser user agents)
//!   3. The `google/fonts` GitHub repository (OFL/Apache/UFL directories)

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use regex::Regex;

#[derive(Clone, Debug, PartialEq)]
pub struct GFont {
    pub family: String,
    pub category: String,
    /// e.g. "regular", "italic", "700", "700italic"; empty if unknown.
    pub variants: Vec<String>,
}

pub const SLOTS: [&str; 4] = ["Regular", "Bold", "Italic", "BoldItalic"];

const UA: &str = "wordrs/0.1 (word processor; font fetch)";

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent(UA).build()
}

fn http_get(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| match e {
        ureq::Error::Status(c, _) => format!("HTTP {c} for {url}"),
        e => format!("{e}"),
    })?;
    let mut buf = Vec::new();
    resp.into_reader().take(80 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn http_get_text(url: &str) -> Result<String, String> {
    let b = http_get(url)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

pub fn cache_root() -> PathBuf {
    dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("wordrs").join("google-fonts")
}

pub fn catalog_cache_path() -> PathBuf {
    cache_root().join("catalog.json")
}

// ------------------------------------------------------------------ catalog

fn norm_category(c: &str) -> String {
    match c.to_lowercase().replace('_', "-").replace(' ', "-").as_str() {
        "sans-serif" => "Sans Serif".into(),
        "serif" => "Serif".into(),
        "display" => "Display".into(),
        "handwriting" => "Handwriting".into(),
        "monospace" => "Monospace".into(),
        other => other.to_string(),
    }
}

/// Parse the official Developer API response (`webfonts/v1/webfonts`).
pub fn parse_api_catalog(json: &str) -> Result<Vec<GFont>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let items = v.get("items").and_then(|i| i.as_array()).ok_or("no items in response")?;
    Ok(items
        .iter()
        .filter_map(|it| {
            Some(GFont {
                family: it.get("family")?.as_str()?.to_string(),
                category: norm_category(it.get("category").and_then(|c| c.as_str()).unwrap_or("")),
                variants: it.get("variants").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            })
        })
        .collect())
}

/// Parse `https://fonts.google.com/metadata/fonts` (has an anti-XSSI prefix).
pub fn parse_metadata_catalog(text: &str) -> Result<Vec<GFont>, String> {
    let start = text.find('{').ok_or("not JSON")?;
    let v: serde_json::Value = serde_json::from_str(&text[start..]).map_err(|e| e.to_string())?;
    let list = v.get("familyMetadataList").and_then(|i| i.as_array()).ok_or("no familyMetadataList")?;
    Ok(list
        .iter()
        .filter_map(|it| {
            let variants = it
                .get("fonts")
                .and_then(|f| f.as_object())
                .map(|o| {
                    o.keys()
                        .map(|k| {
                            let italic = k.ends_with('i');
                            let w = k.trim_end_matches('i');
                            match (w, italic) {
                                ("400", false) => "regular".to_string(),
                                ("400", true) => "italic".to_string(),
                                (w, false) => w.to_string(),
                                (w, true) => format!("{w}italic"),
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(GFont { family: it.get("family")?.as_str()?.to_string(), category: norm_category(it.get("category").and_then(|c| c.as_str()).unwrap_or("")), variants })
        })
        .collect())
}

/// Download the full catalog. Uses the official API when a key is supplied.
pub fn fetch_catalog(api_key: Option<&str>) -> Result<Vec<GFont>, String> {
    let mut errors = vec![];
    if let Some(k) = api_key.filter(|k| !k.trim().is_empty()) {
        match http_get_text(&format!("https://www.googleapis.com/webfonts/v1/webfonts?key={}&sort=popularity", k.trim())).and_then(|t| parse_api_catalog(&t)) {
            Ok(c) if !c.is_empty() => return Ok(c),
            Ok(_) => errors.push("API returned no fonts".to_string()),
            Err(e) => errors.push(format!("API: {e}")),
        }
    }
    match http_get_text("https://fonts.google.com/metadata/fonts").and_then(|t| parse_metadata_catalog(&t)) {
        Ok(c) if !c.is_empty() => return Ok(c),
        Ok(_) => errors.push("metadata returned no fonts".to_string()),
        Err(e) => errors.push(format!("metadata: {e}")),
    }
    Err(errors.join("; "))
}

pub fn save_catalog(c: &[GFont]) {
    let arr: Vec<serde_json::Value> = c.iter().map(|f| serde_json::json!({"f": f.family, "c": f.category, "v": f.variants})).collect();
    let _ = std::fs::create_dir_all(cache_root());
    let _ = std::fs::write(catalog_cache_path(), serde_json::to_vec(&arr).unwrap_or_default());
}

pub fn load_cached_catalog() -> Option<Vec<GFont>> {
    let data = std::fs::read(catalog_cache_path()).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&data).ok()?;
    let out: Vec<GFont> = v
        .as_array()?
        .iter()
        .filter_map(|e| {
            Some(GFont {
                family: e.get("f")?.as_str()?.to_string(),
                category: e.get("c")?.as_str()?.to_string(),
                variants: e.get("v").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
            })
        })
        .collect();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// A bundled list of popular families so the browser is useful offline.
pub fn bundled_catalog() -> Vec<GFont> {
    const L: &[(&str, &str)] = &[
        ("Sans Serif", "Roboto|Open Sans|Noto Sans|Lato|Montserrat|Poppins|Inter|Source Sans 3|Oswald|Raleway|Nunito|Nunito Sans|Ubuntu|Rubik|Work Sans|PT Sans|Mukta|Barlow|Fira Sans|DM Sans|Quicksand|Karla|Manrope|Heebo|Josefin Sans|Cabin|Libre Franklin|Assistant|Hind|Titillium Web|Catamaran|Mulish|IBM Plex Sans|Archivo|Exo 2|Overpass|Asap|Signika|Maven Pro|Arimo|Carlito|Red Hat Display|Lexend|Outfit|Plus Jakarta Sans|Sora|Space Grotesk|Urbanist|Albert Sans|Be Vietnam Pro|Public Sans|Questrial|Didact Gothic|Varela Round|Comfortaa|Kanit|Prompt|Sarabun|Noto Sans JP|Noto Sans KR|Noto Sans SC|Noto Sans Arabic|Noto Sans Hebrew|Noto Sans Devanagari|Noto Sans Thai|Open Sans Condensed|Roboto Condensed|Encode Sans|Jost|Figtree|Hanken Grotesk|Inria Sans|Istok Web|Abel|Cairo|Tajawal|Almarai"),
        ("Serif", "Roboto Serif|Noto Serif|Playfair Display|Merriweather|Lora|PT Serif|Source Serif 4|Libre Baskerville|Crimson Text|EB Garamond|Cormorant Garamond|Cormorant|Bitter|Domine|Arvo|Zilla Slab|Roboto Slab|Cardo|Vollkorn|Spectral|Alegreya|Tinos|Caladea|Gelasio|DM Serif Display|DM Serif Text|Abhaya Libre|Libre Caslon Text|Old Standard TT|Rokkitt|Faustina|Newsreader|Fraunces|Literata|STIX Two Text|Noto Serif JP|Noto Serif KR|Noto Serif SC|Cinzel|Crimson Pro|Lustria|Playfair Display SC|Bree Serif|Neuton|Gentium Book Plus|Karma|Prata|Rufina|Sorts Mill Goudy"),
        ("Display", "Bebas Neue|Anton|Abril Fatface|Alfa Slab One|Righteous|Fredoka|Luckiest Guy|Bangers|Staatliches|Archivo Black|Passion One|Russo One|Monoton|Orbitron|Press Start 2P|Special Elite|Titan One|Chango|Fjalla One|Teko|Yeseva One|Shrikhand|Lilita One|Black Ops One|Bungee|Rubik Mono One|Squada One|Ultra|Fugaz One|Paytone One|Concert One|Creepster|Eater|Fascinate|Gloock|Limelight|Megrim|Rammetto One|Unbounded"),
        ("Handwriting", "Dancing Script|Pacifico|Caveat|Satisfy|Great Vibes|Shadows Into Light|Indie Flower|Kalam|Amatic SC|Courgette|Sacramento|Permanent Marker|Patrick Hand|Architects Daughter|Gloria Hallelujah|Handlee|Yellowtail|Parisienne|Allura|Tangerine|Homemade Apple|Rock Salt|Marck Script|Reenie Beanie|Covered By Your Grace|Nanum Pen Script|Cookie|Lobster|Lobster Two|Kaushan Script|Playball|Alex Brush|Bad Script|Merienda|Gochi Hand|Just Another Hand|Neucha|Rancho|Sriracha|Mansalva|Zeyada|La Belle Aurore"),
        ("Monospace", "Roboto Mono|Source Code Pro|Fira Code|Fira Mono|JetBrains Mono|Inconsolata|Space Mono|IBM Plex Mono|Ubuntu Mono|PT Mono|Cousine|Courier Prime|Anonymous Pro|Overpass Mono|DM Mono|Noto Sans Mono|Red Hat Mono|VT323|Share Tech Mono|Cutive Mono|Major Mono Display|Nova Mono|Oxygen Mono|Syne Mono|Martian Mono"),
    ];
    let mut out = vec![];
    for (cat, names) in L {
        for n in names.split('|') {
            out.push(GFont { family: n.to_string(), category: cat.to_string(), variants: vec![] });
        }
    }
    out.sort_by(|a, b| a.family.to_lowercase().cmp(&b.family.to_lowercase()));
    out.dedup_by(|a, b| a.family == b.family);
    out
}

// ------------------------------------------------------------------ CSS parsing

#[derive(Debug, Clone, PartialEq)]
pub struct CssFace {
    pub italic: bool,
    pub weight: u16,
    pub url: String,
}

/// Parse `@font-face` blocks out of a Google Fonts CSS response.
pub fn parse_css(css: &str) -> Vec<CssFace> {
    let url_re = Regex::new(r#"url\(\s*['"]?([^)'"]+)['"]?\s*\)"#).unwrap();
    let mut out = vec![];
    for block in css.split("@font-face").skip(1) {
        let body = block.split('}').next().unwrap_or("");
        let italic = body.contains("font-style: italic") || body.contains("font-style:italic");
        let weight = body
            .split("font-weight:")
            .nth(1)
            .and_then(|s| s.split(';').next())
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(400);
        if let Some(c) = url_re.captures(body) {
            let url = c[1].to_string();
            if !out.iter().any(|f: &CssFace| f.italic == italic && f.weight == weight) {
                out.push(CssFace { italic, weight, url });
            }
        }
    }
    out
}

fn pick(faces: &[(bool, u16, String)], italic: bool, target: u16) -> Option<String> {
    faces.iter().filter(|f| f.0 == italic).min_by_key(|f| (f.1 as i32 - target as i32).abs()).map(|f| f.2.clone())
}

fn slots_from(faces: Vec<(bool, u16, String)>) -> Vec<(&'static str, String)> {
    let mut out = vec![];
    if let Some(u) = pick(&faces, false, 400) {
        out.push(("Regular", u));
    }
    if let Some(u) = pick(&faces, false, 700) {
        if faces.iter().any(|f| !f.0 && f.1 >= 600) {
            out.push(("Bold", u));
        }
    }
    if let Some(u) = pick(&faces, true, 400) {
        out.push(("Italic", u));
    }
    if let Some(u) = pick(&faces, true, 700) {
        if faces.iter().any(|f| f.0 && f.1 >= 600) {
            out.push(("BoldItalic", u));
        }
    }
    out
}

// ------------------------------------------------------------------ sources

fn urls_from_api(family: &str, key: &str) -> Result<Vec<(&'static str, String)>, String> {
    let url = format!("https://www.googleapis.com/webfonts/v1/webfonts?key={}&family={}", key.trim(), family.replace(' ', "%20"));
    let v: serde_json::Value = serde_json::from_str(&http_get_text(&url)?).map_err(|e| e.to_string())?;
    let files = v.get("items").and_then(|i| i.get(0)).and_then(|i| i.get("files")).and_then(|f| f.as_object()).ok_or("family not found in API")?;
    let mut faces = vec![];
    for (k, u) in files {
        let italic = k.contains("italic");
        let w = k.replace("italic", "");
        let weight: u16 = if w.is_empty() || w == "regular" { 400 } else { w.parse().unwrap_or(400) };
        if let Some(u) = u.as_str() {
            faces.push((italic, weight, u.replace("http://", "https://")));
        }
    }
    Ok(slots_from(faces))
}

fn urls_from_css2(family: &str) -> Result<Vec<(&'static str, String)>, String> {
    let fam = family.replace(' ', "+");
    let attempts = [
        format!("{fam}:ital,wght@0,400;0,700;1,400;1,700"),
        format!("{fam}:ital,wght@0,400;0,700"),
        format!("{fam}:ital,wght@0,400;1,400"),
        format!("{fam}:wght@400;700"),
        format!("{fam}:wght@400"),
        fam.clone(),
    ];
    let mut last = String::new();
    for a in attempts {
        match http_get_text(&format!("https://fonts.googleapis.com/css2?family={a}&display=swap")) {
            Ok(css) => {
                let faces: Vec<(bool, u16, String)> = parse_css(&css).into_iter().map(|f| (f.italic, f.weight, f.url)).collect();
                let slots = slots_from(faces);
                if !slots.is_empty() {
                    return Ok(slots);
                }
                last = "CSS had no font files".into();
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn github_dir(family: &str) -> String {
    family.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase()
}

/// Classify a repository file name such as `Lato-BoldItalic.ttf`.
pub fn classify_file(name: &str) -> Option<&'static str> {
    if !name.to_lowercase().ends_with(".ttf") {
        return None;
    }
    let stem = name[..name.len() - 4].to_string();
    let suffix = stem.rsplit('-').next().unwrap_or(&stem);
    match suffix {
        "Regular" => Some("Regular"),
        "Bold" => Some("Bold"),
        "Italic" => Some("Italic"),
        "BoldItalic" => Some("BoldItalic"),
        _ => None,
    }
}

fn head_ok(url: &str) -> bool {
    agent().head(url).call().is_ok()
}

/// Rate-limit-free source: guess raw.githubusercontent.com file names from the family name.
fn urls_from_github_raw(family: &str) -> Result<Vec<(&'static str, String)>, String> {
    let dir = github_dir(family);
    let name: String = family.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    for lic in ["ofl", "apache", "ufl"] {
        for sub in ["", "static/"] {
            let base = format!("https://raw.githubusercontent.com/google/fonts/main/{lic}/{dir}/{sub}{name}-");
            if !head_ok(&format!("{base}Regular.ttf")) {
                continue;
            }
            let mut out = vec![];
            for slot in SLOTS {
                let url = format!("{base}{slot}.ttf");
                if slot == "Regular" || head_ok(&url) {
                    out.push((slot, url));
                }
            }
            return Ok(out);
        }
        // variable-only families: take the default instance of the variable files
        for axes in ["wght", "wdth,wght", "opsz,wght", "wght,opsz", "slnt,wght", "ital,wght"] {
            let enc = axes.replace(',', "%2C");
            let reg = format!("https://raw.githubusercontent.com/google/fonts/main/{lic}/{dir}/{name}%5B{enc}%5D.ttf");
            if head_ok(&reg) {
                let mut out = vec![("Regular", reg)];
                let it = format!("https://raw.githubusercontent.com/google/fonts/main/{lic}/{dir}/{name}-Italic%5B{enc}%5D.ttf");
                if head_ok(&it) {
                    out.push(("Italic", it));
                }
                return Ok(out);
            }
        }
    }
    Err("family not found in google/fonts".into())
}

fn urls_from_github(family: &str) -> Result<Vec<(&'static str, String)>, String> {
    if let Ok(v) = urls_from_github_raw(family) {
        return Ok(v);
    }
    urls_from_github_api(family)
}

fn urls_from_github_api(family: &str) -> Result<Vec<(&'static str, String)>, String> {
    let dir = github_dir(family);
    let mut err = String::from("family not found in google/fonts");
    for lic in ["ofl", "apache", "ufl"] {
        let list = |sub: &str| -> Result<Vec<(String, String, String)>, String> {
            let url = format!("https://api.github.com/repos/google/fonts/contents/{lic}/{dir}{sub}");
            let v: serde_json::Value = serde_json::from_str(&http_get_text(&url)?).map_err(|e| e.to_string())?;
            let arr = v.as_array().ok_or("unexpected response")?;
            Ok(arr
                .iter()
                .filter_map(|e| Some((e.get("name")?.as_str()?.to_string(), e.get("type")?.as_str()?.to_string(), e.get("download_url").and_then(|u| u.as_str()).unwrap_or("").to_string())))
                .collect())
        };
        let top = match list("") {
            Ok(t) => t,
            Err(e) => {
                err = e;
                continue;
            }
        };
        let mut files: Vec<(String, String)> = top.iter().filter(|e| e.1 == "file").map(|e| (e.0.clone(), e.2.clone())).collect();
        let has_static_slots = files.iter().any(|f| classify_file(&f.0).is_some());
        if !has_static_slots && top.iter().any(|e| e.1 == "dir" && e.0 == "static") {
            if let Ok(st) = list("/static") {
                files.extend(st.into_iter().filter(|e| e.1 == "file").map(|e| (e.0, e.2)));
            }
        }
        let mut out: Vec<(&'static str, String)> = vec![];
        for slot in SLOTS {
            if let Some(f) = files.iter().find(|f| classify_file(&f.0) == Some(slot) && !f.1.is_empty()) {
                out.push((slot, f.1.clone()));
            }
        }
        if out.is_empty() {
            // variable-only family: take the default instance of the variable files
            let var: Vec<&(String, String)> = files.iter().filter(|f| f.0.to_lowercase().ends_with(".ttf") && f.0.contains('[')).collect();
            if let Some(f) = var.iter().find(|f| !f.0.contains("Italic")) {
                out.push(("Regular", f.1.clone()));
            }
            if let Some(f) = var.iter().find(|f| f.0.contains("Italic")) {
                out.push(("Italic", f.1.clone()));
            }
        }
        if !out.is_empty() {
            return Ok(out);
        }
        err = "no TTF files found".into();
    }
    Err(err)
}

fn looks_like_font(data: &[u8]) -> bool {
    ttf_parser::Face::parse(data, 0).is_ok()
}

fn is_variable(data: &[u8]) -> bool {
    ttf_parser::Face::parse(data, 0).map(|f| f.is_variable()).unwrap_or(false)
}

/// Download a family into `dir` (files: Regular.ttf, Bold.ttf, ...). Returns a human-readable note.
pub fn download_family(family: &str, api_key: Option<&str>, dir: &Path) -> Result<String, String> {
    let mut errors: Vec<String> = vec![];
    let mut sources: Vec<(&str, Box<dyn Fn() -> Result<Vec<(&'static str, String)>, String>>)> = vec![];
    if let Some(k) = api_key.filter(|k| !k.trim().is_empty()) {
        let (f, k) = (family.to_string(), k.to_string());
        sources.push(("Google Fonts API", Box::new(move || urls_from_api(&f, &k))));
    }
    {
        let f = family.to_string();
        sources.push(("Google Fonts CSS", Box::new(move || urls_from_css2(&f))));
    }
    {
        let f = family.to_string();
        sources.push(("GitHub google/fonts", Box::new(move || urls_from_github(&f))));
    }
    let mut best: Option<(String, Vec<(&'static str, Vec<u8>)>)> = None;
    for (name, f) in sources {
        match f() {
            Ok(slots) => {
                let mut got: Vec<(&'static str, Vec<u8>)> = vec![];
                for (slot, url) in slots {
                    match http_get(&url) {
                        Ok(d) if looks_like_font(&d) => got.push((slot, d)),
                        Ok(_) => errors.push(format!("{name}: {slot} is not a font file")),
                        Err(e) => errors.push(format!("{name}: {e}")),
                    }
                }
                if got.iter().any(|g| g.0 == "Regular") {
                    let variable = got.iter().any(|g| is_variable(&g.1));
                    let complete = got.len();
                    let better = best.as_ref().map(|b| !variable && b.1.iter().any(|x| is_variable(&x.1)) || complete > b.1.len()).unwrap_or(true);
                    if better {
                        best = Some((name.to_string(), got));
                    }
                    if !variable {
                        break; // static TTFs: done
                    }
                }
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    let (src, files) = best.ok_or_else(|| format!("Could not download \"{family}\" ({})", errors.join("; ")))?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (slot, data) in &files {
        std::fs::write(dir.join(format!("{slot}.ttf")), data).map_err(|e| e.to_string())?;
    }
    Ok(format!("Installed {family} from {src} ({} styles)", files.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_parsing() {
        let css = "/* latin */\n@font-face {\n  font-family: 'Roboto';\n  font-style: normal;\n  font-weight: 400;\n  src: url(https://fonts.gstatic.com/s/roboto/v30/a.ttf) format('truetype');\n}\n@font-face {\n  font-family: 'Roboto';\n  font-style: normal;\n  font-weight: 700;\n  src: url(https://fonts.gstatic.com/s/roboto/v30/b.ttf) format('truetype');\n}\n@font-face {\n  font-family: 'Roboto';\n  font-style: italic;\n  font-weight: 400;\n  src: url('https://fonts.gstatic.com/s/roboto/v30/c.ttf') format('truetype');\n}\n";
        let faces = parse_css(css);
        assert_eq!(faces.len(), 3);
        assert_eq!(faces[1], CssFace { italic: false, weight: 700, url: "https://fonts.gstatic.com/s/roboto/v30/b.ttf".into() });
        assert!(faces[2].italic);
        let slots = slots_from(faces.into_iter().map(|f| (f.italic, f.weight, f.url)).collect());
        let names: Vec<&str> = slots.iter().map(|s| s.0).collect();
        assert_eq!(names, vec!["Regular", "Bold", "Italic"]);
    }

    #[test]
    fn variable_weight_ranges_use_first_number() {
        let css = "@font-face { font-style: normal; font-weight: 100 900; src: url(https://x/y.ttf); }";
        let f = parse_css(css);
        assert_eq!(f[0].weight, 100);
    }

    #[test]
    fn catalogs_parse() {
        let api = r#"{"kind":"webfonts#webfontList","items":[{"family":"Lato","variants":["regular","700","italic"],"category":"sans-serif"}]}"#;
        let c = parse_api_catalog(api).unwrap();
        assert_eq!(c[0].family, "Lato");
        assert_eq!(c[0].category, "Sans Serif");
        let meta = ")]}'\n{\"familyMetadataList\":[{\"family\":\"Lora\",\"category\":\"Serif\",\"fonts\":{\"400\":{},\"400i\":{},\"700\":{}}}]}";
        let c = parse_metadata_catalog(meta).unwrap();
        assert_eq!(c[0].family, "Lora");
        assert!(c[0].variants.contains(&"italic".to_string()));
    }

    #[test]
    fn file_classification() {
        assert_eq!(classify_file("Lato-BoldItalic.ttf"), Some("BoldItalic"));
        assert_eq!(classify_file("OpenSans[wdth,wght].ttf"), None);
        assert_eq!(classify_file("Foo-Regular.ttf"), Some("Regular"));
        assert_eq!(github_dir("Playfair Display"), "playfairdisplay");
    }

    /// Live network test (run with `cargo test -- --ignored`). Falls back to GitHub when
    /// Google's own servers are unreachable.
    #[test]
    #[ignore]
    fn live_download_roundtrip() {
        let dir = std::env::temp_dir().join("wordrs-test-lato");
        let _ = std::fs::remove_dir_all(&dir);
        let note = download_family("Lato", None, &dir).expect("download failed");
        println!("{note}");
        for f in ["Regular.ttf", "Bold.ttf", "Italic.ttf", "BoldItalic.ttf"] {
            let data = std::fs::read(dir.join(f)).unwrap_or_else(|_| panic!("missing {f}"));
            assert!(ttf_parser::Face::parse(&data, 0).is_ok(), "{f} is not a valid font");
        }
        // a variable-only family should still give us at least a regular face
        let dir2 = std::env::temp_dir().join("wordrs-test-opensans");
        let _ = std::fs::remove_dir_all(&dir2);
        let note2 = download_family("Open Sans", None, &dir2).expect("open sans failed");
        println!("{note2}");
        assert!(dir2.join("Regular.ttf").exists());
    }

    #[test]
    fn bundled_list_is_sorted_and_unique() {
        let l = bundled_catalog();
        assert!(l.len() > 200);
        assert!(l.windows(2).all(|w| w[0].family.to_lowercase() <= w[1].family.to_lowercase()));
    }
}

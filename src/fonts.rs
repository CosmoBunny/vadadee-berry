use std::collections::HashSet;

use egui::{Context, FontData, FontDefinitions, FontFamily};
use fontdb::{Database, Family, Query};

pub const DEFAULT_FONT: &str = "Noto Sans";

pub struct FontRegistry {
    families: Vec<String>,
    loaded: HashSet<String>,
    db: Database,
}

impl FontRegistry {
    pub fn new() -> Self {
        let mut db = Database::new();
        db.load_system_fonts();
        let mut families: Vec<String> = db
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();
        families.sort_unstable_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        families.dedup();
        if !families.iter().any(|f| f == DEFAULT_FONT) {
            families.insert(0, DEFAULT_FONT.to_string());
        }
        Self {
            families,
            loaded: HashSet::new(),
            db,
        }
    }

    pub fn families(&self) -> &[String] {
        &self.families
    }

    pub fn default_family(&self) -> String {
        self.families
            .iter()
            .find(|f| f.as_str() == DEFAULT_FONT)
            .or_else(|| self.families.first())
            .cloned()
            .unwrap_or_else(|| DEFAULT_FONT.to_string())
    }

    pub fn query_face_bytes(&self, family: &str, bold: bool, italic: bool) -> Option<Vec<u8>> {
        let weight = if bold {
            fontdb::Weight::BOLD
        } else {
            fontdb::Weight::NORMAL
        };
        let style = if italic {
            fontdb::Style::Italic
        } else {
            fontdb::Style::Normal
        };
        let query = Query {
            families: &[Family::Name(family)],
            weight,
            stretch: fontdb::Stretch::Normal,
            style,
        };
        let id = self.db.query(&query)?;
        self.db
            .with_face_data(id, |data, _| data.to_vec())
    }

    pub fn ensure_loaded(&mut self, ctx: &Context, family: &str) {
        if self.loaded.contains(family) {
            return;
        }
        let query = Query {
            families: &[Family::Name(family)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        
        let key = family.to_string();
        let mut defs = ctx.fonts(|f| f.definitions().clone());
        
        if let Some(id) = self.db.query(&query) {
            if let Some(bytes) = self.db.with_face_data(id, |data, _| data.to_vec()) {
                defs.font_data.insert(key.clone(), FontData::from_owned(bytes).into());
                defs.families
                    .entry(FontFamily::Name(key.clone().into()))
                    .or_default()
                    .push(key.clone());
                ctx.set_fonts(defs);
                self.loaded.insert(key);
                return;
            }
        }
        
        // Fallback: Bind the font family name to egui's default proportional fonts list so it never crashes.
        let fallback_fonts = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        defs.families.insert(FontFamily::Name(key.clone().into()), fallback_fonts);
        ctx.set_fonts(defs);
        self.loaded.insert(key);
    }
}
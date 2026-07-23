use std::collections::HashSet;
use std::sync::{Arc, OnceLock};

use egui::{Context, FontData, FontDefinitions, FontFamily, FontId};
use fontdb::{Database, Family, Query};

use crate::icons;

pub const DEFAULT_FONT: &str = "Noto Sans";

pub struct FontRegistry {
    families: Vec<String>,
    loaded: HashSet<String>,
    db: Database,
    /// Accumulates font definitions across multiple `ensure_loaded` calls in the same
    /// frame. egui applies `set_fonts` asynchronously; cloning *active* defs between
    /// successive loads would drop earlier families and leave `FontFamily::Name` unbound.
    staged: Option<FontDefinitions>,
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
            staged: None,
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
        let family = sanitize_svg_font_family(family);
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
            families: &[Family::Name(&family)],
            weight,
            stretch: fontdb::Stretch::Normal,
            style,
        };
        let id = self.db.query(&query)?;
        self.db
            .with_face_data(id, |data, _| data.to_vec())
    }

    fn family_bound_in(defs: &FontDefinitions, fam: &FontFamily) -> bool {
        defs.families
            .get(fam)
            .map(|list| !list.is_empty())
            .unwrap_or(false)
    }

    fn family_bound_active(ctx: &Context, fam: &FontFamily) -> bool {
        ctx.fonts(|f| Self::family_bound_in(f.definitions(), fam))
    }

    /// Ensure `family` is registered with egui so `FontFamily::Name` never panics.
    /// Safe to call every frame; merges multiple loads in one frame without clobbering.
    pub fn ensure_loaded(&mut self, ctx: &Context, family: &str) {
        let key = sanitize_svg_font_family(family);
        if key.is_empty() {
            return;
        }
        let fam = FontFamily::Name(key.as_str().into());

        // Fully applied in the active font atlas.
        if Self::family_bound_active(ctx, &fam) {
            self.loaded.insert(key);
            // Staged is obsolete once active has the family; drop to pick up fresh base later.
            if let Some(staged) = self.staged.as_ref() {
                if Self::family_bound_in(staged, &fam) {
                    // Keep staged until all pending families apply — only clear when
                    // every staged Name family is also active (cheap: clear if active
                    // definitions equal-or-superset for our tracked keys).
                }
            }
            return;
        }

        // Merge into same-frame staged defs (never re-clone active mid-batch).
        let mut defs = self
            .staged
            .take()
            .unwrap_or_else(|| ctx.fonts(|f| f.definitions().clone()));

        if Self::family_bound_in(&defs, &fam) {
            // Already staged this frame but not active yet — re-apply and wait.
            ctx.set_fonts(defs.clone());
            self.staged = Some(defs);
            self.loaded.insert(key);
            ctx.request_repaint();
            return;
        }

        let query = Query {
            families: &[Family::Name(key.as_str())],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };

        if let Some(id) = self.db.query(&query) {
            if let Some(bytes) = self.db.with_face_data(id, |data, _| data.to_vec()) {
                let data_key = key.clone();
                defs.font_data
                    .insert(data_key.clone(), FontData::from_owned(bytes).into());
                defs.families.entry(fam.clone()).or_default().push(data_key);
                ctx.set_fonts(defs.clone());
                self.staged = Some(defs);
                self.loaded.insert(key);
                ctx.request_repaint();
                return;
            }
        }

        // Fallback: bind name to proportional stack so layout never panics.
        let fallback_fonts = defs
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        if fallback_fonts.is_empty() {
            // Absolute last resort — still bind something non-empty if possible.
            if let Some((name, _)) = defs.font_data.iter().next() {
                defs.families
                    .insert(fam, vec![name.clone()]);
            } else {
                // Leave unbound only if there is literally no font data; callers use
                // `resolved_family` which falls back to Proportional.
                self.staged = Some(defs);
                return;
            }
        } else {
            defs.families.insert(fam, fallback_fonts);
        }
        ctx.set_fonts(defs.clone());
        self.staged = Some(defs);
        self.loaded.insert(key);
        ctx.request_repaint();
    }

    /// `FontFamily` safe for immediate egui layout. Uses `Name` only when active;
    /// otherwise Proportional (one frame after ensure_loaded until fonts rebuild).
    pub fn resolved_family(ctx: &Context, family: &str) -> FontFamily {
        let key = sanitize_svg_font_family(family);
        if key.is_empty() {
            return FontFamily::Proportional;
        }
        let fam = FontFamily::Name(key.as_str().into());
        if Self::family_bound_active(ctx, &fam) {
            fam
        } else {
            FontFamily::Proportional
        }
    }

    pub fn font_id(ctx: &Context, family: &str, size: f32) -> FontId {
        FontId::new(size.max(1.0), Self::resolved_family(ctx, family))
    }
}

static USVG_FONTDB: OnceLock<Arc<Database>> = OnceLock::new();

/// Strip quotes sometimes stored in project text styles (breaks usvg font matching).
pub fn sanitize_svg_font_family(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn register_nerd_font_aliases(db: &mut Database) {
    let bytes = include_bytes!("../assets/DaddyTimeMonoNerdFont-Regular.ttf").to_vec();
    let source = fontdb::Source::Binary(Arc::new(bytes));
    let ids = db.load_font_source(source);
    let Some(primary) = ids.first().copied() else {
        return;
    };
    let Some(template) = db.face(primary).cloned() else {
        return;
    };

    const ALIASES: &[&str] = &[
        "AnonymicePro Nerd Font",
        "Anonymice Pro Nerd Font",
        icons::FONT_NAME,
        "DaddyTimeMono Nerd Font",
        "DaddyTimeMonoNerdFont",
        // Common system Nerd Font family names (canvas text may reference these)
        "CaskaydiaCove Nerd Font",
        "CaskaydiaCove NF",
        "Cascadia Code",
        "Cascadia Mono",
    ];

    for alias in ALIASES {
        let query = Query {
            families: &[Family::Name(alias)],
            weight: fontdb::Weight::NORMAL,
            stretch: fontdb::Stretch::Normal,
            style: fontdb::Style::Normal,
        };
        if db.query(&query).is_some() {
            continue;
        }
        let mut info = template.clone();
        info.id = fontdb::ID::dummy();
        info.families = vec![(alias.to_string(), fontdb::Language::English_UnitedStates)];
        db.push_face_info(info);
    }
}

fn build_usvg_fontdb() -> Arc<Database> {
    let mut db = Database::new();
    db.set_sans_serif_family(DEFAULT_FONT);
    db.load_system_fonts();
    register_nerd_font_aliases(&mut db);
    Arc::new(db)
}

/// Shared [`usvg::Options`] with system fonts and embedded Nerd Font aliases for export/rasterize.
pub fn usvg_options() -> usvg::Options<'static> {
    let mut opt = usvg::Options::default();
    opt.fontdb = USVG_FONTDB.get_or_init(build_usvg_fontdb).clone();
    opt.font_family = DEFAULT_FONT.to_string();
    opt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_quotes() {
        assert_eq!(
            sanitize_svg_font_family("  \"CaskaydiaCove Nerd Font\"  "),
            "CaskaydiaCove Nerd Font"
        );
    }
}

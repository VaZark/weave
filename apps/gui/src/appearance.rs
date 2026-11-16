//! Portable, self-describing theme documents and the local imported-theme library.
use crate::{App, data};
use iced_adaptive_design::{
    self as iad,
    theme_config::{Document, Kind, Schema, ThemeConfig as ToolkitConfig},
};
use std::path::{Path, PathBuf};

const MAX_THEME_BYTES: u64 = 256 * 1024;
const BASE_THEME: &str = include_str!("../themes/base.ron");

fn schema(scheme: iad::Scheme) -> Schema {
    let mut schema = Schema::new()
        .text("theme", "name", "", "Theme display name")
        .text("theme", "key", "", "Stable lowercase theme key")
        .text("player", "artwork", "square", "square or disc")
        .boolean(
            "player",
            "radial-visualizer",
            false,
            "Live audio spectrum around circular artwork",
        )
        .boolean(
            "player",
            "ambient",
            false,
            "Tint surfaces with the current cover color",
        );
    for key in ["genre", "mood", "lang", "energy", "style", "custom"] {
        let style = data::category_style((key != "custom").then_some(key));
        schema = schema.color(
            "tags",
            key,
            &iad::color::to_hex(scheme.light_dark(
                iad::color::darken(style.color, 0.15),
                iad::color::lighten(style.color, 0.15),
            )),
            "Category and individual tag color",
        );
    }
    schema
}

/// A validated theme document compiled once for each scheme.
#[derive(Debug, Clone)]
pub struct ThemeConfig {
    light: ToolkitConfig,
    dark: ToolkitConfig,
}

impl ThemeConfig {
    pub fn for_scheme(&self, scheme: iad::Scheme) -> &ToolkitConfig {
        scheme.light_dark(&self.light, &self.dark)
    }
    /// Read a color from the effective cascade. Bare keys use `[colors]`;
    /// `tags:genre` addresses another declared color section.
    pub fn light_dark(
        &self,
        scheme: iad::Scheme,
        key: &str,
        default: Option<&str>,
    ) -> Option<iced::Color> {
        let (section, key) = key.split_once(':').unwrap_or(("colors", key));
        let config = self.for_scheme(scheme);
        if config.is_set(section, key) {
            return config.color(section, key);
        }
        if let Some(default) = default {
            return iad::color::parse(default);
        }
        if section == "colors" {
            // The toolkit's declared defaults are scheme-neutral; ask for this scheme's.
            iad::theme_config::palette_default(scheme, key)
        } else {
            config.color(section, key)
        }
    }
    pub fn text(&self, section: &str, key: &str) -> Option<&str> {
        self.light.text(section, key)
    }
    pub fn bool(&self, section: &str, key: &str) -> Option<bool> {
        self.light.bool(section, key)
    }
    fn warnings(&self) -> Vec<String> {
        self.light
            .warnings()
            .iter()
            .chain(self.dark.warnings())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    fn unknown(&self) -> Vec<(String, String)> {
        self.light
            .unknown()
            .iter()
            .chain(self.dark.unknown())
            .cloned()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

/// The document for one scheme: the base sections, with the scheme's `light:`/`dark:` sections
/// laid over them. Text that is not RON has no values here; `ThemeDocument::parse` reports it.
fn mode_doc(text: &str, scheme: iad::Scheme, full_schema: &Schema) -> Document {
    use iad::theme_config::ron::Value as R;
    let raw = Document::parse(text).unwrap_or_default();
    let mut doc = Document::default();
    for (section, key, value) in raw.entries() {
        // Old documents remain importable, but their scheme metadata no longer
        // overrides the desktop. New documents have no scheme field.
        let legacy_scheme = section == "theme"
            && key == "scheme"
            && matches!(value, R::String(s) if matches!(s.as_str(), "light" | "dark" | "system"));
        if !section.starts_with("light:") && !section.starts_with("dark:") && !legacy_scheme {
            doc.set(section, key, value.clone());
        }
    }
    let prefix = scheme.light_dark("light:", "dark:");
    for (section, key, value) in raw.entries() {
        if let Some(base) = section.strip_prefix(prefix) {
            // Mode-specific sections contain colors, leaving identity and behavior mode-neutral.
            if full_schema
                .get(base, key)
                .is_some_and(|spec| spec.ty.kind() == Kind::Color)
            {
                doc.set(base, key, value.clone());
            } else {
                doc.set(section, key, value.clone());
            }
        }
    }
    doc
}

fn build_config(text: &str, scheme: iad::Scheme) -> ToolkitConfig {
    let app_schema = schema(scheme);
    let full_schema = Schema::toolkit().with(&app_schema);
    // Resolve each layer's scheme before layering, so base dark colors cannot
    // outrank an imported skin's unmarked colors at draw time.
    let mut base = mode_doc(BASE_THEME, scheme, &full_schema);
    let overlay = mode_doc(text, scheme, &full_schema).to_text("", &full_schema);
    let build = |base: &Document| {
        let base = base.to_text("", &full_schema);
        ToolkitConfig::builder(crate::platforms::APP_ID)
            .schema(app_schema.clone())
            .files(false)
            .inheritance(iad::theme_config::Inheritance::OptOut)
            .toolkit_text(base)
            .app_text(overlay.clone())
            .build()
    };
    let cfg = build(&base);
    let mut palette = iad::Palette::new(scheme);
    cfg.apply_to_palette(&mut palette, scheme);
    // Primary became independent in the toolkit. Keep Weave's accent-following
    // default in the lower config layer, so explicit skin primary tokens still win.
    for (key, color) in [
        ("primary", palette.accent_bg_color),
        ("primary-fg", palette.accent_fg_color),
        ("primary-text", palette.accent_color),
    ] {
        base.set(
            "colors",
            key,
            iad::theme_config::ron::Value::String(iad::color::to_hex(color)),
        );
    }
    build(&base)
}

pub fn config(text: &str) -> ThemeConfig {
    ThemeConfig {
        light: build_config(text, iad::Scheme::Light),
        dark: build_config(text, iad::Scheme::Dark),
    }
}

#[derive(Debug, Clone)]
pub struct ThemeDocument {
    pub name: String,
    pub key: String,
    pub config: ThemeConfig,
    text: String,
}

impl ThemeDocument {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() as u64 > MAX_THEME_BYTES {
            return Err("Theme files must be smaller than 256 KiB".into());
        }
        // A file that is not RON is reported with where it went wrong.
        Document::parse(text)?;
        let cfg = config(text);
        if !cfg.warnings().is_empty() {
            return Err(cfg.warnings().join("; "));
        }
        if !cfg.unknown().is_empty() {
            return Err(format!("Unknown theme settings: {:?}", cfg.unknown()));
        }
        let name = cfg.text("theme", "name").unwrap_or("").trim().to_string();
        let key = cfg.text("theme", "key").unwrap_or("").to_string();
        if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
            return Err("theme name must contain 1–80 printable characters".into());
        }
        if key.is_empty()
            || key.len() > 64
            || !key.as_bytes()[0].is_ascii_lowercase()
            || !key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("theme key must start with a lowercase letter and contain only lowercase letters, numbers, and hyphens (up to 64 characters)".into());
        }
        if !matches!(cfg.text("player", "artwork"), Some("square" | "disc")) {
            return Err("player artwork must be square or disc".into());
        }
        Ok(Self {
            name,
            key,
            config: cfg,
            text: text.into(),
        })
    }
}

#[derive(Debug)]
pub struct ThemeLibrary {
    root: PathBuf,
    pub themes: Vec<ThemeDocument>,
    pub selected: Option<String>,
}

pub fn config_directory() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::platforms::APP_ID)
}

impl ThemeLibrary {
    pub fn load(root: PathBuf) -> Self {
        let mut themes = Vec::new();
        if let Ok(files) = std::fs::read_dir(root.join("themes")) {
            for entry in files.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "ron") {
                    if let Ok(theme) = read_document(&path) {
                        if path
                            .file_stem()
                            .is_some_and(|stem| stem == theme.key.as_str())
                        {
                            themes.push(theme);
                        }
                    }
                }
            }
        }
        themes.sort_by_key(|theme| theme.name.to_lowercase());
        let selected = std::fs::read_to_string(root.join("active-theme"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|key| themes.iter().any(|theme| &theme.key == key));
        Self {
            root,
            themes,
            selected,
        }
    }

    pub fn current(&self) -> Option<&ThemeDocument> {
        self.selected
            .as_ref()
            .and_then(|key| self.themes.iter().find(|theme| &theme.key == key))
    }

    pub fn import(&mut self, path: &Path) -> Result<String, String> {
        let theme = read_document(path)?;
        let dir = self.root.join("themes");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        atomic_write(&dir.join(format!("{}.ron", theme.key)), &theme.text)?;
        let key = theme.key.clone();
        self.themes.retain(|old| old.key != key);
        self.themes.push(theme);
        self.themes.sort_by_key(|theme| theme.name.to_lowercase());
        Ok(key)
    }

    pub fn select(&mut self, key: Option<String>) -> Result<(), String> {
        if key
            .as_ref()
            .is_some_and(|key| !self.themes.iter().any(|theme| &theme.key == key))
        {
            return Err("That theme is no longer available".into());
        }
        std::fs::create_dir_all(&self.root).map_err(|e| e.to_string())?;
        atomic_write(
            &self.root.join("active-theme"),
            key.as_deref().unwrap_or(""),
        )?;
        self.selected = key;
        Ok(())
    }
}

fn read_document(path: &Path) -> Result<ThemeDocument, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_THEME_BYTES {
        return Err("Choose a theme config file smaller than 256 KiB".into());
    }
    ThemeDocument::parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
}

fn atomic_write(path: &Path, text: &str) -> Result<(), String> {
    let temporary = path.with_extension("importing");
    std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}

pub async fn choose() -> Option<PathBuf> {
    // Do not filter by extension: a valid theme can have any filename. Validate
    // its contents after selection instead of hiding files in the native portal.
    let mut dialog = rfd::AsyncFileDialog::new().set_title("Import a Weave theme");
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
    let imported = config_directory().join("themes");
    if examples.is_dir() {
        dialog = dialog.set_directory(examples);
    } else if imported.is_dir() {
        dialog = dialog.set_directory(imported);
    }
    dialog
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf())
}

/// Resolve layout and paint from the same cascade without changing process globals.
pub fn resolved_config(
    desktop: &iad::config::Config,
    scheme: iad::Scheme,
    cfg: &ThemeConfig,
) -> iad::config::Config {
    let mut resolved = desktop.clone().with_theme_config(cfg.for_scheme(scheme));
    // OptOut resets desktop preferences; the app still owns its system/light/dark choice.
    resolved.color_scheme = iad::config::Pick::Fixed(scheme);
    resolved
}

pub fn theme(scheme: iad::Scheme, cfg: &ThemeConfig) -> iad::Theme {
    iad::Theme::from_config(&resolved_config(&iad::config::current(), scheme, cfg))
}

impl App {
    /// Only live applications (and isolated render fixtures) install toolkit globals.
    pub fn enable_runtime_appearance(&mut self) {
        self.runtime_appearance = true;
        self.sync_runtime_appearance();
    }

    pub fn sync_runtime_appearance(&self) {
        if self.runtime_appearance {
            iad::config::install(resolved_config(
                &iad::config::current(),
                self.effective_scheme(),
                &self.theme_config,
            ));
        }
    }

    pub fn apply_theme(&mut self) {
        self.theme_config = self
            .themes
            .current()
            .map(|theme| theme.config.clone())
            .unwrap_or_else(|| config(""));
        self.sync_runtime_appearance();
    }
    /// Scheme-aware color lookup with an explicit Rust-side fallback.
    pub fn light_dark(&self, key: &str, default: Option<&str>) -> Option<iced::Color> {
        self.theme_config
            .light_dark(self.effective_scheme(), key, default)
    }
    pub fn category_style(&self, key: Option<&str>) -> data::TagStyle {
        let mut style = data::category_style(key);
        let fallback = self.effective_scheme().light_dark(
            iad::color::darken(style.color, 0.15),
            iad::color::lighten(style.color, 0.15),
        );
        let key = key.unwrap_or("custom").to_lowercase();
        let key = if key == "language" { "lang" } else { &key };
        style.color = self
            .light_dark(&format!("tags:{key}"), Some(&iad::color::to_hex(fallback)))
            .unwrap_or(fallback);
        style
    }
    pub fn tag_style(&self, tag: &str) -> data::TagStyle {
        let mut style = self.lib.style_of(tag);
        let key = tag
            .strip_prefix('=')
            .unwrap_or(tag)
            .split_once(':')
            .map(|(key, _)| key);
        style.color = self.category_style(key).color;
        style
    }
}

#[cfg(test)]
pub const EXAMPLES: [&str; 4] = [
    include_str!("../themes/neon-arcade.ron"),
    include_str!("../themes/tokyo-lagoon.ron"),
    include_str!("../themes/mocha-pop.ron"),
    include_str!("../themes/sakura-soda.ron"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_base_restores_layout_metrics_and_skin_overrides_without_global_installation() {
        let desktop = iad::config::Config::default();
        let base = config("");
        for scheme in [iad::Scheme::Light, iad::Scheme::Dark] {
            assert!(base.warnings().is_empty(), "{:?}", base.warnings());
            assert!(base.unknown().is_empty(), "{:?}", base.unknown());
            let resolved = resolved_config(&desktop, scheme, &base);
            let metrics = resolved.metrics();
            assert_eq!(metrics.gutter, 16.0);
            assert_eq!(metrics.radius_m, 9.0);
            assert_eq!(metrics.radius_l, 12.0);
            assert_eq!(metrics.radius_window, 15.0);
            assert_eq!(metrics.headerbar_height, 47.0);
            assert_eq!(metrics.button_min_height, 34.0);
            assert_eq!(metrics.card_padding, iced::Padding::from(12.0));
            for (text, radius_m, radius_window) in [
                (EXAMPLES[0], 14.0, 20.0),
                (EXAMPLES[1], 8.0, 14.0),
                (EXAMPLES[2], 12.0, 18.0),
                (EXAMPLES[3], 18.0, 24.0),
            ] {
                let skin = config(text);
                let selected = resolved_config(&resolved, scheme, &skin);
                assert_eq!(selected.metrics().gutter, metrics.gutter);
                assert_eq!(selected.metrics().radius_m, radius_m);
                assert_eq!(selected.metrics().radius_window, radius_window);
                let painted = iad::Theme::from_config(&selected);
                assert_eq!(painted.metrics.radius_m, radius_m);
                assert_eq!(painted.metrics.radius_window, radius_window);
                let restored = resolved_config(&selected, scheme, &base);
                assert_eq!(restored.metrics().radius_window, metrics.radius_window);
                assert_eq!(restored.metrics().radius_m, metrics.radius_m);
            }
        }
    }

    #[test]
    fn primary_actions_follow_inherited_or_skin_accent_unless_explicitly_overridden() {
        for scheme in [iad::Scheme::Light, iad::Scheme::Dark] {
            for text in std::iter::once("").chain(EXAMPLES) {
                let cfg = config(text);
                let theme = theme(scheme, &cfg);
                let p = &theme.palette;
                for (primary, accent) in [
                    (p.primary_bg_color, p.accent_bg_color),
                    (p.primary_fg_color, p.accent_fg_color),
                    (p.primary_color, p.accent_color),
                ] {
                    assert_eq!(iad::color::to_hex(primary), iad::color::to_hex(accent));
                }
                assert_eq!(
                    iad::color::to_hex(p.primary_bg_color),
                    iad::color::to_hex(cfg.for_scheme(scheme).color("colors", "accent").unwrap())
                );
                let button =
                    iad::styles::button::suggested(&theme, iced::widget::button::Status::Active);
                assert_eq!(
                    button.background,
                    Some(iced::Background::Color(p.primary_bg_color))
                );
            }
            let cfg = config(
                r##"{
    "colors": {
        "accent": "#a92777",
        "primary": "#116633",
        "primary-fg": "#ffffff",
        "primary-text": "#113322",
    },
    "dark:colors": {
        "accent": "#ff6ac1",
        "primary": "#225599",
        "primary-fg": "#ffffee",
        "primary-text": "#aaddff",
    },
}
"##,
            );
            let theme = theme(scheme, &cfg);
            let p = &theme.palette;
            for (key, actual) in [
                ("primary", p.primary_bg_color),
                ("primary-fg", p.primary_fg_color),
                ("primary-text", p.primary_color),
            ] {
                assert_eq!(Some(actual), cfg.for_scheme(scheme).color("colors", key));
            }
            assert_ne!(p.primary_bg_color, p.accent_bg_color);
            let button =
                iad::styles::button::suggested(&theme, iced::widget::button::Status::Active);
            assert_eq!(
                button.background,
                Some(iced::Background::Color(p.primary_bg_color))
            );
        }
    }

    #[test]
    fn examples_follow_system_and_keep_every_color_adaptive_and_legible() {
        for text in EXAMPLES {
            let document = ThemeDocument::parse(text).unwrap();
            for (section, _, value) in Document::parse(text).unwrap().entries() {
                if matches!(
                    section,
                    "colors" | "tags" | "dark:colors" | "dark:tags" | "light:colors" | "light:tags"
                ) {
                    let ron_value = value;
                    let iad::theme_config::ron::Value::String(value) = ron_value else {
                        panic!("{section}: {ron_value:?} is not a color string");
                    };
                    assert!(iad::color::parse(value).is_some(), "{section}: {value}");
                }
            }
            for scheme in [iad::Scheme::Light, iad::Scheme::Dark] {
                let resolved = theme(scheme, &document.config);
                let s = resolved.semantic;
                for surface in [
                    s.window,
                    s.view,
                    s.card,
                    s.headerbar,
                    s.sidebar,
                    s.popover,
                    s.dialog,
                ] {
                    assert!(
                        iad::color::contrast_ratio(surface.bg, surface.fg) >= 4.5,
                        "{} {scheme:?}",
                        document.name
                    );
                }
            }
            let light = theme(iad::Scheme::Light, &document.config);
            let dark = theme(iad::Scheme::Dark, &document.config);
            assert!(
                iad::color::relative_luminance(light.semantic.window.bg)
                    > iad::color::relative_luminance(dark.semantic.window.bg)
            );
            assert_ne!(
                document
                    .config
                    .light_dark(iad::Scheme::Light, "tags:genre", None),
                document
                    .config
                    .light_dark(iad::Scheme::Dark, "tags:genre", None)
            );
        }
    }
    #[test]
    fn nullable_default_uses_toolkit_defaults_and_custom_variables_can_supply_a_fallback() {
        let cfg = config("");
        for scheme in [iad::Scheme::Light, iad::Scheme::Dark] {
            let native = theme(scheme, &cfg);
            assert_eq!(
                cfg.light_dark(scheme, "window-bg", None),
                Some(native.palette.window_bg_color)
            );
            assert_eq!(cfg.light_dark(scheme, "unknown-app-color", None), None);
            assert_eq!(
                cfg.light_dark(scheme, "unknown-app-color", Some("#12345680")),
                iad::color::parse("#12345680")
            );
        }
        let configured = config(r##"{
    "colors": {
        "accent": "#abcdef",
    },
}
"##);
        assert_eq!(
            configured.light_dark(iad::Scheme::Light, "accent", Some("#000")),
            iad::color::parse("#abcdef")
        );
    }

    #[test]
    fn mode_sections_cascade_over_defaults_independently_of_file_order() {
        let document = ThemeDocument::parse(r##"{
    "theme": {
        "name": "Cascade",
        "key": "cascade",
    },
    "dark:colors": {
        "window-bg": "#111111",
    },
    "light:colors": {
        "accent": "#123456",
    },
    "colors": {
        "window-bg": "#eeeeee",
        "window-fg": "#333333",
        "accent": "#456789",
    },
    "tags": {
        "genre": "#123456",
    },
    "dark:tags": {
        "genre": "#abcdef",
    },
}
"##).unwrap();
        let cfg = &document.config;
        let read =
            |scheme, key| iad::color::to_hex(cfg.light_dark(scheme, key, Some("#987654")).unwrap());
        assert_eq!(read(iad::Scheme::Light, "window-bg"), "#eeeeee");
        assert_eq!(read(iad::Scheme::Dark, "window-bg"), "#111111");
        assert_eq!(read(iad::Scheme::Light, "accent"), "#123456");
        assert_eq!(read(iad::Scheme::Dark, "accent"), "#456789");
        for scheme in [iad::Scheme::Light, iad::Scheme::Dark] {
            assert_eq!(read(scheme, "window-fg"), "#333333");
            assert_eq!(read(scheme, "missing"), "#987654");
        }
        assert_ne!(
            read(iad::Scheme::Light, "tags:genre"),
            read(iad::Scheme::Dark, "tags:genre")
        );
    }
    #[test]
    fn both_mode_overrides_are_validated_and_config_expressions_are_rejected() {
        for section in ["light:colors", "dark:colors"] {
            for value in ["not-a-color", "light-dark(#fff,#000)"] {
                assert!(
                    ThemeDocument::parse(&format!(
                        "{{ \"theme\": {{ \"name\": \"Test\", \"key\": \"test\" }}, {section:?}: {{ \"window-bg\": {value:?} }} }}"
                    ))
                    .is_err()
                );
            }
        }
        let old = ThemeDocument::parse(r##"{
    "theme": {
        "name": "Old",
        "key": "old",
        "scheme": "dark",
    },
}
"##).unwrap();
        assert_eq!(
            theme(iad::Scheme::Light, &old.config).scheme,
            iad::Scheme::Light
        );
        assert_eq!(
            theme(iad::Scheme::Dark, &old.config).scheme,
            iad::Scheme::Dark
        );
        let dark_variant = ThemeDocument::parse(r##"{
    "theme": {
        "name": "Legacy",
        "key": "legacy",
    },
    "colors": {
        "accent": "#123456",
    },
    "dark:colors": {
        "window-bg": "#181818",
    },
}
"##).unwrap();
        assert_eq!(
            theme(iad::Scheme::Dark, &dark_variant.config)
                .semantic
                .window
                .bg,
            iad::color::parse("#181818").unwrap()
        );
    }

    #[test]
    fn import_persists_identity_selection_and_reimport_updates_the_same_key() {
        let root = crate::import_picker::tests::temp();
        let source = root.join("arbitrary-filename.theme");
        std::fs::write(&source, EXAMPLES[0]).unwrap();
        let mut library = ThemeLibrary::load(root.join("settings"));
        let key = library.import(&source).unwrap();
        library.select(Some(key.clone())).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("settings/themes").join(format!("{key}.ron")))
                .unwrap(),
            EXAMPLES[0]
        );
        let reloaded = ThemeLibrary::load(root.join("settings"));
        assert_eq!(reloaded.selected.as_deref(), Some(key.as_str()));
        assert_eq!(reloaded.current().unwrap().name, "Neon Orbit");
        std::fs::write(&source, EXAMPLES[0].replace("Neon Orbit", "Orbit Remix")).unwrap();
        library.import(&source).unwrap();
        assert_eq!(library.themes.len(), 1);
        assert_eq!(library.current().unwrap().name, "Orbit Remix");
        library.select(None).unwrap();
        assert!(
            ThemeLibrary::load(root.join("settings"))
                .current()
                .is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn invalid_metadata_and_settings_are_rejected_before_import() {
        for text in [
            r##"{
    "theme": {
        "name": "Oops",
        "key": "../escape",
    },
}
"##,
            r##"{
    "theme": {
        "name": "Oops",
        "key": "valid",
        "scheme": "purple",
    },
}
"##,
            r##"{
    "theme": {
        "name": "Oops",
        "key": "valid",
    },
    "player": {
        "artwork": "triangle",
    },
}
"##,
            r##"{
    "colors": {
        "accent": "bad",
    },
}
"##,
            "",
        ] {
            assert!(ThemeDocument::parse(text).is_err());
        }
    }
}

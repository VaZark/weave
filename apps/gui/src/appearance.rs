//! Portable, self-describing theme documents and the local imported-theme library.
use crate::{App, data};
use iced_adaptive_design::{
    self as iad,
    theme_config::{Kind, Schema, ThemeConfig as ToolkitConfig},
};
use std::path::{Path, PathBuf};

const MAX_THEME_BYTES: u64 = 256 * 1024;

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
        // Toolkit schemas declare their native dark defaults separately.
        if section == "colors" && scheme.is_dark() {
            config.color("colors-dark", key)
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

fn build_config(text: &str, scheme: iad::Scheme) -> ToolkitConfig {
    let app_schema = schema(scheme);
    let full_schema = Schema::toolkit().with(&app_schema);
    let raw = iad::config::parse_keyfile(text);
    let mut values: std::collections::BTreeMap<_, _> = raw
        .iter()
        .filter(|((section, key), value)| {
            // Old documents remain importable, but their scheme metadata no longer
            // overrides the desktop. New documents have no scheme field.
            let legacy_scheme = section == "theme"
                && key == "scheme"
                && matches!(value.as_str(), "light" | "dark" | "system");
            !section.starts_with("light:")
                && !section.starts_with("dark:")
                && section != "colors-dark"
                && !legacy_scheme
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    // Keep the toolkit's older dark-color section as a lower-priority import alias.
    if scheme.is_dark() {
        for ((section, key), value) in &raw {
            if section == "colors-dark" {
                values.insert(("colors".into(), key.clone()), value.clone());
            }
        }
    }
    let prefix = scheme.light_dark("light:", "dark:");
    for ((section, key), value) in &raw {
        if let Some(base) = section.strip_prefix(prefix) {
            // Mode-specific sections contain colors, leaving identity and behavior mode-neutral.
            if full_schema
                .get(base, key)
                .is_some_and(|spec| spec.ty.kind() == Kind::Color)
            {
                values.insert((base.to_owned(), key.clone()), value.clone());
            } else {
                values.insert((section.clone(), key.clone()), value.clone());
            }
        }
    }
    let mut resolved = String::new();
    let mut previous = None;
    for ((section, key), value) in values {
        if previous.as_deref() != Some(section.as_str()) {
            resolved.push_str(&format!("\n[{section}]\n"));
            previous = Some(section);
        }
        resolved.push_str(&format!("{key}={value}\n"));
    }
    ToolkitConfig::builder(crate::platforms::APP_ID)
        .schema(app_schema)
        .files(false)
        .app_text(resolved)
        .build()
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
            return Err("[theme] name must contain 1–80 printable characters".into());
        }
        if key.is_empty()
            || key.len() > 64
            || !key.as_bytes()[0].is_ascii_lowercase()
            || !key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("[theme] key must start with a lowercase letter and contain only lowercase letters, numbers, and hyphens (up to 64 characters)".into());
        }
        if !matches!(cfg.text("player", "artwork"), Some("square" | "disc")) {
            return Err("[player] artwork must be square or disc".into());
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
                if path.extension().is_some_and(|ext| ext == "conf") {
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
        atomic_write(&dir.join(format!("{}.conf", theme.key)), &theme.text)?;
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

pub fn theme(scheme: iad::Scheme, cfg: &ThemeConfig) -> iad::Theme {
    let mut desktop = (*iad::config::current()).clone();
    desktop.color_scheme = iad::config::Pick::Fixed(scheme);
    let theme = iad::Theme::from_config(&desktop);
    let scheme = theme.scheme;
    theme.with_theme_config(cfg.for_scheme(scheme))
}

impl App {
    pub fn apply_theme(&mut self) {
        self.theme_config = self
            .themes
            .current()
            .map(|theme| theme.config.clone())
            .unwrap_or_else(|| config(""));
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
    include_str!("../themes/neon-arcade.conf"),
    include_str!("../themes/tokyo-lagoon.conf"),
    include_str!("../themes/mocha-pop.conf"),
    include_str!("../themes/sakura-soda.conf"),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn examples_follow_system_and_keep_every_color_adaptive_and_legible() {
        for text in EXAMPLES {
            let document = ThemeDocument::parse(text).unwrap();
            for ((section, _), value) in iad::config::parse_keyfile(text) {
                if matches!(
                    section.as_str(),
                    "colors" | "tags" | "dark:colors" | "dark:tags" | "light:colors" | "light:tags"
                ) {
                    assert!(iad::color::parse(&value).is_some(), "{section}: {value}");
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
        let configured = config("[colors]\naccent=#abcdef\n");
        assert_eq!(
            configured.light_dark(iad::Scheme::Light, "accent", Some("#000")),
            iad::color::parse("#abcdef")
        );
    }

    #[test]
    fn mode_sections_cascade_over_defaults_independently_of_file_order() {
        let document = ThemeDocument::parse("[theme]\nname=Cascade\nkey=cascade\n[dark:colors]\nwindow-bg=#111111\n[light:colors]\naccent=#123456\n[colors]\nwindow-bg=#eeeeee\nwindow-fg=#333333\naccent=#456789\n[tags]\ngenre=#123456\n[dark:tags]\ngenre=#abcdef\n").unwrap();
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
                        "[theme]\nname=Test\nkey=test\n[{section}]\nwindow-bg={value}\n"
                    ))
                    .is_err()
                );
            }
        }
        let old = ThemeDocument::parse("[theme]\nname=Old\nkey=old\nscheme=dark\n").unwrap();
        assert_eq!(
            theme(iad::Scheme::Light, &old.config).scheme,
            iad::Scheme::Light
        );
        assert_eq!(
            theme(iad::Scheme::Dark, &old.config).scheme,
            iad::Scheme::Dark
        );
        let legacy = ThemeDocument::parse("[theme]\nname=Legacy\nkey=legacy\n[colors]\naccent=#123456\n[colors-dark]\nwindow-bg=#181818\n").unwrap();
        assert_eq!(
            theme(iad::Scheme::Dark, &legacy.config).semantic.window.bg,
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
            std::fs::read_to_string(root.join("settings/themes").join(format!("{key}.conf")))
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
            "[theme]\nname=Oops\nkey=../escape\n",
            "[theme]\nname=Oops\nkey=valid\nscheme=purple\n",
            "[theme]\nname=Oops\nkey=valid\n[player]\nartwork=triangle\n",
            "[colors]\naccent=bad\n",
            "",
        ] {
            assert!(ThemeDocument::parse(text).is_err());
        }
    }
}

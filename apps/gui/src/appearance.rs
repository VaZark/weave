//! Portable, self-describing theme documents and the local imported-theme library.
use crate::{App, data};
use iced_adaptive_design::{
    self as iad,
    theme_config::{Schema, ThemeConfig},
};
use std::path::{Path, PathBuf};

const MAX_THEME_BYTES: u64 = 256 * 1024;

fn schema() -> Schema {
    let mut schema = Schema::new()
        .text("theme", "name", "", "Theme display name")
        .text("theme", "key", "", "Stable lowercase theme key")
        .text("theme", "scheme", "system", "light, dark, or system")
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
            &iad::color::to_hex(style.color),
            "Category and individual tag color",
        );
    }
    schema
}

pub fn config(text: &str) -> ThemeConfig {
    ThemeConfig::builder(crate::platforms::APP_ID)
        .schema(schema())
        .files(false)
        .app_text(text)
        .build()
}

#[derive(Debug, Clone)]
pub struct ThemeDocument {
    pub name: String,
    pub key: String,
    pub scheme: Option<iad::palette::Scheme>,
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
        let scheme = match cfg.text("theme", "scheme").unwrap_or("system") {
            "system" => None,
            "light" => Some(iad::palette::Scheme::Light),
            "dark" => Some(iad::palette::Scheme::Dark),
            _ => return Err("[theme] scheme must be light, dark, or system".into()),
        };
        if !matches!(cfg.text("player", "artwork"), Some("square" | "disc")) {
            return Err("[player] artwork must be square or disc".into());
        }
        Ok(Self {
            name,
            key,
            scheme,
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

pub fn theme(scheme: Option<iad::palette::Scheme>, cfg: &ThemeConfig) -> iad::Theme {
    let mut desktop = (*iad::config::current()).clone();
    if let Some(scheme) = scheme {
        desktop.color_scheme = iad::config::Pick::Fixed(scheme);
    }
    iad::Theme::from_config(&desktop).with_theme_config(cfg)
}

impl App {
    pub fn apply_theme(&mut self) {
        self.theme_config = self
            .themes
            .current()
            .map(|theme| theme.config.clone())
            .unwrap_or_else(|| config(""));
        self.theme_scheme = self.themes.current().and_then(|theme| theme.scheme);
    }
    pub fn category_style(&self, key: Option<&str>) -> data::TagStyle {
        let mut style = data::category_style(key);
        let key = key.unwrap_or("custom").to_lowercase();
        let key = if key == "language" { "lang" } else { &key };
        if self.theme_config.is_set("tags", key) {
            if let Some(color) = self.theme_config.color("tags", key) {
                style.color = color;
            }
        }
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
    fn examples_announce_their_identity_and_keep_text_legible() {
        for text in EXAMPLES {
            let document = ThemeDocument::parse(text).unwrap();
            let theme = theme(document.scheme, &document.config);
            let s = theme.semantic;
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
                    "{}",
                    document.name
                );
            }
        }
    }
    #[test]
    fn import_persists_identity_selection_and_reimport_updates_the_same_key() {
        let root = crate::import_picker::tests::temp();
        let source = root.join("arbitrary-filename.theme");
        std::fs::write(&source, EXAMPLES[0]).unwrap();
        let mut library = ThemeLibrary::load(root.join("settings"));
        let key = library.import(&source).unwrap();
        library.select(Some(key.clone())).unwrap();
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

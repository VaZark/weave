//! What the toolkit cannot know about Weave on each platform. The generic part (the window's app
//! id, the `.desktop` text) is `iced_adaptive_design::platform`; the files to install live in the
//! repo's `platforms/` folder.

use iced_adaptive_design::platform::linux::DesktopEntry;

/// The window's app id, and the basename of the installed `.desktop` file and icon.
pub const APP_ID: &str = "io.github.vazark.Weave";

/// Weave's `.desktop` entry, which `platforms/linux/install.sh` installs.
pub const DESKTOP_ENTRY: DesktopEntry<'static> = DesktopEntry {
    app_id: APP_ID,
    name: "Weave",
    comment: "Play your music",
    exec: "weave-player-gui",
    icon: APP_ID,
    categories: &["AudioVideo", "Audio", "Player"],
};

#[cfg(test)]
mod tests {
    use super::*;
    use iced_adaptive_design::platform::linux::valid_app_id;

    #[test]
    fn the_app_id_is_valid_and_names_the_installed_files() {
        assert!(valid_app_id(APP_ID));
        let icon = format!(
            "{}/../../platforms/linux/icon/{APP_ID}.svg",
            env!("CARGO_MANIFEST_DIR")
        );
        assert!(std::path::Path::new(&icon).exists(), "missing {icon}");
    }
}

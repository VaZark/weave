# platforms

What Weave ships per platform that the toolkit (`iced-adaptive-design`) cannot know: the icon, the
install scripts, the Flatpak manifest. The generic part (the window's app id, the `.desktop` text) is
in the toolkit's `platform` module, configured from `apps/gui/src/platforms.rs`.

## Installation

Linux, Wayland only. The window's app id equals the `.desktop` file name
(`io.github.vazark.Weave`), which is how GNOME Resources shows Weave as an app instead of a bare
process. `cargo run` shows a bare process; install it to get the app.

**From source**, into `~/.local` (or another prefix as the argument):

```sh
cargo build --release -p weave-player-gui
platforms/linux/scripts/install.sh
platforms/linux/scripts/uninstall.sh   # removes it; your library and settings stay
```

**Flatpak**, built locally (needs `flatpak-builder` and the runtime, SDK and
`org.freedesktop.Sdk.Extension.rust-stable` at 26.08):

```sh
flatpak-builder --user --install --force-clean build-dir platforms/linux/flatpak/io.github.vazark.Weave.yml
flatpak run io.github.vazark.Weave
flatpak uninstall --user io.github.vazark.Weave
```

# Importable Weave player skins

Open **Main menu → Themes… → Import theme…**, choose a theme file, and its declared name
appears in the selector. The imported theme is activated and remembered across
restarts. **Desktop theme** restores the standard appearance. There is no built-in
preset list and no environment variable needed to select a theme.

Import these examples from this directory:

- `neon-arcade.conf` → **Neon Orbit**: circular artwork, a live radial spectrum,
  plum surfaces, pink, cyan and violet accents.
- `tokyo-lagoon.conf` → **Amber Tide**: teal surfaces, a circular spectrum, and
  ambient surface tinting from the current cover's color.
- `mocha-pop.conf` → **Studio Velvet**: a restrained square-artwork player and
  violet-charcoal surfaces.
- `sakura-soda.conf` → **Cotton Candy**: a light rose/porcelain player with a disc
  and raspberry spectrum ring.

The chooser shows all files; extensions do not determine whether a theme is valid.
When running from a development checkout it opens the example themes folder.

Every theme announces its own identity. The filename does not control the label
or identity:

```ini
[theme]
name=My Night Player
key=my-night-player
scheme=dark

[player]
artwork=disc
radial-visualizer=true
ambient=true

[colors]
accent=#ff6ac1
window-bg=#18112b
window-fg=#f6efff
view-bg=#211532
view-fg=#f6efff
card-bg=#302047
card-fg=#f6efff

[metrics]
radius-m=14
radius-l=18

[tags]
genre=#b892ff
mood=#ff83c9
lang=#64e6cf
```

`name` is required (1–80 printable characters). `key` is required, starts with a
lowercase letter and contains lowercase letters, digits or hyphens (up to 64
characters). `scheme` is `light`, `dark` or `system` (default). Artwork is `square`
or `disc`. Effects default to off. All toolkit ThemeConfig color, typography and
metric tokens are available; category colors are in `[tags]`.

Imports validate metadata and token types before writing. Files must be UTF-8
and under 256 KiB. Themes are inert config data, not executable plugins. Imported
copies live in `$XDG_CONFIG_HOME/io.github.vazark.Weave/themes/<key>.conf`
(normally `~/.config/...`); selection lives in `active-theme` beside that folder.
Reimporting the same key updates that theme instead of adding a duplicate.
Edit a source file and reimport it to try changes without rebuilding.

The spectrum uses Weave's existing audio sample tap. It decays when playback
stops and respects reduced-motion settings. The player currently uses generated
cover placeholders; disc skins round the existing artwork source. Ambient tint
uses that source's cover color. These skins do not add embedded-art extraction.
Responsive collection modes and playback/filter state are independent of skins.

Design references (inspiration only; no app assets or source copied):
[Amberol](https://apps.gnome.org/Amberol/),
[Lark Player](https://www.larkplayer.com/),
[Poweramp](https://powerampapp.com/),
[Musicolet](https://krosbits.in/musicolet/),
[Muzio](https://www.muzioplayer.com/), and
[InShot Music Player](https://play.google.com/store/apps/details?id=musicplayer.musicapps.music.mp3player).

Tests cover metadata, rejected files, import/reimport, persisted selection, theme
switching, text contrast, and wide/narrow Now Playing renders. Set
`WEAVE_THEME_REVIEW_DIR` to an existing directory when running the
`theme_presets_render` test to export screenshots.

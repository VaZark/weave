# Importable player skins

Palette-only built-in options do not express the intended music-player skins.
Use portable ThemeConfig documents with required `[theme] name` and `key`, mode-specific
color sections over common defaults, and optional player presentation settings.
The selector contains imported documents plus Desktop. A system chooser imports
one document; validation happens before an atomic copy into the app's own config
folder. The stable key controls storage and reimport identity, while the declared
name controls display. Persist the selected key independently of music state.
Invalid imports preserve the active theme. No engine/database schema changes.

Keep a bounded presentation contract: square/disc artwork, an optional radial
spectrum and optional cover tint. Use the existing real audio tap; do not simulate
reactive sound. Only visible effects subscribe to spectrum ticks, and reduced
motion disables the moving ring. Cover extraction remains separate; the current
artwork source is the existing generated placeholder. Configs supply values to
shared views, without executable themes or theme-specific player forks.

Adaptive color resolution and system-following example palettes are specified in [ADR 0021](0021-adaptive-theme-colors.md).

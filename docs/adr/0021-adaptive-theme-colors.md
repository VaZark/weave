# Adaptive theme color sections

Desktop theme notifications cannot change a skin whose scheme and every palette
color are fixed. Store the detected desktop scheme in app state and select the
matching color cascade before drawing. The session Dark mode opt-in overrides
that detected scheme; theme documents no longer declare a scheme.

`[colors]` supplies defaults. `[light:colors]` and `[dark:colors]` override only
specified keys in their respective modes, regardless of file order. The same
prefixes work for other declared color sections such as `[tags]`. Values stay
plain colors; there are no expressions in theme files.

Compile and validate both cascades once on import using the pinned toolkit's
ThemeConfig. Rust reads the active cascade with `light_dark(key, None)` for
known toolkit colors or `light_dark(key, Some("#rrggbb"))` for an app-specific
fallback. Keys without a section address colors; `tags:genre` names an app section.
The helper returns None for an unknown key without a fallback. Imported text is
preserved verbatim, and the toolkit's old `colors-dark` section remains a
lower-priority compatibility alias. Valid legacy scheme metadata is ignored on import.

All example themes provide both palettes. Tests simulate successive desktop
notifications, validate both cascades and contrast, and cover missing keys,
nullable defaults, partial overrides, and application opt-in precedence.

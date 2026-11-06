# Application-owned icons

The toolkit now resolves installed XDG icons. Reusing those drawings would make
Weave's established transport, navigation and tag icons depend on the desktop theme.

Keep Weave's existing 16px stroke artwork in a separate `weave-icons` workspace
crate. Its default API exposes SVG assets without GUI dependencies; an optional
`iced` feature adapts them to the toolkit's custom `IconSource` and inherited
foreground rendering. The GUI uses those assets for its own actions, covers and
stored tag keys. Toolkit-owned chrome continues to follow the desktop theme.

This preserves the old appearance without adding bundled artwork back to the
toolkit or changing persisted icon keys. Replacing the artwork later only affects
this crate. The GUI pins toolkit revision `52dff3836dd27b05eb44d5e0636a6da3ad710535`;
the existing ignored local Cargo patch remains available for sibling development.

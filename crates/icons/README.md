# weave-icons

Weave-owned 16px SVG artwork. It preserves the player’s earlier stroke-based icon
set independently of installed XDG themes, including transport, navigation, tags,
and the library mark. The source assets were moved from the project's
`iced-libadwaita` revision `1c7c2e2`; they are maintained here rather than in the toolkit.

The default crate has no GUI dependencies: `Icon::Play.source()` returns the SVG.
With `features = ["iced"]`, `Icon::Play.view(20.0)` follows its parent’s foreground,
and any `Icon` converts to `iced_adaptive_design::IconSource` for menus, chips,
avatars and icon toggles. Explicit tinting is available through `colored`.

Desktop-owned icons, including toolkit window controls, remain system themed.

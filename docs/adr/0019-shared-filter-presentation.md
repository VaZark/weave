# Shared filter presentation and editor layout

Jam, page filters and playlist rules share fields but currently own separate
modal wrappers, heights and scrolling. This causes different behavior across
pages, including a dialog for playlist rules on narrow screens.

Use one generic responsive-dialog primitive: a bounded dialog on wide layouts,
a full-screen sheet on narrow layouts, with one padding/size/focus policy. Compose
it once at the window root, leaving page views independent of modal presentation.
Use a separate reusable FilterEditor layout with fixed header and actions and one
scrolling field area. Its context supplies the title, description, field messages,
and actions. Jam and page filters stay live; playlist drafts keep Save/Cancel and
the existing reviewed manual-membership changes. Inline creation fields keep using
the same field renderer. No engine filtering or persistence semantics change.

Derive the active filter context from the existing editor state, so responsive
resizing changes presentation without copying, clearing or committing a filter.
One close action handles the header, desktop backdrop and Escape. Tests cover all
contexts at both widths and draft cancellation versus live editing.

# Import opens the system chooser directly

The initial in-app mixed-path browser was rejected during user review. Import must
open the system file browser immediately, without an intermediate Weave page.

The Linux desktop portal exposes distinct file and directory modes and returns
selected URIs, not the directory currently being browsed. An empty file selection
cannot be confirmed as a folder import through this API. After discussing that
constraint, the user chose system file/folder modes over adding a native GTK browser.
See the [portal FileChooser contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.FileChooser.html).

Use the existing rfd dependency with two direct menu actions: Import files… opens
the multi-file chooser; Import folder… opens the folder chooser. The empty-library
action opens the folder chooser directly. Cancellation returns no paths and imports
nothing. No additional dependency or permission changes are needed.

The engine remains responsible for recursive scanning, canonical-path deduplication
and partial success. Pass confirmed paths directly to import_paths. Keep the result
summary and per-path failure report; remove all application-owned directory listing,
selection, navigation and pre-import confirmation UI. Guard against opening multiple
choosers or submitting a second batch while importing, and reject stale responses.

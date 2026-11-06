# Manual playlist order editing

Reuse the toolkit ReorderState and ReorderableList for a draft order of stable track
slugs. Only manual playlists expose the editor. Clearing a transient search and
presentation sort on entry ensures the editor always covers the complete saved
playlist. The header uses pencil/check/cross actions with tooltips; drag handles and
up/down actions change the draft. Save is unavailable until the draft changes.
Navigation or cancellation discards it. Playback continues independently.

Keep persistence in the engine/storage API. playlist_reorder validates that the
saved order still matches the draft's starting order and that the proposed order
contains each current member exactly once, then writes the order in one SQLite
transaction. No membership changes or queue updates occur. This avoids partially
saved multi-move operations and rejects concurrent playlist changes. A failed save
keeps the draft available; the user can cancel and reopen it.

No schema change or dependency is needed. Coverage checks cancellation, manual-only
editing, unchanged playback, atomic validation, stale drafts and persistence after
reopening the database. Visual review remains with the user.

Manual playlist creation is a full-screen page using the shared name validation and
filter editor. It starts empty and stores optional addition rules in the existing
filter_spec column. Unlike dynamic rules, these constrain the Add tracks picker;
they never evaluate over or hide curated membership. Kind and presence of rules
are independent, and filtered manual playlists retain all curation controls.

The picker uses engine search, shared ActionRows and existing playlist_add/remove
APIs. Its presentation follows existing dialogs and bounded narrow bottom sheets;
track menus expose the same remove operation. Membership editing is blocked during
an order draft to preserve its original-membership validation.

When changed rules would disqualify curated tracks, Save first shows a confirmation
dialog containing the affected count at every window width. Cancel changes nothing;
Confirm saves rules and removes exactly the reviewed memberships in one transaction.
The storage operation rechecks membership and eligible matches and rejects a stale
review. Playlist rules, membership, and library tracks are not partially updated.

CLI manual creation also accepts addition rules. Manual sources still contribute
curated membership to another playlist; addition rules are not applied to playback.
Source dependencies include references in manual addition rules so deletion detaches
them correctly. Existing rows retain their behavior and no schema migration or new
toolkit abstraction is needed.

Removing a selected tag also opens this confirmation when the affected count is
zero. Relaxing an AND rule can disqualify no tracks; the dialog reports zero rather
than implying that removing a tag necessarily removes members.

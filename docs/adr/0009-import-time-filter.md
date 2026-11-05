# Import-time filtering

Use a rolling `imported_within_days` condition in the existing FilterSpec, ANDed
with other conditions, using the canonical track added_at timestamp. Seven days
is the GUI preset; CLI and saved queries can use any positive number of days.
An absolute cutoff would freeze saved playlists; a ranked newest-N source would
change membership with library size. A relative duration keeps saved queries live.
Expose added_at on track results so snapshots use the same core cutoff calculation
without per-track detail queries. No schema migration or new dependency is needed.
The optional filter is backward compatible with saved definitions; the timestamp
is an additive API/JSON field. This can be removed without changing stored music.

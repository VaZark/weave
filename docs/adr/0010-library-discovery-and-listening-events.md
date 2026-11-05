# Discovery and measured listening

Keep collection search, tag-category browsing, import enumeration and Rediscover
in the library API. Extend existing FilterSpec for text and inactivity predicates;
reuse it for CLI, saved playlists and analytics exclusions. GUI presentation remains
separate. Mixed inputs are scanned once, canonical paths deduplicated, with a
structured partial-success report. Non-audio is ignored; inaccessible paths are
reported, and valid inputs still import. Existing single-path import remains strict.

Track starts in play_history are legacy discovery data, not listening measurements.
Add tables at open alongside play_history, without changing the squashed schema or
requiring reimport. Playback hosts create slug event IDs only after output starts,
report monotonic cumulative audible wall time, and close events with explicit reasons.
Persist incremental time segments for accurate range/day clipping; never infer time
from a seek position. Repeated reports are idempotent. Crashes retain only reported
time, and a replacement host interrupts the abandoned event at its last report.
Pauses create no listening segments. Sessions group output with under 30 minutes
of inactivity. Metadata/tag attribution uses the current canonical library.

Analytics are derived from events and segments, with UTC buckets and half-open
millisecond ranges. No old start counts are converted into measured listening.
Completion means the decoder reached EOF; completion rate, replay share and total
completed listens remain separate. Favorites rank explicit ratings, with measured
time as a tie-break. Most listened ranks measured time. The APIs expose chart data,
not rendered charts. No new dependencies, UI components or external services.

Alternatives: accounting from queue wall time counts time without an audio host;
position deltas count seeks; only lifetime aggregates cannot answer arbitrary ranges.
These additive tables and optional filter fields are reversible. The pre-0.1 API
adds typed results/commands; existing entry points remain available.

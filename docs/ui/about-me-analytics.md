# About me: listening analytics

The GUI trial app exposes **About me** beside the All playlists / artists / albums / tracks destinations in Explore. It opens a dedicated, control-free analytics page.

## Page contents

- Listening time, tracks played, top artist and top track.
- Period selector: last 24 hours, 7 days, 30 days, 1 year, and a custom number of days.
- Daily listening activity line chart.
- Artist listening-share chart.
- Genre distribution visualization.
- Favourite tags.
- Follow-on analytics: completion and skip rates, replay frequency, session duration, album completion, discovery and rediscovery, listening streaks, time-of-week heatmap, genre evolution and yearly comparisons.

## Data contract and current limitation

The GUI fixture currently has lifetime play counts and a last-played age in days. It does not have timestamped playback events or listened duration. Therefore this page intentionally labels its charts and period totals as illustrative. It must not be treated as accurate analytics.

The engine should eventually expose an append-only playback event stream with at least:

- stable track ID and event timestamp (UTC);
- playback start and end / listened duration;
- outcome (completed, skipped, interrupted);
- session ID;
- optional interaction events (pause, resume, seek, replay).

Period aggregates should be derived from those events. Count a play only after a documented threshold, exclude paused time from listening duration, and distinguish plays from unique tracks. Keep raw events as the source of truth; add cached read models only when profiling justifies them.

## Navigation and toolkit boundary

About me is application-level functionality in Weave, not a music-specific toolkit widget. The existing GUI uses the toolkit's generic layout, typography, buttons and surfaces. Any media identity/control composition used by Weave should remain an app/gallery example rather than becoming a toolkit-level opinionated media player.

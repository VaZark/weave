# Feature matrix

CLI and engine work is implemented together. Non-chart GUI changes are implemented **as one batch**;
visual review belongs to the user. “Implemented; user visual review” means code exists but has not had
interactive verification. Rendering stays immediately before charts.

| Feature | CLI | GUI | Engine/library |
| --- | --- | --- | --- |
| tag DSl | Pending | pending | pnending |
| Timestamped playback events, pauses/seeks/restarts | Done (`stats --events`) | — | Done |
| About me time ranges and exclusion filters | Done | Implemented; user visual review | Done |
| Listening overview and favorites/replay metrics | Done | Implemented; user visual review | Done |
| Window clipping and gradient overflow | — | Deferred until before charts | Toolkit task |
| Activity chart data, UTC buckets | Done | Chart pending | Done |
| Genre hierarchy/sunburst data | Done | Chart pending | Done |
| Artist listening share, top N + Other | Done | Chart pending | Done |
| Listening sessions/bubble chart data | Done | Chart pending | Done |
| Completion, replay and repeat frequency | Done | Chart pending | Done |

The GUI now retains full saved FilterSpec definitions and reads evaluated playlist
membership from the engine. Non-chart GUI changes are implemented as one batch;
compilation and behavior checks follow implementation. Visual review belongs to the
user. Charts and the rendering investigation scheduled immediately before charts
remain deferred.

The detailed requirements below remain the acceptance criteria. Backend completion does
not imply GUI completion. No historical listening time is inferred from old start counts.

Backend verification:

- Workspace tests pass with default features, excluding the two existing hardware audio tests
  (`--skip tests::audio::`); audio-device initialization is unavailable in this sandbox.
- The complete workspace also passed without default features. Playback accounting is tested
  against fake audio output, including pause, seek, completion, failed starts and restarts.
- CLI smoke tests passed on a temporary library, including partial imports and saved queries.
- CLI/engine Clippy passes with warnings denied. Workspace Clippy now passes with warnings denied after the GUI batch.
- Additive measured-history tables preserve existing libraries; old starts are not backfilled.

Tag filter page

weave-player search --or-group --tag-any genre:zouk,genre:bachata --tag-all mood:romantic --or-group --tag-all lang:tamil,tag:romantic,90s --bpm-max 90

Push the changes to vendored/weaver as it's original tag manager. no complex and nested query mgmt yet.

The tag filter in the dialog needs a + beside the tag icon for each group. on clicking it, the current selection will be grouped and show up on the input bar with a comma after it. (ChipInput  - mantine).

we can't have Show "> Categories custom" when we select a category. Just a < would be enough but put it at the start of the line just show more collapse chevron at the end of the line. Check if both chevreons are properly styled. hte eery increasing shading is out place.

Drop Recently imported and exclude tags; can we style the input to better fit with the app. The bg and shadows need some work too - that grey looks out of place. Also do something about the damn fat scrollbar

Import

1. [x] After importing, show a Recently imported filter in Jam/Search (rolling last 7 days).
2. [x] Import opens the system browser directly. User chose separate native file/folder modes after review of portal limitations; the custom import page is removed.
3. [x] Top bar settings popover reuses the bottom sheet menu in narrow mode.

Filter dialog

1. [x] Tag pickers default to three complete lines.
2. [x] Chip pickers share configurable line limits with an overflow-only chevron after the last chip on the final clamped line.
3. [x] Tag search has an inline category icon, unlimited category chips, child-tag choices, Custom for unkeyed tags and back navigation that preserves selection.

Explore page

1. The widgets for "All *" should also use the same layout as Recent playlist
2. Search bar should in all the "All *" pages. against jsut for discovery. add appropriate params in engine
3. Make everything on the Explore Page a Section divider with collapsible logic (make collapsible and the chevron a parameter with toggleCollapse being default action when collapsible + paramter to defaultCollapseState expanded/collapsed)
6. Drop Usage in explore page. Add an  "About me" Clickable Row
4. Rediscover's should be a another clickable row. Opens Favourite/Playlist like page with for music not listened in 180 days ordered by highly rated. Both paramters configurable via filter dialog
5. Put Explore after the "All *" block, folowed by rediscover, and most listened tracks/albums

About me (last priority)

1. Clicking "About me" should open MediaControl page but with no other controls besides filter icon. then content should be stats and graphs that we can come up with.
   - refactor Media control? MediaControl's should ony guarante Image/avatar, name, description. Everything else like after_name (used by tags in JAM), top right used by search and std controls block (play, filters, save playlist) are paramter based. remove it from the toolkit library but keep it in gallery as example.ame

2. UIdeas
   - global time filters for last 24hr, 1 week, month, year + custom duration
   - exclusion filters : same as dialog as search
   - Text bloc: Usage in hours, No of tracks, Fav artist, track, longest listening session, most replayed track, artist, tag

Rendering

1. Investigate why the gradient in Jam/Album page extend beyond the borders of the window. That should not be technically possible. invetigate and push the changes to the toolkit. the Toolkit should never allow that because it defines the borders of the window. An anything rendered beynd it's milits is never rendered

Charts (after rendering)

   - Daily activity chart: Hours listened scrollable by X-axis
   - Sun burst chart by genre
   - Pie chart: Listening share by artist
   - Usage heatmap-bubble chart : point per session, size for session duration and tint based on tags
   - Most Loyal to : how often do u stick to it to completion x no of times played


### Codex prompt

# Weave: Library UX, Explore, Import, Filtering and Listening Analytics

You are working on **Weave**, a Rust desktop music library application built with Iced and the `iced-libadwaita` toolkit.

Your task is to investigate and implement the following product and UI improvements in Weave. Weave is also the trial application for the toolkit, so use it to validate reusable toolkit capabilities where appropriate.

## Working principles

1. **Weave is the target application.** Do not create a separate implementation or make changes to unrelated projects.
2. Inspect the existing codebase, architecture, components, data models and current UI before making decisions.
3. Reuse existing abstractions wherever possible. Avoid introducing duplicate components or parallel interaction models.
4. Keep business logic and data transformation in the core. UI components should present data and handle presentation-level interactions.
5. Keep music-specific concepts in Weave. Only introduce or modify toolkit abstractions when the behaviour is genuinely reusable.
6. Respect the existing design system and responsive behaviour.
7. Do not introduce arbitrary dependencies without evaluating existing Iced capabilities and alternatives.
8. Preserve existing functionality and avoid unnecessary architectural rewrites.

## 1. Import experience

### Recently imported (completed)

After importing music, expose a **Recently imported** filter in the Jam/Search page.

Requirements:
- Make it a search source/filter, not an entirely separate browsing system.
- Use actual import timestamps.
- Support combining it with existing filters.
- Ensure newly imported items are discoverable immediately.

### Mixed file and folder selection

The import workflow should allow selecting files and folders together in a single operation.

Requirements:
- Support mixed file/folder selection.
- Recursively enumerate folders.
- Avoid importing duplicates.
- Handle inaccessible files and partial failures gracefully.
- Report import results clearly.

### Responsive settings menu (completed)

On narrow layouts, reuse the existing bottom-sheet menu presentation for the top-bar settings menu instead of opening a sidebar.

The menu's contents and actions should remain consistent between layouts. Only the presentation should change.

## 3. Filter dialog improvements

### Chip expansion

All chip collections should have a consistent line-clamping behaviour.

Requirements:
- Default maximum: three lines for tags, two for artists and albums.
- Reserve a downward chevron immediately after the last visible chip on the final clamped line when content overflows.
- Expand inline.
- Show an upward chevron after the expanded chips to collapse again.
- Make the line limit configurable.
- Apply consistently to tag filters and other chip collections.

### Category-based tag browsing

Improve the tag filter with category-based navigation.

Proposed interaction:

1. Display the regular tag filter and search input.
2. Add a tag/category icon on the right side of the input.
3. Clicking it switches to category browsing.
4. Display named categories as chips, without the normal line limit.
5. Selecting a category displays its child tags.
6. Provide a `Custom` category for unnamed tags.
7. Allow returning to the category list without losing selections.

Search should remain available in both modes.

Use the existing tag data model. Do not introduce a second tag-selection mechanism.

## 4. Explore page restructuring

### Consistent All pages

All `All *` buttons should use the same layout and interaction model as the existing Recent playlist.

### Search on every All page

Every `All *` page should have a search bar.

Search must not be limited to discovery.

Architecture:

- **GUI widget:** implements the input, loading state, results, empty state and interactions.
- **Weave:** supplies the search sources and maps results to music entities.
- Keep the search logic out of the GUI. Update the engine and cli

### Reusable collapsible sections

Everything on the Explore page should be structured as a section divider with optional collapsible behaviour.

The section component should support:
- `collapsible`
- `default_collapse_state` (`expanded` or `collapsed`)
- A configurable chevron.
- `toggle_collapse` as the default action when collapsible.
- Optional trailing actions.

The application should be able to own/persist collapse state when needed.

Do not make non-collapsible sections appear interactive.

### Explore navigation and ordering

Remove the Usage widget from Explore.

Add an **About me** clickable row.

Add a **Rediscover** clickable row.

Recommended ordering:

1. Library. (the "All *")
2. Explore.
3. Rediscover.
4. Most listened tracks.
5. Most listened albums.

### Rediscover page

Rediscover opens a collection page using the same layout as playlists and other music collections.

Default behaviour:
- Show music that has not been listened to in at least 180 days.
- Order by historical preference.
- Expose the inactivity duration and ranking criteria through the existing filter dialog.

Make both parameters configurable.

Ranking should account for more than raw play count. Consider historical listening behaviour, ratings and completion data where available.

Distinguish between:
- Not listened to recently.
- Previously played but rarely completed.
- Frequently replayed historically.

Do not invent data that the current model cannot provide. Ask me if it's easier to create a simpler model

## 5. MediaControl composition

Revisit the existing MediaControl implementation.

Its guaranteed contract should be limited to:

- Image/avatar.
- Name.
- Description.

Everything else should be caller-provided or parameterised:

- `after_name` content, such as JAM tags.
- Top-right content, such as search.
- Standard controls block, such as play, filters and save playlist.
- Other optional actions.

The goal is to turn MediaControl into a composable presentation primitive rather than an opinionated music-player component.

Remove the opinionated MediaControl implementation from the toolkit library, but retain a complete composition example in the toolkit gallery.

Do not move music-specific functionality into the toolkit.

Evaluate whether the existing toolkit slot/composition APIs are sufficient. Prefer reusable slot types over arbitrary callback complexity where possible.

For this trial application, implement and validate the resulting composition in Weave.

## 6. About me: listening analytics

Clicking **About me** should open a MediaControl-style page containing:
- Image/avatar.
- Name.
- Description.
- Filter action only.

Do not display the usual play, save or other media controls.

The rest of the page is dedicated to personal listening statistics and visualisations.

### Global filters

Provide global time filters:
- Last 24 hours.
- Last 7 days.
- Last month.
- Last year.
- Custom duration (daterange primitive custom widget).

All metrics and charts should respond to the same selected time range.

Reuse the existing exclusion filters from the search/filter dialog. Do not implement a separate filtering system.

### Overview metrics

Display:
- Total listening hours.
- Number of unique tracks listened to.
- Favourite artist.
- Favourite track.
- Longest listening session.
- Most replayed track.
- Most listened-to artist.
- Most listened-to tag.

Clearly define the ranking metric behind favourite and most-listened values.

### Rendering correctness: clipping and overflow (before charts)

Investigate the gradient in the Jam/Album page that currently extends beyond the window boundaries.

This should not happen.

Determine the actual cause before applying a fix. Investigate:
- Canvas or custom renderer behaviour.
- Layering and overlay composition.
- Gradients and shaders using window-relative coordinates.
- Widget transformations and layout bounds.
- Nested container clipping.
- Any rendering path that bypasses normal clipping.

### Toolkit responsibility

If the issue originates in `iced-libadwaita` or its rendering abstractions, fix it at the toolkit level.

The desired invariant is:

**Nothing should render outside its defined clipping boundary unless the component explicitly requests intentional overflow.**

Distinguish between:
- Normal content, which must be clipped.
- Shadows and popovers, which may intentionally extend beyond parent bounds.
- Explicit overlay or escape boundaries.

Do not simply clip everything globally and break popovers, shadows or menus.

Document the clipping contract and add regression coverage where possible.

### Visualisations

Implement the following, prioritising clarity and integration with the existing design system.

**Daily activity**
- Listening hours over time.
- Horizontally scrollable X-axis.
- Adapt resolution to the selected period.

**Genre sunburst**
- Hierarchical genre/tag distribution.
- Support drill-down where practical.

**Artist distribution**
- Pie chart showing listening share by artist.
- Support configurable top N and group the remainder into `Other`.

**Session bubble chart**
- One point per listening session.
- Bubble size represents session duration.
- Colour/tint represents tags or genres.
- Explore useful axis combinations, such as date and time of day.

**Listening loyalty**
Measure how often the user listens to tracks to completion, alongside replay behaviour.

Keep these metrics distinct:
- Completion rate.
- Replay rate.
- Repeat frequency.
- Total completed listens.

Avoid conflating a track that is frequently replayed with one that is consistently completed.

## 7. Playback analytics data foundation

Before implementing analytics, inspect the current playback and library data models.

Lifetime play counts alone are insufficient for accurate time-filtered statistics.

Determine whether the existing model can support:
- Daily listening duration.
- Session duration.
- Completion rate.
- Listening heatmaps.
- Arbitrary time ranges.
- Replay and repeat frequency.

If not, propose and implement a timestamped playback event model.

A potential event should contain:
- Track identifier.
- Started timestamp.
- Ended timestamp.
- Actual listened duration.
- Completion status.
- Interruption/reason where known.
- Session identifier.

The event stream should be the source of truth. Aggregate statistics should be derived from it.

Account for interrupted playback, seeking, pauses, duplicate events and application restarts. Avoid counting paused time as listening time.

Do not present fabricated analytics as real user data. If historical events are unavailable, show an appropriate empty state and explain that statistics will accumulate from the point tracking begins.

## 8. Implementation and validation workflow

Work in the following order:

1. Inspect the current repository and map the relevant architecture.
2. Identify existing components and abstractions that can be reused.
3. Implement import improvements.
4. Implement filter dialog improvements.
5. Implement Explore restructuring and search.
6. Refactor MediaControl composition.
7. Establish the playback event model.
8. Implement About me overview metrics after the data foundation.
9. Fix rendering/clipping immediately before implementing charts.
10. Implement analytics charts.

For each stage:
- Explain the relevant existing architecture.
- Identify affected modules.
- Describe the proposed changes.
- Implement the changes.
- Run formatting, compilation and relevant tests.
- Report what was verified and what remains unverified.

Do not claim a feature works merely because the code has been written.

## 9. Final deliverable

Provide:
- Summary of changes.
- Architectural decisions.
- Any new toolkit abstractions and why they are reusable.
- Data-model changes and migration implications.
- Tests and build results.
- Known limitations.
- Remaining work.

Prioritise correctness, coherent architecture and reuse over the number of features completed.

Do not sacrifice the toolkit's generality to make a single Weave screen easier to implement.

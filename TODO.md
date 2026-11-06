# Weave: Current Implementation Audit and Roadmap Reconciliation

The existing development roadmap is a historical record of requirements, not necessarily the current state of the application.

The local working tree contains newer implementations and refinements that may supersede parts of the roadmap.

**Treat the current local code as the best-known implementation state. Do not assume the roadmap is authoritative when it conflicts with implemented behavior.**

## Objectives

1. Inspect the current working tree, including uncommitted changes.
2. Understand the architecture, existing components, and implemented functionality.
3. Evaluate the roadmap against the current code.
4. Identify requirements that have already been implemented, refined, superseded, or made obsolete.
5. Identify duplicate functionality, unnecessary abstractions, and opportunities for simplification.
6. Recommend what to keep, modify, consolidate, remove, or implement.

## Rules

1. Do not compare Git branches or use remote Git history as the basis of this review.
2. Do not assume uncommitted or unpushed changes are incomplete.
3. Do not modify code during this audit.
4. Do not automatically implement roadmap requirements.
5. Preserve newer design refinements when they supersede older requirements.
6. Do not propose architectural rewrites unless a concrete problem justifies them.
7. Keep music-specific logic in Weave and reusable UI abstractions in `iced-libadwaita`.
8. Distinguish verified implementation facts from assumptions and recommendations.
9. Reference relevant source files and symbols when explaining findings.

## Evaluation process

For each feature in the roadmap:

1. **Roadmap intent:** What problem was the original requirement trying to solve?
2. **Current implementation:** What actually exists in the working tree?
3. **Later refinements:** Has the implementation evolved beyond the original requirement?
4. **Gaps:** What remains missing or broken?
5. **Recommendation:**
   - Keep as implemented.
   - Refine the existing implementation.
   - Consolidate with existing functionality.
   - Remove obsolete requirements or code.
   - Implement missing functionality.
   - Defer until needed.
6. **Impact:** Which Weave or toolkit modules would be affected?
7. **Dependencies:** Does this change require another feature or fix first? Does it require updating upstream features?

## Output

Start with a concise feature inventory and implementation status.

Then evaluate features individually, following the roadmap's functional groupings.

For each feature, provide evidence from the current code and a proposed direction, but **do not finalize design decisions on behalf of the user**.

Present the findings for collaborative review, one feature at a time.

Do not rewrite the entire roadmap yet. The revised TODO will be produced after the individual features have been discussed and decisions agreed upon.

# Weave: Roadmap

## 2. Working principles

1. Weave is the target application. Do not create parallel applications or unrelated implementations.
2. Inspect existing code, architecture, components, and data models before making changes.
3. Reuse existing abstractions rather than introducing duplicate components.
4. Keep business logic, filtering, and data transformations in the engine.
5. Keep presentation and presentation-level interactions in the GUI.
6. Implement corresponding CLI and engine functionality together when introducing new business capabilities.
7. Keep music-specific behavior in Weave.
8. Move reusable behavior into `iced-libadwaita` only when its generality is established.
9. Preserve the existing design system and responsive behavior.
10. Prefer Iced and existing toolkit functionality before introducing dependencies.
11. Do not infer missing historical listening data.
12. Preserve existing functionality and avoid unnecessary rewrites.

## 3. UI foundations and toolkit improvements

### 3.1 Responsive component composition

Review existing Weave widgets and identify repeated responsive patterns.

1. Compose reusable presentation primitives into higher-level components where appropriate.
2. Reuse the same logical component across different screen sizes.
3. Prefer changing presentation over duplicating interactions or application state.
4. Evaluate existing `iced-libadwaita` composition and slot APIs.
5. Suggest toolkit changes only when the behavior is reusable outside Weave.

Use responsive settings menus and collection pages as validation cases.

### 3.2 Animations and transitions

Investigate broken animations and transitions throughout the application.

1. Animations should be part of component activation and dismissal.
2. Avoid implementing independent animation logic in every consuming screen.
3. Inspect existing toolkit animation support before adding new mechanisms.
4. Ensure transitions are consistent across popovers, dialogs, bottom sheets, and navigation.
5. Handle interrupted, repeated, and reversed transitions correctly.

Where the issue originates in the toolkit, fix it there.

### 3.3 Clipping, overlays, and window boundaries

Investigate the rendering problems affecting:

- Jam and Album page gradients.
- Background tints extending beyond rounded window boundaries.
- Filter dialogs and bottom sheets overriding window corner radii.
- Overlays that incorrectly behave as fullscreen windows.

Establish the following rules:

1. The window owns its outer boundaries and corner radius.
2. Normal content must respect its clipping boundary.
3. A bottom sheet may occupy 100% of the available content height without becoming a fullscreen window.
4. Only an actual window mode change may override window-level decoration behavior.
5. Bottom sheets and tints may occupy different z-index positions while remaining within the same window boundary.
6. Shadows and popovers may intentionally overflow local component boundaries.
7. Separate windows have independent boundaries and are not subject to their parent window's clipping.
8. Do not introduce global clipping that breaks legitimate overlays.

Investigate the actual cause, including layout bounds, transformations, gradient coordinates, canvas rendering, overlay composition, and renderer clipping.

Fix toolkit-level defects in `iced-libadwaita`, document the clipping contract, and add regression coverage.

**This work must be completed before implementing analytics charts.**

## 4. Library exploration and navigation

### 4.1 Explore page structure

Restructure Explore into consistent, reusable sections.

1. Use a common section component for Explore content.
2. Support:
   1. Optional collapsibility.
   2. Initial expanded/collapsed state.
   3. Configurable chevron.
   4. Default collapse/expand interaction.
   5. Optional trailing actions.
   6. Application-owned persistent collapse state.
3. Non-collapsible sections must not appear interactive.
4. Remove the existing Usage widget.
5. Introduce navigation entries for About Me and Rediscover.

Preferred ordering:

1. Library (All collections).
2. Explore.
3. Rediscover.
4. Most listened tracks.
5. Most listened albums.

### 4.2 Consistent collection pages

All `All *` destinations should reuse the established Recent Playlist layout and interaction patterns.

Each page should have:

1. A consistent collection layout.
2. Search functionality.
3. Shared loading, empty, and results presentations.
4. Responsive behavior appropriate to the available width.

Keep search responsibilities separated:

- Engine and CLI: search capabilities and query evaluation.
- Weave: search sources and mapping results to music entities.
- GUI: input, loading state, results, empty state, and interactions.

### 4.3 Rediscover

Implement Rediscover as a collection page using the existing playlist/collection layout.

Default behavior:

1. Find music not listened to for at least 180 days.
2. Order by historical preference.
3. Allow the inactivity threshold and ranking criteria to be configured through the existing filter dialog.

Consider available historical signals:

- Listening frequency.
- Ratings.
- Completion.
- Replay behavior.

Distinguish music that has not been played recently, music that was rarely completed, and music that was previously replayed frequently.

Use only signals supported by the data model. Do not fabricate historical preferences.

If the available model cannot support a meaningful ranking, propose a simpler alternative before introducing new data structures.

### 4.4 Music import

Preserve the completed Recently Imported search capability.

For the import workflow:

1. Support selecting files and directories together when the underlying file-selection mechanism permits it.
2. Recursively enumerate selected directories.
3. Avoid duplicate imports.
4. Handle inaccessible files and partial failures.
5. Report imported, skipped, and failed items.
6. Make newly imported music discoverable immediately.

Keep Recently Imported as a search capability, but remove its dedicated control from the tag-filter dialog as requested by the newer UI notes.

### 4.5 Responsive settings menu

The narrow-layout settings menu already uses the existing bottom-sheet presentation.

Preserve this implementation and verify that:

1. The same actions appear across layouts.
2. State and behavior remain consistent.
3. The menu respects window clipping.
4. Activation and dismissal animations work correctly.

## 5. Tag filtering and query composition

### 5.1 Filter dialog cleanup

Simplify the filter dialog.

1. Remove the dedicated Recently Imported and Exclude Tags controls from the visible dialog.
2. Preserve existing engine-level filter capabilities unless explicitly deprecated.
3. Improve background, shadows, and surface styling.
4. Remove inconsistent grey surfaces and excessive progressive shading.
5. Refine scrollbar width and styling.
6. Keep the input visually consistent with the rest of Weave.
7. Preserve existing selections while navigating filter categories.

### 5.2 Chip collections

Use consistent chip wrapping and expansion behavior.

1. Default maximum visible lines:
   1. Tags: 3.
   2. Artists: 2.
   3. Albums: 2.
2. Make the line limit configurable.
3. Display a downward chevron after the final visible chip when content overflows.
4. Expand the existing collection inline.
5. Display an upward chevron to collapse.
6. Preserve chip selection and input state across expansion.
7. Reuse the same behavior across all chip collections.

Evaluate whether this should become a reusable toolkit component.

### 5.3 Category-based tag browsing

Keep the existing tag data model and selection mechanism.

1. Display the regular tag filter with search input.
2. Add a category/tag icon beside the input.
3. Allow switching between tag selection and category browsing.
4. Display categories as chips without the normal line limit.
5. Selecting a category displays its child tags.
6. Provide a Custom category for uncategorized tags.
7. Preserve selections when navigating between categories.
8. Keep search available in both modes.

Navigation and visual refinement:

1. Use a simple back chevron at the beginning of the category row.
2. Avoid verbose breadcrumb text such as `> Categories Custom`.
3. Place the show-more/collapse chevron at the end of the chip collection.
4. Keep the visual hierarchy and background shading consistent.

### 5.4 Tag grouping and ChipInput

Introduce grouping using the existing filter model where possible.

1. Add a `+` action beside the tag icon for each filter group.
2. Clicking it groups the current selection.
3. Represent the resulting group in the input bar.
4. Append a comma after the completed group to allow continued entry.
5. Evaluate a Mantine-style ChipInput interaction.
6. Maintain a single canonical filter representation.

Do not introduce a second independent query model in the GUI.

### 5.5 Tag query DSL

The advanced DSL remains pending.

Target syntax:

`weave-player search --or-group --tag-any genre:zouk,genre:bachata --tag-all mood:romantic --or-group --tag-all lang:tamil,tag:romantic,90s --bpm-max 90`

Requirements:

1. Define the grouping semantics explicitly before implementation.
2. Use the existing `FilterSpec` model where possible.
3. Implement parsing and evaluation in the engine.
4. Expose the capability through the CLI.
5. Have the GUI consume the same evaluated query model.
6. Avoid implementing a separate GUI query engine.
7. Defer complex nested query management until the basic grouping model is validated.
8. Push reusable tag-management changes to `vendored/weaver`, the original tag manager.

## 6. MediaControl composition

Refactor the existing MediaControl abstraction.

Its guaranteed contract should include only:

1. Image/avatar.
2. Name.
3. Description.

All other content should be optional and caller-provided:

1. `after_name`, such as Jam tags.
2. Top-right content, such as search.
3. Standard controls, including play, filter, and save actions.
4. Additional contextual actions.

Implementation requirements:

1. Use existing composition and slot APIs where possible.
2. Avoid music-specific assumptions in reusable toolkit primitives.
3. Prefer composable presentation primitives over increasingly large configuration objects.
4. Remove the opinionated MediaControl implementation from the toolkit library.
5. Retain a complete composition example in the toolkit gallery.
6. Validate the resulting composition in Weave.
7. Keep application-specific controls and behavior in Weave.

## 7. About Me: Listening analytics

**Status:** The analytics backend and overview GUI are implemented. The GUI requires visual review. Charts remain pending.

### 7.1 Page layout

About Me opens a MediaControl-style page containing:

1. Avatar/image.
2. Name.
3. Description.
4. Filter action.

Do not show normal media playback, save, or playlist controls.

The remainder of the page displays listening statistics and charts.

### 7.2 Global filters

Support the existing global time filters:

1. Last 24 hours.
2. Last 7 days.
3. Last month.
4. Last year.
5. Custom date range.

All displayed metrics and charts must respond to the same filter state.

Reuse existing exclusion-filter capabilities rather than introducing a separate analytics filtering system.

Evaluate the existing date-range primitive for the custom duration control.

### 7.3 Listening overview

Review the implemented UI for:

1. Total listening hours.
2. Unique tracks listened to.
3. Favorite artist.
4. Favorite track.
5. Longest listening session.
6. Most replayed track.
7. Most listened-to artist.
8. Most listened-to tag.

Clearly distinguish favorite rankings from raw listening frequency.

Do not infer missing historical listening time from legacy play counts.

### 7.4 Playback analytics foundation

The timestamped playback-event model and corresponding CLI/engine functionality are already implemented.

Preserve the event stream as the source of truth.

Validate that aggregation correctly handles:

1. Playback starts and ends.
2. Pauses and resumes.
3. Seeking.
4. Interrupted playback.
5. Application restarts.
6. Duplicate events.
7. Session boundaries.
8. Actual listening duration.
9. Completion and replay behavior.

Paused time must not count as listening time.

Historical periods without event data must display an appropriate empty state rather than invented statistics.

### 7.5 Charts

Implement charts only after resolving the rendering and clipping issues from Section 3.

#### Daily activity

1. Display listening hours over time.
2. Support horizontal scrolling.
3. Adapt time resolution to the selected period.
4. Use the implemented UTC bucket data while presenting dates appropriately.

#### Genre sunburst

1. Visualize hierarchical genre/tag distribution.
2. Reuse the existing genre hierarchy data.
3. Support drill-down where practical.

#### Artist distribution

1. Display listening share by artist.
2. Support configurable Top N.
3. Group remaining artists into Other.

#### Listening sessions

1. Render one bubble per listening session.
2. Use bubble size for session duration.
3. Use color/tint for associated tags or genres.
4. Explore useful axis combinations, such as date and time of day.

#### Listening loyalty

Keep the following metrics distinct:

1. Completion rate.
2. Replay rate.
3. Repeat frequency.
4. Total completed listens.

Do not conflate frequent replaying with consistently completing a track.

Charts must share the global About Me filters and follow the existing responsive design system.

## 8. Implementation order

### Phase 1: UI foundations

1. Inspect reusable composition patterns.
2. Fix broken component animations and transitions.
3. Investigate and resolve window clipping, tint, and bottom-sheet issues.
4. Validate toolkit-level fixes in Weave.

### Phase 2: Library and filtering

1. Complete filter dialog visual cleanup.
2. Implement consistent chip expansion.
3. Refine category-based tag browsing.
4. Implement basic tag grouping and ChipInput behavior.
5. Complete collection search and Explore restructuring.
6. Complete Rediscover.
7. Complete any remaining mixed file/folder import work.

Non-chart GUI changes should be implemented as a coordinated batch, reusing shared primitives rather than introducing disconnected screen-specific solutions.

### Phase 3: MediaControl and About Me

1. Refactor MediaControl into a composable primitive.
2. Update Weave and the toolkit gallery.
3. Visually review the implemented About Me overview and filters.
4. Verify that analytics use the existing event-based data model.

### Phase 4: Charts

1. Confirm rendering/clipping regressions are resolved.
2. Implement activity, sunburst, artist, session, and loyalty charts.
3. Verify global filtering and responsive behavior.
4. Perform interactive visual review.

### Phase 5: Advanced tag DSL

1. Finalize the query grammar and grouping semantics.
2. Implement engine and CLI support.
3. Integrate query construction into the existing GUI filter model.
4. Synchronize reusable tag-manager functionality with `vendored/weaver`.

## 9. Validation and reporting

For each implementation stage:

1. Inspect the relevant existing architecture.
2. Identify affected modules.
3. Describe proposed changes and reuse opportunities.
4. Implement the changes.
5. Run formatting, compilation, and relevant tests.
6. Verify CLI/engine behavior where applicable.
7. Report GUI implementation separately from interactive visual verification.
8. Identify any changes made to `iced-libadwaita`.
9. Document remaining limitations and incomplete work.

Use these statuses consistently:

- **Pending:** Not implemented.
- **Implemented, unverified:** Code exists but validation is incomplete.
- **Implemented, user visual review:** Automated checks pass or implementation exists, but appearance/interaction requires user review.
- **Done:** Relevant implementation and verification are complete.
- **Deferred:** Intentionally scheduled for a later phase.

Do not treat successful compilation as proof of correct appearance or interaction.

## 10. Final deliverable

Provide:

1. Summary of completed changes.
2. Architectural decisions.
3. Affected Weave modules.
4. Any reusable toolkit abstractions introduced or modified.
5. Engine/CLI changes.
6. Data-model and migration implications.
7. Build and test results.
8. Visual verification status.
9. Known limitations.
10. Remaining tasks.

Prioritize correctness, coherent architecture, and reuse over feature count.

Do not sacrifice the generality of `iced-libadwaita` to simplify one Weave screen.
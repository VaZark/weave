# Library GUI composition and measured overview

Use engine queries for collection search, Rediscover, live filter results and saved
playlist membership. Keep the complete FilterSpec alongside the GUI's editable
projection so editing tags cannot discard sources, ratings, limits, text or playback
predicates. Reload evaluated playlist membership after edits. GUI indexes remain only
presentation addresses; engine calls resolve canonical slugs.

All collection pages share a toolbar/search and flat list structure. Explore uses
controlled SectionDivider headings whose default action toggles its application-owned
open state; the generic widget supports collapsibility, initial state, optional chevrons
and trailing actions. Replace the sample Usage section with real navigation and listening
rankings. Rediscover uses the engine's existing inactivity and ranking semantics.

Move the opinionated DetailHeader composition out of the toolkit into Weave and retain
it as a complete gallery example. The neutral ProfileHeader guarantees image, name and
description; caller-owned after-name, top-right and controls slots provide the rest.
The About me page supplies a filter action but no playback/save controls. Statistics
come from measured engine events, with common exclusions and rolling or custom UTC
ranges using the existing DateRangeInput. Load results on a worker and reject stale
responses. Charts and the rendering/clipping investigation scheduled before them remain
separate work. No new dependencies or fabricated historical analytics are introduced.

//! "Continue at end of queue": pick tracks similar to what has been playing, by shared
//! tags and by closeness to the average BPM.

use std::collections::{HashMap, HashSet};

use weave_core::{FilterSpec, Page, Result, Track, normalize_tag};
use weave_storage::Storage;

/// Only the most recent queue entries describe the current mood.
const CONTEXT_TRACKS: usize = 20;
/// Distinct context tags used to find candidates.
const MAX_TAGS: usize = 30;
const MAX_CANDIDATES: usize = 500;
/// How many tracks to append at once.
pub const BATCH: usize = 5;
/// Candidates must be within this fraction of the average BPM (at least `MIN_BPM_SPAN`).
const BPM_TOLERANCE: f64 = 0.10;
const MIN_BPM_SPAN: f64 = 5.0;

/// `queue_tracks` is the whole queue in list order; the tail is the context.
pub fn suggest(storage: &Storage, queue_tracks: &[Track]) -> Result<Vec<Track>> {
    let context = &queue_tracks[queue_tracks.len().saturating_sub(CONTEXT_TRACKS)..];
    let Some(filter) = candidate_filter(context) else {
        return Ok(Vec::new());
    };
    let candidates = storage
        .search(
            &filter,
            Page {
                offset: 0,
                limit: Some(MAX_CANDIDATES),
            },
        )?
        .tracks;
    let queued: HashSet<&str> = queue_tracks.iter().map(|t| t.id.0.as_str()).collect();
    Ok(rank(context, candidates, &queued, BATCH))
}

fn tag_weights(context: &[Track]) -> HashMap<String, usize> {
    let mut weights = HashMap::new();
    for t in context {
        for tag in &t.tags {
            *weights.entry(normalize_tag(tag)).or_insert(0) += 1;
        }
    }
    weights
}

fn average_bpm(context: &[Track]) -> Option<f64> {
    let known: Vec<f64> = context.iter().filter_map(|t| t.metadata.bpm).collect();
    (!known.is_empty()).then(|| known.iter().sum::<f64>() / known.len() as f64)
}

/// `None` when nothing about the context can describe a similar track.
fn candidate_filter(context: &[Track]) -> Option<FilterSpec> {
    let weights = tag_weights(context);
    let avg = average_bpm(context);
    if weights.is_empty() && avg.is_none() {
        return None;
    }
    let mut tags: Vec<(&String, &usize)> = weights.iter().collect();
    tags.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let span = avg.map(|a| (a * BPM_TOLERANCE).max(MIN_BPM_SPAN));
    Some(FilterSpec {
        tag_any: tags
            .into_iter()
            .take(MAX_TAGS)
            .map(|(t, _)| t.clone())
            .collect(),
        bpm_min: avg.zip(span).map(|(a, s)| a - s),
        bpm_max: avg.zip(span).map(|(a, s)| a + s),
        ..Default::default()
    })
}

/// Best first: most shared tags (weighted by how common each is in the context), then
/// closest to the average BPM, then slug for a stable result.
fn rank(
    context: &[Track],
    candidates: Vec<Track>,
    exclude: &HashSet<&str>,
    n: usize,
) -> Vec<Track> {
    let weights = tag_weights(context);
    let avg = average_bpm(context);
    let mut scored: Vec<(usize, f64, Track)> = candidates
        .into_iter()
        .filter(|t| !exclude.contains(t.id.0.as_str()))
        .map(|t| {
            let score = t
                .tags
                .iter()
                .map(|tag| weights.get(&normalize_tag(tag)).copied().unwrap_or(0))
                .sum();
            let distance = match (avg, t.metadata.bpm) {
                (Some(a), Some(b)) => (a - b).abs(),
                _ => f64::MAX,
            };
            (score, distance, t)
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.id.0.cmp(&b.2.id.0))
    });
    scored.into_iter().take(n).map(|(_, _, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use weave_core::{Metadata, TrackId};

    fn track(slug: &str, tags: &[&str], bpm: Option<f64>) -> Track {
        Track {
            id: TrackId(slug.into()),
            metadata: Metadata {
                title: slug.into(),
                bpm,
                ..Default::default()
            },
            tags: tags.iter().map(|t| t.to_string()).collect(),
            rating: None,
        }
    }

    #[test]
    fn filter_uses_tags_and_bpm_window() {
        let ctx = [
            track("a", &["Bachata"], Some(100.0)),
            track("b", &["bachata", "90s"], Some(120.0)),
        ];
        let f = candidate_filter(&ctx).unwrap();
        assert_eq!(
            f.tag_any,
            ["bachata", "90s"],
            "most common first, lowercased"
        );
        assert_eq!(
            (f.bpm_min, f.bpm_max),
            (Some(99.0), Some(121.0)),
            "avg 110 +-10%"
        );
    }

    #[test]
    fn bpm_window_has_a_minimum_span() {
        let f = candidate_filter(&[track("a", &["x"], Some(20.0))]).unwrap();
        assert_eq!((f.bpm_min, f.bpm_max), (Some(15.0), Some(25.0)));
    }

    #[test]
    fn filter_degrades_gracefully() {
        let tags_only = candidate_filter(&[track("a", &["x"], None)]).unwrap();
        assert_eq!((tags_only.bpm_min, tags_only.tag_any.len()), (None, 1));
        let bpm_only = candidate_filter(&[track("a", &[], Some(100.0))]).unwrap();
        assert!(bpm_only.tag_any.is_empty() && bpm_only.bpm_min.is_some());
        assert!(
            candidate_filter(&[track("a", &[], None)]).is_none(),
            "nothing to go on"
        );
        assert!(candidate_filter(&[]).is_none());
    }

    #[test]
    fn ranking_prefers_shared_tags_then_closer_bpm_and_skips_queued() {
        let ctx = [
            track("c1", &["bachata", "90s"], Some(100.0)),
            track("c2", &["bachata"], Some(100.0)),
        ];
        let candidates = vec![
            track("one-tag-far", &["bachata"], Some(108.0)),
            track("two-tags", &["bachata", "90s"], Some(109.0)),
            track("one-tag-near", &["bachata"], Some(101.0)),
            track("queued", &["bachata", "90s"], Some(100.0)),
            track("tie-b", &["90s"], Some(100.0)),
            track("tie-a", &["90s"], Some(100.0)),
        ];
        let queued: HashSet<&str> = ["queued"].into_iter().collect();
        let got: Vec<String> = rank(&ctx, candidates, &queued, 10)
            .into_iter()
            .map(|t| t.id.0)
            .collect();
        assert_eq!(
            got,
            ["two-tags", "one-tag-near", "one-tag-far", "tie-a", "tie-b"]
        );
    }

    #[test]
    fn ranking_respects_the_batch_size() {
        let ctx = [track("c", &["x"], None)];
        let candidates = (0..10)
            .map(|i| track(&format!("t{i}"), &["x"], None))
            .collect();
        assert_eq!(rank(&ctx, candidates, &HashSet::new(), 3).len(), 3);
    }
}

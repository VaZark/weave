//! Measured listening, independent of queue positions and legacy start counts.
use crate::{Error, FilterSpec, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ListenEnd {
    Completed,
    Skipped,
    Stopped,
    Interrupted,
    Error,
}
text_enum!(ListenEnd {
    Completed => "completed", Skipped => "skipped", Stopped => "stopped",
    Interrupted => "interrupted", Error => "error",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningSegment {
    pub started_ms: i64,
    pub ended_ms: i64,
    pub listened_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningEvent {
    pub id: String,
    pub session: String,
    pub track: String,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub listened_ms: u64,
    pub reason: Option<ListenEnd>,
    pub segments: Vec<ListeningSegment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsPeriod {
    Day,
    Week,
    Month,
    Year,
}
text_enum!(AnalyticsPeriod { Day => "day", Week => "week", Month => "month", Year => "year" });
impl AnalyticsPeriod {
    pub fn milliseconds(self) -> i64 {
        let days = match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
            Self::Year => 365,
        };
        days * 86_400_000
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalyticsQuery {
    /// Half-open UTC millisecond range. Omitted bounds mean all recorded history through now.
    pub since_ms: Option<i64>,
    /// Rolling period ending at until_ms (or now); conflicts with since_ms.
    pub period: Option<AnalyticsPeriod>,
    pub until_ms: Option<i64>,
    pub filter: FilterSpec,
    pub top_n: usize,
}
impl Default for AnalyticsQuery {
    fn default() -> Self {
        Self {
            since_ms: None,
            period: None,
            until_ms: None,
            filter: FilterSpec::default(),
            top_n: 10,
        }
    }
}
impl AnalyticsQuery {
    pub fn range(&self, now: i64) -> Result<(i64, i64)> {
        self.filter.validate()?;
        if self.period.is_some() && self.since_ms.is_some() {
            return Err(Error::InvalidInput(
                "period and since-ms are mutually exclusive".into(),
            ));
        }
        let until = self.until_ms.unwrap_or(now);
        let since = self.since_ms.unwrap_or_else(|| {
            self.period
                .map_or(0, |p| until.saturating_sub(p.milliseconds()).max(0))
        });
        if since < 0 || until <= since || self.top_n == 0 || self.top_n > 1000 {
            return Err(Error::InvalidInput(
                "analytics needs 0 <= since < until and top-n between 1 and 1000".into(),
            ));
        }
        Ok((since, until))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningShare {
    pub id: String,
    pub name: String,
    pub listened_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityBucket {
    pub started_ms: i64,
    pub listened_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenreShare {
    pub name: String,
    pub parent: Option<String>,
    pub listened_ms: u64,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListeningLoyalty {
    pub track: String,
    pub listens: usize,
    pub completed_listens: usize,
    pub ended_listens: usize,
    pub listened_ms: u64,
    /// Completed EOF events / ended observed listens in the range. None when none ended.
    pub completion_rate: Option<f64>,
    /// Additional listens / observed listens; separate from completion.
    pub replay_rate: Option<f64>,
    /// Additional listens divided by the range's number of 24-hour days.
    pub repeats_per_day: f64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListeningSession {
    pub id: String,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub listened_ms: u64,
    pub tags: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ListeningAnalytics {
    pub since_ms: i64,
    pub until_ms: i64,
    pub tracking_since_ms: Option<i64>,
    pub listened_ms: u64,
    pub unique_tracks: usize,
    pub listens: usize,
    pub completed_listens: usize,
    pub ended_listens: usize,
    pub completion_rate: Option<f64>,
    pub replay_rate: Option<f64>,
    pub repeats_per_day: f64,
    pub favorite_track: Option<ListeningShare>,
    pub favorite_artist: Option<ListeningShare>,
    pub most_listened_track: Option<ListeningShare>,
    pub most_listened_artist: Option<ListeningShare>,
    pub most_listened_tag: Option<ListeningShare>,
    pub most_replayed_track: Option<ListeningLoyalty>,
    pub longest_session: Option<ListeningSession>,
    /// UTC day buckets, with absent days meaning zero.
    pub daily: Vec<ActivityBucket>,
    pub activity_bucket_ms: u64,
    pub activity: Vec<ActivityBucket>,
    pub artists: Vec<ListeningShare>,
    pub artist_other_ms: u64,
    pub artist_unknown_ms: u64,
    /// Leaf time is divided across the track's genres; parents sum their children.
    pub genres: Vec<GenreShare>,
    pub sessions: Vec<ListeningSession>,
    pub loyalty: Vec<ListeningLoyalty>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_and_custom_ranges_are_validated() {
        let now = 40_000_000_000;
        for p in AnalyticsPeriod::ALL {
            let q = AnalyticsQuery {
                period: Some(*p),
                ..Default::default()
            };
            assert_eq!(q.range(now).unwrap(), (now - p.milliseconds(), now));
        }
        assert!(
            AnalyticsQuery {
                since_ms: Some(-1),
                ..Default::default()
            }
            .range(now)
            .is_err()
        );
        assert!(
            AnalyticsQuery {
                since_ms: Some(100),
                until_ms: Some(100),
                ..Default::default()
            }
            .range(now)
            .is_err()
        );
        assert!(
            AnalyticsQuery {
                period: Some(AnalyticsPeriod::Day),
                since_ms: Some(1),
                ..Default::default()
            }
            .range(now)
            .is_err()
        );
        assert!(
            AnalyticsQuery {
                top_n: 0,
                ..Default::default()
            }
            .range(now)
            .is_err()
        );
    }
}

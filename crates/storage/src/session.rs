//! Persistence of the playback session (see `weave_player_core::Queue`). The queue is small,
//! so it is stored and loaded whole, inside one transaction.

use rusqlite::params;
use weave_player_core::{Error, PlayerState, Queue, Result, SleepTimer, TrackId};

use crate::{Storage, db_err};

fn corrupt(what: &str, e: String) -> Error {
    Error::Storage(format!("corrupt player state ({what}): {e}"))
}

/// Raw `player_state` row, before parsing the text enums.
struct StateRow {
    status: String,
    repeat: String,
    shuffle: bool,
    end_of_queue: String,
    sleep_until: Option<i64>,
    sleep_stop_after: bool,
    stop_after_track: bool,
    current_position: Option<i64>,
    volume: f64,
    position_ms: i64,
    position_at: i64,
    start_seq: i64,
    seek_seq: i64,
}

impl Storage {
    pub fn load_queue(&self) -> Result<Queue> {
        let row = self
            .conn
            .query_row(
                "SELECT status, repeat, shuffle, end_of_queue, sleep_until, sleep_soft,
                        stop_after_track, current_position, volume, position_ms,
                        position_at, start_seq, seek_seq
                 FROM player_state WHERE id = 1",
                [],
                |r| {
                    Ok(StateRow {
                        status: r.get(0)?,
                        repeat: r.get(1)?,
                        shuffle: r.get(2)?,
                        end_of_queue: r.get(3)?,
                        sleep_until: r.get(4)?,
                        sleep_stop_after: r.get(5)?,
                        stop_after_track: r.get(6)?,
                        current_position: r.get(7)?,
                        volume: r.get(8)?,
                        position_ms: r.get(9)?,
                        position_at: r.get(10)?,
                        start_seq: r.get(11)?,
                        seek_seq: r.get(12)?,
                    })
                },
            )
            .map_err(db_err)?;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT q.position, t.slug, q.play_rank FROM queue_items q
                 JOIN tracks t ON t.id = q.track_id ORDER BY q.position",
            )
            .map_err(db_err)?;
        let rows: Vec<(i64, String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map_err(db_err)?
            .collect::<std::result::Result<_, _>>()
            .map_err(db_err)?;

        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by_key(|&i| rows[i].2);
        Ok(Queue {
            order,
            state: PlayerState {
                status: row.status.parse().map_err(|e| corrupt("status", e))?,
                repeat: row.repeat.parse().map_err(|e| corrupt("repeat", e))?,
                shuffle: row.shuffle,
                end_of_queue: row
                    .end_of_queue
                    .parse()
                    .map_err(|e| corrupt("end_of_queue", e))?,
                sleep: row.sleep_until.map(|at_ms| SleepTimer {
                    at_ms,
                    stop_after: row.sleep_stop_after,
                }),
                stop_after_track: row.stop_after_track,
                current: row
                    .current_position
                    .and_then(|p| rows.iter().position(|r| r.0 == p)),
                volume: row.volume as f32,
                position_ms: row.position_ms.max(0) as u64,
                position_at_ms: row.position_at,
                start_seq: row.start_seq.max(0) as u64,
                seek_seq: row.seek_seq.max(0) as u64,
            },
            items: rows.into_iter().map(|r| TrackId(r.1)).collect(),
        })
    }

    pub fn save_queue(&mut self, queue: &Queue) -> Result<()> {
        // Inverse permutation: list index -> play rank.
        let mut rank = vec![0usize; queue.items.len()];
        if queue.order.len() != queue.items.len() {
            return Err(Error::Storage(
                "queue order does not match its items".into(),
            ));
        }
        for (r, &index) in queue.order.iter().enumerate() {
            *rank
                .get_mut(index)
                .ok_or_else(|| Error::Storage("queue order out of range".into()))? = r;
        }

        let tx = self.conn.transaction().map_err(db_err)?;
        tx.execute("DELETE FROM queue_items", []).map_err(db_err)?;
        for (position, track) in queue.items.iter().enumerate() {
            let internal: String = tx
                .query_row("SELECT id FROM tracks WHERE slug = ?1", [&track.0], |r| {
                    r.get(0)
                })
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        Error::NotFound(format!("track {track}"))
                    }
                    e => db_err(e),
                })?;
            tx.execute(
                "INSERT INTO queue_items (position, track_id, play_rank) VALUES (?1, ?2, ?3)",
                params![position as i64, internal, rank[position] as i64],
            )
            .map_err(db_err)?;
        }
        let s = &queue.state;
        tx.execute(
            "UPDATE player_state SET status = ?1, repeat = ?2, shuffle = ?3, end_of_queue = ?4,
                 sleep_until = ?5, sleep_soft = ?6, stop_after_track = ?7, current_position = ?8,
                 volume = ?9, position_ms = ?10, position_at = ?11, start_seq = ?12, seek_seq = ?13
             WHERE id = 1",
            params![
                s.status.as_str(),
                s.repeat.as_str(),
                s.shuffle,
                s.end_of_queue.as_str(),
                s.sleep.map(|t| t.at_ms),
                s.sleep.is_some_and(|t| t.stop_after),
                s.stop_after_track,
                s.current.map(|c| c as i64),
                s.volume as f64,
                s.position_ms as i64,
                s.position_at_ms,
                s.start_seq as i64,
                s.seek_seq as i64,
            ],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }

    /// A host's report of where playback really is. Ignored (returns `false`) unless the
    /// session is still on the same track start (`start_seq`), so a slow report can never
    /// overwrite the position of a newer track. Touches only the position anchor.
    pub fn report_position(
        &mut self,
        start_seq: u64,
        position_ms: u64,
        at_ms: i64,
    ) -> Result<bool> {
        let changed = self
            .conn
            .execute(
                "UPDATE player_state SET position_ms = ?1, position_at = ?2
                 WHERE id = 1 AND start_seq = ?3",
                params![position_ms as i64, at_ms, start_seq as i64],
            )
            .map_err(db_err)?;
        Ok(changed == 1)
    }

    /// Ask the playback host to exit (`true`), or clear the request (`false`).
    pub fn set_host_quit(&mut self, quit: bool) -> Result<()> {
        self.conn
            .execute(
                "UPDATE player_state SET host_quit = ?1 WHERE id = 1",
                [quit],
            )
            .map_err(db_err)?;
        Ok(())
    }

    pub fn host_quit_requested(&self) -> Result<bool> {
        self.conn
            .query_row("SELECT host_quit FROM player_state WHERE id = 1", [], |r| {
                r.get(0)
            })
            .map_err(db_err)
    }
}

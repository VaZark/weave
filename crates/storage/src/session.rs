//! Persistence of the playback session (see `weave_core::Queue`). The queue is small,
//! so it is stored and loaded whole, inside one transaction.

use rusqlite::params;
use weave_core::{Error, PlayerState, Queue, Result, TrackId};

use crate::{Storage, db_err};

fn corrupt(what: &str, e: String) -> Error {
    Error::Storage(format!("corrupt player state ({what}): {e}"))
}

impl Storage {
    pub fn load_queue(&self) -> Result<Queue> {
        let (status, repeat, shuffle, end, sleep, current): (
            String,
            String,
            bool,
            String,
            Option<i64>,
            Option<i64>,
        ) = self
            .conn
            .query_row(
                "SELECT status, repeat, shuffle, end_of_queue, sleep_until, current_position
                 FROM player_state WHERE id = 1",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
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
                status: status.parse().map_err(|e| corrupt("status", e))?,
                repeat: repeat.parse().map_err(|e| corrupt("repeat", e))?,
                shuffle,
                end_of_queue: end.parse().map_err(|e| corrupt("end_of_queue", e))?,
                sleep_until_ms: sleep,
                current: current.and_then(|p| rows.iter().position(|r| r.0 == p)),
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
                 sleep_until = ?5, current_position = ?6 WHERE id = 1",
            params![
                s.status.as_str(),
                s.repeat.as_str(),
                s.shuffle,
                s.end_of_queue.as_str(),
                s.sleep_until_ms,
                s.current.map(|c| c as i64),
            ],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }
}

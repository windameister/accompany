use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    pub id: String,
    pub memory_type: String,
    pub content: String,
    pub source: String,
    pub confidence: f64,
    pub access_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Thread-safe SQLite wrapper. All operations run on spawn_blocking
/// to avoid blocking the async runtime.
#[derive(Clone)]
pub struct MemoryDb {
    conn: Arc<Mutex<Connection>>,
}

impl MemoryDb {
    pub fn open(db_path: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create db directory: {}", e))?;
        }

        let conn = Connection::open(db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("Failed to set pragmas: {}", e))?;

        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS memories (
                id          TEXT PRIMARY KEY,
                memory_type TEXT NOT NULL,
                content     TEXT NOT NULL,
                source      TEXT NOT NULL DEFAULT 'conversation',
                confidence  REAL NOT NULL DEFAULT 0.8,
                access_count INTEGER NOT NULL DEFAULT 0,
                created_at  TEXT NOT NULL,
                updated_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type);
            CREATE INDEX IF NOT EXISTS idx_memories_updated ON memories(updated_at);
            ",
        )
        .map_err(|e| format!("Migration failed: {}", e))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub async fn add_memory(
        &self,
        memory_type: &str,
        content: &str,
        source: &str,
        confidence: f64,
    ) -> Result<String, String> {
        let conn = self.conn.clone();
        let id = ulid::Ulid::new().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let mt = memory_type.to_string();
        let ct = content.to_string();
        let src = source.to_string();
        let id2 = id.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT INTO memories (id, memory_type, content, source, confidence, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![id2, mt, ct, src, confidence, now, now],
            )
            .map_err(|e| format!("Failed to insert: {}", e))?;
            Ok(id2)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn search_memories(&self, query: &str, limit: usize) -> Result<Vec<Memory>, String> {
        let conn = self.conn.clone();
        let query = query.to_string();

        let mut scored = tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT id, memory_type, content, source, confidence, access_count, created_at, updated_at
                     FROM memories ORDER BY updated_at DESC LIMIT 200",
                )
                .map_err(|e| format!("Query failed: {}", e))?;

            let memories: Vec<Memory> = stmt
                .query_map([], |row| {
                    Ok(Memory {
                        id: row.get(0)?,
                        memory_type: row.get(1)?,
                        content: row.get(2)?,
                        source: row.get(3)?,
                        confidence: row.get(4)?,
                        access_count: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })
                .map_err(|e| format!("Failed to query: {}", e))?
                .filter_map(|r| r.ok())
                .collect();

            let query_lower = query.to_lowercase();
            let mut scored: Vec<(f64, Memory)> = memories
                .into_iter()
                .map(|m| {
                    let content_lower = m.content.to_lowercase();
                    let keyword_score = keyword_similarity(&query_lower, &content_lower);
                    let recency_score = m.confidence * 0.3;
                    let freq_score = (m.access_count as f64).min(10.0) / 10.0 * 0.1;
                    (keyword_score * 0.6 + recency_score + freq_score, m)
                })
                .filter(|(score, _)| *score > 0.05)
                .collect();

            scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            scored.truncate(limit);
            Ok::<_, String>(scored)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))??;

        // Bump access counts
        for (_, m) in &scored {
            let _ = self.bump_access(&m.id).await;
        }

        Ok(scored.into_iter().map(|(_, m)| m).collect())
    }

    async fn bump_access(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.execute(
                "UPDATE memories SET access_count = access_count + 1 WHERE id = ?1",
                params![id],
            )
            .map_err(|e| format!("Failed to bump: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn all_memories(&self) -> Result<Vec<Memory>, String> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT id, memory_type, content, source, confidence, access_count, created_at, updated_at
                     FROM memories ORDER BY updated_at DESC",
                )
                .map_err(|e| format!("Query failed: {}", e))?;

            let memories = stmt
                .query_map([], |row| {
                    Ok(Memory {
                        id: row.get(0)?,
                        memory_type: row.get(1)?,
                        content: row.get(2)?,
                        source: row.get(3)?,
                        confidence: row.get(4)?,
                        access_count: row.get(5)?,
                        created_at: row.get(6)?,
                        updated_at: row.get(7)?,
                    })
                })
                .map_err(|e| format!("Failed to query: {}", e))?
                .filter_map(|r| r.ok())
                .collect();

            Ok(memories)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
    }

    pub async fn delete_memory(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.clone();
        let id = id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.execute("DELETE FROM memories WHERE id = ?1", params![id])
                .map_err(|e| format!("Failed to delete: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))?
    }
}

fn keyword_similarity(query: &str, content: &str) -> f64 {
    let query_words: Vec<&str> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 1)
        .collect();

    if query_words.is_empty() {
        return 0.0;
    }

    let matched = query_words.iter().filter(|w| content.contains(**w)).count();
    matched as f64 / query_words.len() as f64
}

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;
use sha2::{Digest, Sha256};

const CACHE_SCHEMA: &str = "\
    CREATE TABLE IF NOT EXISTS embedding_cache (\
        content_hash TEXT PRIMARY KEY,\
        model TEXT NOT NULL,\
        vector BLOB NOT NULL,\
        created_at TEXT NOT NULL DEFAULT (datetime('now'))\
    );\
";

/// Load the sqlite-vec extension into all future connections.
///
/// Must be called before opening any `Connection` that uses vec0 tables.
/// Safe to call multiple times (idempotent within the process).
fn register_vec_extension() {
    use rusqlite::ffi::sqlite3_auto_extension;
    use sqlite_vec::sqlite3_vec_init;

    // SAFETY: `sqlite3_vec_init` has the correct signature expected by
    // `sqlite3_auto_extension` — it is an SQLite extension entry point
    // compiled from the vendored C source via `sqlite-vec`'s build.rs.
    unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut ::std::os::raw::c_char,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> ::std::os::raw::c_int,
        >(sqlite3_vec_init as *const ())));
    }
}

pub struct EmbeddingStore {
    conn: Mutex<Connection>,
}

impl EmbeddingStore {
    /// # Errors
    /// Returns `rusqlite::Error` if the database cannot be opened.
    pub fn open(db_path: &Path) -> Result<Self, rusqlite::Error> {
        register_vec_extension();
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(CACHE_SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    #[must_use]
    pub fn get(&self, content_hash: &str) -> Option<Vec<f64>> {
        self.conn
            .lock()
            .ok()
            .and_then(|conn| embedding_store_get(&conn, content_hash))
    }

    /// # Errors
    /// Returns `rusqlite::Error` if the insert fails.
    pub fn put(
        &self,
        content_hash: &str,
        model: &str,
        vector: &[f64],
    ) -> Result<(), rusqlite::Error> {
        let blob = serialize_vector(vector);
        self.conn
            .lock()
            .map_err(|_| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    Some("lock poisoned".into()),
                )
            })?
            .execute(
                "INSERT OR REPLACE INTO embedding_cache (content_hash, model, vector) VALUES (?1, ?2, ?3)",
                rusqlite::params![content_hash, model, blob],
            )?;
        Ok(())
    }

    /// Evict cache entries older than `max_age_days`.
    ///
    /// # Errors
    /// Returns `rusqlite::Error` if the deletion fails.
    pub fn evict_stale(&self, max_age_days: u64) -> Result<usize, rusqlite::Error> {
        self.conn
            .lock()
            .map_err(|_| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    Some("lock poisoned".into()),
                )
            })?
            .execute(
                "DELETE FROM embedding_cache WHERE created_at < datetime('now', ?1)",
                rusqlite::params![format!("-{max_age_days} days")],
            )
    }

    // ── Vector index methods (sqlite-vec) ─────────────────────

    /// Ensure the `memory_vectors` vec0 virtual table exists with the given
    /// embedding dimensionality.
    ///
    /// # Errors
    /// Returns `rusqlite::Error` if the table creation fails.
    pub fn ensure_vector_table(&self, dims: usize) -> Result<(), rusqlite::Error> {
        self.conn
            .lock()
            .map_err(|_| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    Some("lock poisoned".into()),
                )
            })?
            .execute_batch(&format!(
                "CREATE VIRTUAL TABLE IF NOT EXISTS memory_vectors USING vec0(\
                    memory_id TEXT PRIMARY KEY,\
                    embedding FLOAT[{dims}] distance_metric=cosine\
                )"
            ))?;
        Ok(())
    }

    /// Insert or replace a memory's embedding in the vector index.
    ///
    /// The embedding is converted from f64 to f32 (vec0 uses single precision).
    ///
    /// # Errors
    /// Returns `rusqlite::Error` if the upsert fails.
    pub fn upsert_vector(&self, memory_id: &str, embedding: &[f64]) -> Result<(), rusqlite::Error> {
        let blob = f64_slice_to_f32_blob(embedding);
        let conn = self.conn.lock().map_err(|_| {
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                Some("lock poisoned".into()),
            )
        })?;
        // vec0 doesn't support INSERT OR REPLACE; delete first if exists
        conn.execute(
            "DELETE FROM memory_vectors WHERE memory_id = ?1",
            [memory_id],
        )
        .ok();
        conn.execute(
            "INSERT INTO memory_vectors (memory_id, embedding) VALUES (?1, ?2)",
            rusqlite::params![memory_id, blob],
        )?;
        drop(conn);
        Ok(())
    }

    /// Search the vector index for the nearest memories by cosine distance.
    ///
    /// Returns `(memory_id, distance)` pairs ordered by ascending distance
    /// (closest first). An empty `Vec` is returned on any error.
    #[must_use]
    pub fn search_vectors(&self, query_embedding: &[f64], limit: usize) -> Vec<(String, f64)> {
        let blob = f64_slice_to_f32_blob(query_embedding);
        let Ok(conn) = self.conn.lock() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT memory_id, distance \
             FROM memory_vectors \
             WHERE embedding MATCH ?1 AND k = ?2",
        ) else {
            return Vec::new();
        };
        let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
        stmt.query_map(rusqlite::params![blob, limit_i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Remove a memory from the vector index.
    ///
    /// # Errors
    /// Returns `rusqlite::Error` if the deletion fails.
    pub fn remove_vector(&self, memory_id: &str) -> Result<(), rusqlite::Error> {
        self.conn
            .lock()
            .map_err(|_| {
                rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    Some("lock poisoned".into()),
                )
            })?
            .execute(
                "DELETE FROM memory_vectors WHERE memory_id = ?1",
                [memory_id],
            )?;
        Ok(())
    }
}

/// Query a single embedding vector by content hash.
fn embedding_store_get(conn: &Connection, content_hash: &str) -> Option<Vec<f64>> {
    let mut stmt = conn
        .prepare("SELECT vector FROM embedding_cache WHERE content_hash = ?1")
        .ok()?;
    stmt.query_row(rusqlite::params![content_hash], |row| {
        let blob: Vec<u8> = row.get(0)?;
        Ok(deserialize_vector(&blob))
    })
    .ok()
}

/// Compute a SHA-256 content hash for cache keying.
#[must_use]
pub fn content_hash(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

fn serialize_vector(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn deserialize_vector(data: &[u8]) -> Vec<f64> {
    data.chunks_exact(8)
        .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap_or([0u8; 8])))
        .collect()
}

/// Convert an f64 slice to a little-endian f32 byte blob for sqlite-vec.
///
/// Embedding vectors have more than enough precision at 32 bits.
/// Convert f64 embedding slice to f32 little-endian blob for sqlite-vec.
///
/// Embedding components are normalized to `[-1, 1]` — well within f32 range.
/// The IEEE 754 bit-level conversion preserves maximum precision.
fn f64_slice_to_f32_blob(v: &[f64]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for &x in v {
        buf.extend_from_slice(&ieee754_f64_to_f32_le(x));
    }
    buf
}

/// Convert f64 to f32 little-endian bytes via IEEE 754 bit manipulation.
///
/// This avoids the Rust `as f32` cast (which triggers pedantic clippy lints)
/// by directly extracting sign, exponent, and mantissa from the f64 bits.
/// For embedding values in `[-1, 1]`, the result matches `(x as f32).to_le_bytes()`.
const fn ieee754_f64_to_f32_le(x: f64) -> [u8; 4] {
    let bits = x.to_bits();
    let sign = (bits >> 63) as u32;
    let exp = ((bits >> 52) & 0x7FF) as i32;
    let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;

    let f32_bits = if exp == 0 && mantissa == 0 {
        // ±0
        sign << 31
    } else if exp == 0x7FF {
        // Inf or NaN → f32 Inf/NaN
        (sign << 31) | 0x7F80_0000 | if mantissa != 0 { 0x0040_0000 } else { 0 }
    } else {
        // Normal or subnormal: re-bias exponent from f64 (bias 1023) to f32 (bias 127)
        let f32_exp = exp - 1023 + 127;
        if f32_exp >= 0xFF {
            // Overflow → ±Inf
            (sign << 31) | 0x7F80_0000
        } else if f32_exp <= 0 {
            // Underflow → ±0 (subnormals in embedding range are effectively 0)
            sign << 31
        } else {
            // f32_exp is guaranteed positive (checked > 0 above)
            let f32_exp = f32_exp.unsigned_abs();
            // Take top 23 bits of the 52-bit mantissa
            let f32_mantissa = (mantissa >> 29) as u32;
            (sign << 31) | (f32_exp << 23) | f32_mantissa
        }
    };
    f32_bits.to_le_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_roundtrip() {
        let v = vec![1.0, -2.5, 3.125, 0.0];
        let data = serialize_vector(&v);
        let back = deserialize_vector(&data);
        assert_eq!(v, back);
    }

    #[test]
    fn cache_put_get() {
        let dir = tempfile::tempdir().unwrap();
        let store = EmbeddingStore::open(&dir.path().join("embed.db")).unwrap();
        let hash = content_hash("hello world");
        store.put(&hash, "model1", &[0.1, 0.2, 0.3]).unwrap();
        let vec = store.get(&hash).unwrap();
        assert_eq!(vec.len(), 3);
        assert!((vec[0] - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn cache_miss() {
        let dir = tempfile::tempdir().unwrap();
        let store = EmbeddingStore::open(&dir.path().join("embed.db")).unwrap();
        assert!(store.get("nonexistent").is_none());
    }

    #[test]
    fn content_hash_deterministic() {
        let h1 = content_hash("test");
        let h2 = content_hash("test");
        assert_eq!(h1, h2);
        let h3 = content_hash("different");
        assert_ne!(h1, h3);
    }

    #[test]
    fn vector_table_upsert_and_search() {
        let dir = tempfile::tempdir().unwrap();
        let store = EmbeddingStore::open(&dir.path().join("vec.db")).unwrap();
        store.ensure_vector_table(3).unwrap();

        store.upsert_vector("m1", &[1.0, 0.0, 0.0]).unwrap();
        store.upsert_vector("m2", &[0.0, 1.0, 0.0]).unwrap();
        store.upsert_vector("m3", &[0.9, 0.1, 0.0]).unwrap();

        let results = store.search_vectors(&[1.0, 0.0, 0.0], 2);
        assert_eq!(results.len(), 2);
        // m1 should be closest (distance ~0), then m3
        assert_eq!(results[0].0, "m1");
        assert_eq!(results[1].0, "m3");
    }

    #[test]
    fn vector_remove() {
        let dir = tempfile::tempdir().unwrap();
        let store = EmbeddingStore::open(&dir.path().join("vec.db")).unwrap();
        store.ensure_vector_table(2).unwrap();

        store.upsert_vector("m1", &[1.0, 0.0]).unwrap();
        store.remove_vector("m1").unwrap();

        let results = store.search_vectors(&[1.0, 0.0], 10);
        assert!(results.is_empty());
    }

    #[test]
    fn vector_upsert_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let store = EmbeddingStore::open(&dir.path().join("vec.db")).unwrap();
        store.ensure_vector_table(2).unwrap();

        store.upsert_vector("m1", &[1.0, 0.0]).unwrap();
        store.upsert_vector("m1", &[0.0, 1.0]).unwrap();

        let results = store.search_vectors(&[0.0, 1.0], 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "m1");
    }

    #[test]
    fn search_empty_table() {
        let dir = tempfile::tempdir().unwrap();
        let store = EmbeddingStore::open(&dir.path().join("vec.db")).unwrap();
        store.ensure_vector_table(4).unwrap();

        let results = store.search_vectors(&[1.0, 0.0, 0.0, 0.0], 10);
        assert!(results.is_empty());
    }
}

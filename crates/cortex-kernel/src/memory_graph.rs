use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use cortex_types::MemoryRelation;
use rusqlite::{Connection, params};

const SCHEMA: &str = "\
    CREATE TABLE IF NOT EXISTS relations (
        source_id TEXT NOT NULL,
        target_id TEXT NOT NULL,
        relation_type TEXT NOT NULL,
        metadata TEXT,
        PRIMARY KEY (source_id, target_id, relation_type)
    );
    CREATE INDEX IF NOT EXISTS idx_relations_source ON relations(source_id);
    CREATE INDEX IF NOT EXISTS idx_relations_target ON relations(target_id);
    CREATE INDEX IF NOT EXISTS idx_relations_type ON relations(relation_type);";

pub struct MemoryGraph {
    conn: Mutex<Connection>,
}

impl MemoryGraph {
    /// Open or create a memory graph database at the given path.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the database cannot be opened or the schema fails.
    pub fn open(path: &Path) -> Result<Self, MemoryGraphError> {
        let conn =
            Connection::open(path).map_err(|e| MemoryGraphError::Storage(format!("open: {e}")))?;
        Self::init_conn(conn)
    }

    /// Create an in-memory memory graph (useful for testing).
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the in-memory database cannot be created.
    pub fn in_memory() -> Result<Self, MemoryGraphError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| MemoryGraphError::Storage(format!("open in-memory: {e}")))?;
        Self::init_conn(conn)
    }

    fn init_conn(conn: Connection) -> Result<Self, MemoryGraphError> {
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| MemoryGraphError::Storage(format!("pragmas: {e}")))?;

        conn.execute_batch(SCHEMA)
            .map_err(|e| MemoryGraphError::Storage(format!("init schema: {e}")))?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn lock_conn(&self) -> Result<MutexGuard<'_, Connection>, MemoryGraphError> {
        self.conn
            .lock()
            .map_err(|e| MemoryGraphError::Storage(format!("mutex poisoned: {e}")))
    }

    /// Insert or replace a relation between two memory nodes.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the insert fails.
    pub fn add_relation(&self, rel: &MemoryRelation) -> Result<(), MemoryGraphError> {
        {
            let conn = self.lock_conn()?;
            conn.execute(
                "INSERT OR REPLACE INTO relations (source_id, target_id, relation_type, metadata) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    rel.source_id,
                    rel.target_id,
                    rel.relation_type,
                    rel.metadata
                ],
            )
            .map_err(|e| MemoryGraphError::Storage(format!("add_relation: {e}")))?;
        }
        Ok(())
    }

    /// Remove a specific relation. Returns `true` if a row was deleted.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the delete fails.
    pub fn remove_relation(
        &self,
        source_id: &str,
        target_id: &str,
        relation_type: &str,
    ) -> Result<bool, MemoryGraphError> {
        let rows = {
            let conn = self.lock_conn()?;
            conn.execute(
                "DELETE FROM relations \
                 WHERE source_id = ?1 AND target_id = ?2 AND relation_type = ?3",
                params![source_id, target_id, relation_type],
            )
            .map_err(|e| MemoryGraphError::Storage(format!("remove_relation: {e}")))?
        };
        Ok(rows > 0)
    }

    /// Return all directly connected node IDs for a given memory.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the query fails.
    pub fn neighbors(&self, memory_id: &str) -> Result<Vec<String>, MemoryGraphError> {
        let conn = self.lock_conn()?;
        query_string_list(
            &conn,
            "SELECT DISTINCT target_id FROM relations WHERE source_id = ?1 \
             UNION \
             SELECT DISTINCT source_id FROM relations WHERE target_id = ?1",
            &[memory_id],
            "neighbors",
        )
    }

    /// Return directly connected node IDs filtered by relation type.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the query fails.
    pub fn neighbors_by_type(
        &self,
        memory_id: &str,
        relation_type: &str,
    ) -> Result<Vec<String>, MemoryGraphError> {
        let conn = self.lock_conn()?;
        query_string_list(
            &conn,
            "SELECT DISTINCT target_id FROM relations \
             WHERE source_id = ?1 AND relation_type = ?2 \
             UNION \
             SELECT DISTINCT source_id FROM relations \
             WHERE target_id = ?1 AND relation_type = ?2",
            &[memory_id, relation_type],
            "neighbors_by_type",
        )
    }

    /// Return all relations involving a given memory node.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the query fails.
    pub fn relations_of(&self, memory_id: &str) -> Result<Vec<MemoryRelation>, MemoryGraphError> {
        let conn = self.lock_conn()?;
        query_relations(
            &conn,
            "SELECT source_id, target_id, relation_type, metadata FROM relations \
             WHERE source_id = ?1 OR target_id = ?1",
            &[memory_id],
            "relations_of",
        )
    }

    /// Return all node IDs reachable within `max_depth` hops (excluding the start node).
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the recursive query fails.
    pub fn reachable(
        &self,
        memory_id: &str,
        max_depth: u32,
    ) -> Result<HashSet<String>, MemoryGraphError> {
        if max_depth == 0 {
            return Ok(HashSet::new());
        }
        let conn = self.lock_conn()?;
        query_reachable(&conn, memory_id, max_depth)
    }

    /// Return all distinct node IDs that appear in any relation (as source or target).
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the query fails.
    pub fn all_node_ids(&self) -> Result<HashSet<String>, MemoryGraphError> {
        let conn = self.lock_conn()?;
        query_string_set(
            &conn,
            "SELECT DISTINCT source_id FROM relations \
             UNION \
             SELECT DISTINCT target_id FROM relations",
            "all_node_ids",
        )
    }

    /// Return all relations in the graph.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the query fails.
    pub fn all_relations(&self) -> Result<Vec<MemoryRelation>, MemoryGraphError> {
        let conn = self.lock_conn()?;
        query_relations(
            &conn,
            "SELECT source_id, target_id, relation_type, metadata FROM relations",
            &[],
            "all_relations",
        )
    }

    /// Return the number of relations each node participates in (undirected degree).
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the query fails.
    pub fn degree_map(&self) -> Result<HashMap<String, usize>, MemoryGraphError> {
        let rels = self.all_relations()?;
        let mut degrees: HashMap<String, usize> = HashMap::new();
        for rel in &rels {
            *degrees.entry(rel.source_id.clone()).or_default() += 1;
            *degrees.entry(rel.target_id.clone()).or_default() += 1;
        }
        Ok(degrees)
    }

    /// Compute connected components from a set of node IDs using BFS.
    /// The adjacency is derived from relations in the graph.
    ///
    /// # Errors
    /// Returns `MemoryGraphError::Storage` if the relation query fails.
    pub fn connected_components(
        &self,
        nodes: &HashSet<String>,
    ) -> Result<Vec<HashSet<String>>, MemoryGraphError> {
        let rels = self.all_relations()?;
        let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
        for rel in &rels {
            if nodes.contains(&rel.source_id) && nodes.contains(&rel.target_id) {
                adj.entry(rel.source_id.clone())
                    .or_default()
                    .insert(rel.target_id.clone());
                adj.entry(rel.target_id.clone())
                    .or_default()
                    .insert(rel.source_id.clone());
            }
        }

        let mut visited: HashSet<String> = HashSet::new();
        let mut components = Vec::new();

        for node in nodes {
            if visited.contains(node) {
                continue;
            }
            let mut component = HashSet::new();
            let mut queue = VecDeque::new();
            queue.push_back(node.clone());
            visited.insert(node.clone());

            while let Some(current) = queue.pop_front() {
                component.insert(current.clone());
                if let Some(neighbors) = adj.get(&current) {
                    for neighbor in neighbors {
                        if !visited.contains(neighbor) {
                            visited.insert(neighbor.clone());
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
            components.push(component);
        }

        Ok(components)
    }
}

#[derive(Debug)]
pub enum MemoryGraphError {
    Storage(String),
}

impl std::fmt::Display for MemoryGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "memory graph error: {e}"),
        }
    }
}

impl std::error::Error for MemoryGraphError {}

/// Query helper: collect string IDs from a parameterized SQL.
fn query_string_list(
    conn: &rusqlite::Connection,
    sql: &str,
    params_slice: &[&str],
    label: &str,
) -> Result<Vec<String>, MemoryGraphError> {
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_slice
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| MemoryGraphError::Storage(format!("{label} prepare: {e}")))?;
    Ok(stmt
        .query_map(params_refs.as_slice(), |row| row.get(0))
        .map_err(|e| MemoryGraphError::Storage(format!("{label} query: {e}")))?
        .filter_map(Result::ok)
        .collect())
}

/// Query helper: collect string IDs into a `HashSet`.
fn query_string_set(
    conn: &rusqlite::Connection,
    sql: &str,
    label: &str,
) -> Result<HashSet<String>, MemoryGraphError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| MemoryGraphError::Storage(format!("{label} prepare: {e}")))?;
    Ok(stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| MemoryGraphError::Storage(format!("{label} query: {e}")))?
        .filter_map(Result::ok)
        .collect())
}

/// Query helper: collect `MemoryRelation` rows.
fn query_relations(
    conn: &rusqlite::Connection,
    sql: &str,
    params_slice: &[&str],
    label: &str,
) -> Result<Vec<MemoryRelation>, MemoryGraphError> {
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_slice
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| MemoryGraphError::Storage(format!("{label} prepare: {e}")))?;
    Ok(stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(MemoryRelation {
                source_id: row.get(0)?,
                target_id: row.get(1)?,
                relation_type: row.get(2)?,
                metadata: row.get(3)?,
            })
        })
        .map_err(|e| MemoryGraphError::Storage(format!("{label} query: {e}")))?
        .filter_map(Result::ok)
        .collect())
}

/// Query helper: recursive reachability traversal.
fn query_reachable(
    conn: &rusqlite::Connection,
    memory_id: &str,
    max_depth: u32,
) -> Result<HashSet<String>, MemoryGraphError> {
    let mut stmt = conn
        .prepare(
            "WITH RECURSIVE traverse(id, depth) AS ( \
                 SELECT DISTINCT target_id, 1 FROM relations WHERE source_id = ?1 \
                 UNION \
                 SELECT DISTINCT source_id, 1 FROM relations WHERE target_id = ?1 \
                 UNION \
                 SELECT DISTINCT r.target_id, t.depth + 1 \
                 FROM traverse t \
                 JOIN relations r ON r.source_id = t.id \
                 WHERE t.depth < ?2 \
                 UNION \
                 SELECT DISTINCT r.source_id, t.depth + 1 \
                 FROM traverse t \
                 JOIN relations r ON r.target_id = t.id \
                 WHERE t.depth < ?2 \
             ) \
             SELECT DISTINCT id FROM traverse WHERE id != ?1",
        )
        .map_err(|e| MemoryGraphError::Storage(format!("reachable prepare: {e}")))?;
    Ok(stmt
        .query_map(params![memory_id, max_depth], |row| row.get(0))
        .map_err(|e| MemoryGraphError::Storage(format!("reachable query: {e}")))?
        .filter_map(Result::ok)
        .collect())
}

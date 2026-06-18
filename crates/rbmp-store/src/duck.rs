use std::path::Path;
use duckdb::{Connection, Result as DuckResult};
use anyhow::Result;
use tracing::{info, error};
use crate::schema::CREATE_TABLES;

/// Persistent DuckDB-backed route store
pub struct RouteStore {
    conn: Connection,
}

impl RouteStore {
    /// Open (or create) the DuckDB database at `path`.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path.as_ref())?;
        info!(path = %path.as_ref().display(), "Opened DuckDB route store");
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    /// Open an ephemeral in-memory database (for testing / --no-persist mode)
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(CREATE_TABLES)?;
        info!("DuckDB schema initialized");
        Ok(())
    }

    pub fn conn(&self) -> &Connection { &self.conn }

    /// Flush DuckDB WAL to disk
    pub fn checkpoint(&self) -> Result<()> {
        self.conn.execute_batch("CHECKPOINT;")?;
        Ok(())
    }
}

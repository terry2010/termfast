//! SQLCipher encrypted storage — single .db file for all local data.
//!
//! Replaces FileConfigStorage (config.json), runtime_state.json,
//! credentials.enc, cloud_tokens.json, sync_state.enc, sync_hash.dat,
//! pairings.json, ecdh_key.json, device_key.json, device_id.json.
//!
//! All data is encrypted at rest with AES-256 via SQLCipher.
//! The encryption key (DEK) is either the built-in default (no master password)
//! or derived from the user's master password via Argon2id.

use crate::error::{Error, Result};
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Built-in default DEK (32 bytes).
/// Not a secret — only provides at-rest protection for users without a master password.
/// The real security boundary is the user-set master password (Argon2id-derived DEK).
pub const DEFAULT_DEK: [u8; 32] = [
    0x54, 0x65, 0x72, 0x6d, 0x46, 0x61, 0x73, 0x74, // "TermFast"
    0x44, 0x65, 0x66, 0x61, 0x75, 0x6c, 0x74, 0x44, // "DefaultD"
    0x45, 0x4b, 0x32, 0x30, 0x32, 0x34, 0x4e, 0x6f, // "EK2024No"
    0x74, 0x53, 0x65, 0x63, 0x72, 0x65, 0x74, 0x21, // "tSecret!"
]; // "TermFastDefaultDEK2024NotSecret!"

/// Result of attempting to open a DB with a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenResult {
    /// Key is wrong — DB file exists but decryption fails.
    WrongKey,
    /// DB file is corrupt (partial rekey, WAL damage, etc.).
    Corrupt,
    /// Other IO or SQLite error.
    Other(String),
}

impl std::fmt::Display for OpenResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenResult::WrongKey => write!(f, "wrong key"),
            OpenResult::Corrupt => write!(f, "database corrupt"),
            OpenResult::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for OpenResult {}

/// Core SQLCipher storage — raw SQL operations + schema management.
/// Does NOT implement any trait. Wrapped by SqlCipherConfigStorage and
/// SqlCipherCredentialStore in their respective crates.
pub struct SqlCipherStorage {
    conn: Mutex<Connection>,
    pub db_path: PathBuf,
}

// === SECTION 1 END ===

impl SqlCipherStorage {
    /// Create a new DB file with the given DEK and initialize schema.
    /// Caller must ensure the file does not already exist.
    pub fn create_new(db_path: &Path, dek: &[u8; 32]) -> Result<Self> {
        if db_path.exists() {
            return Err(Error::Config(format!(
                "DB file already exists: {}",
                db_path.display()
            )));
        }
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::Io)?;
        }
        let conn = open_with_key(db_path, dek).map_err(|e| match e {
            OpenResult::WrongKey => Error::Config("wrong key for new DB (should not happen)".into()),
            OpenResult::Corrupt => Error::Config("corrupt new DB (should not happen)".into()),
            OpenResult::Other(msg) => Error::Other(msg),
        })?;
        let storage = Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
        };
        storage.init_schema()?;
        Ok(storage)
    }

    /// Open an existing DB file with the given DEK.
    /// Use `open_or_recover` instead for crash-safe startup.
    pub fn open(db_path: &Path, dek: &[u8; 32]) -> std::result::Result<Self, OpenResult> {
        let conn = open_with_key(db_path, dek)?;
        Ok(Self {
            conn: Mutex::new(conn),
            db_path: db_path.to_path_buf(),
        })
    }

    /// Open from an already-established connection (used by daemon layer
    /// after open_or_recover succeeds).
    pub fn from_conn(conn: Connection, db_path: PathBuf) -> Self {
        Self {
            conn: Mutex::new(conn),
            db_path,
        }
    }

    /// Check if the DB file exists on disk.
    pub fn db_file_exists(&self) -> bool {
        self.db_path.exists()
    }

    /// Check if the current DEK is the default (i.e., user has not set a master password).
    pub fn is_using_default_dek(&self) -> bool {
        // We can't directly compare the DEK since we don't store it.
        // Instead, check a marker in schema_meta.
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT value FROM schema_meta WHERE key = 'using_default_dek'",
            [],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => val == "true",
            Err(_) => true, // default to true if marker doesn't exist
        }
    }

    /// Initialize schema (create all tables).
    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS general_config (
                id    INTEGER PRIMARY KEY DEFAULT 1,
                data  TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS servers (
                id         TEXT PRIMARY KEY,
                name       TEXT NOT NULL,
                data       TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS trigger_templates (
                id         TEXT PRIMARY KEY,
                name       TEXT NOT NULL,
                data       TEXT NOT NULL,
                is_builtin INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS local_triggers (
                id         TEXT PRIMARY KEY,
                data       TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS runtime_state (
                server_id  TEXT PRIMARY KEY,
                data       TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS credentials (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS cloud_tokens (
                provider   TEXT PRIMARY KEY,
                data       TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sync_state (
                provider   TEXT PRIMARY KEY,
                data       TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS sync_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS pairings (
                pairing_id TEXT PRIMARY KEY,
                data       TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS ecdh_keys (
                id          INTEGER PRIMARY KEY DEFAULT 1,
                public_key  BLOB NOT NULL,
                private_key BLOB NOT NULL,
                created_at  TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS device_keys (
                id             INTEGER PRIMARY KEY DEFAULT 1,
                public_key_der BLOB NOT NULL,
                private_key    BLOB NOT NULL,
                security_level TEXT NOT NULL,
                created_at     TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS kv (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('schema_version', '1');
            INSERT OR IGNORE INTO schema_meta (key, value) VALUES ('using_default_dek', 'true');
            INSERT OR IGNORE INTO sync_meta (key, value) VALUES ('local_version', '0');
            ",
        )?;
        Ok(())
    }

    /// Backup the DB file (WAL-safe). Used before rekey.
    pub fn backup(&self) -> Result<PathBuf> {
        let conn = self.conn.lock().unwrap();
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .map_err(|e| Error::Other(e.to_string()))?;
        let backup_path = self.db_path.with_extension("db.bak");
        std::fs::copy(&self.db_path, &backup_path).map_err(Error::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &backup_path,
                std::fs::Permissions::from_mode(0o600),
            );
        }
        Ok(backup_path)
    }

    /// Rekey the DB (change encryption key). Must be called with the
    /// existing connection (already open with old DEK).
    pub fn rekey(&self, new_dek: &[u8; 32]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // 0. Backup first — abort if backup fails (design invariant)
        drop(conn);
        self.backup()?;

        let conn = self.conn.lock().unwrap();
        // 1. Exit WAL mode (rekey + WAL has known issues)
        conn.pragma_update(None, "journal_mode", "DELETE")
            .map_err(|e| Error::Other(e.to_string()))?;
        // 2. Checkpoint to flush WAL data
        conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .map_err(|e| Error::Other(e.to_string()))?;
        // 3. Rekey
        let key_hex = hex::encode(new_dek);
        conn.pragma_update(None, "rekey", format!("x'{}'", key_hex))
            .map_err(|e| Error::Other(e.to_string()))?;
        // 4. Restore WAL mode
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| Error::Other(e.to_string()))?;

        // Update marker
        conn.execute(
            "UPDATE schema_meta SET value = 'false' WHERE key = 'using_default_dek'",
            [],
        )
        .map_err(|e| Error::Other(e.to_string()))?;

        // 5. Delete backup (rename to stale on failure)
        let bak_path = self.db_path.with_extension("db.bak");
        if let Err(e) = std::fs::remove_file(&bak_path) {
            if bak_path.exists() {
                tracing::warn!("failed to delete .db.bak: {}, renaming", e);
                let stale = self.db_path.with_extension(format!(
                    "db.bak.stale.{}",
                    chrono::Utc::now().timestamp()
                ));
                let _ = std::fs::rename(&bak_path, &stale);
            }
        }
        Ok(())
    }

    /// Reset: delete DB file and recreate with default DEK.
    pub fn reset(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("DELETE FROM servers; DELETE FROM trigger_templates; DELETE FROM local_triggers; DELETE FROM runtime_state; DELETE FROM credentials; DELETE FROM cloud_tokens; DELETE FROM sync_state; DELETE FROM pairings; DELETE FROM ecdh_keys; DELETE FROM device_keys; DELETE FROM kv; UPDATE general_config SET data = '{}'; UPDATE sync_meta SET value = '0' WHERE key = 'local_version'; UPDATE schema_meta SET value = 'true' WHERE key = 'using_default_dek';").map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }
}

// === SECTION 2 END ===

// --- Servers CRUD ---

impl SqlCipherStorage {
    pub fn list_servers(&self) -> Result<Vec<(String, String, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, data, sort_order FROM servers ORDER BY sort_order, name")
            .map_err(|e| Error::Other(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn upsert_server(&self, id: &str, name: &str, data: &str, sort_order: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO servers (id, name, data, sort_order, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, data, sort_order, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn delete_server(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM servers WHERE id = ?1", params![id])
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Trigger templates CRUD ---

    #[allow(clippy::type_complexity)]
    pub fn list_templates(&self) -> Result<Vec<(String, String, String, i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, data, is_builtin, sort_order FROM trigger_templates ORDER BY sort_order, name")
            .map_err(|e| Error::Other(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn upsert_template(
        &self,
        id: &str,
        name: &str,
        data: &str,
        is_builtin: bool,
        sort_order: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO trigger_templates (id, name, data, is_builtin, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, data, is_builtin as i64, sort_order],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn delete_template(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM trigger_templates WHERE id = ?1", params![id])
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Local triggers CRUD ---

    pub fn list_local_triggers(&self) -> Result<Vec<(String, String, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, data, sort_order FROM local_triggers ORDER BY sort_order")
            .map_err(|e| Error::Other(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn upsert_local_trigger(&self, id: &str, data: &str, sort_order: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO local_triggers (id, data, sort_order) VALUES (?1, ?2, ?3)",
            params![id, data, sort_order],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn delete_local_trigger(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM local_triggers WHERE id = ?1", params![id])
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- General config (single-row JSON blob) ---

    pub fn get_general_config(&self) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT data FROM general_config WHERE id = 1",
            [],
            |row| row.get::<_, String>(0),
        ) {
            Ok(data) => Ok(Some(data)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn upsert_general_config(&self, data: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO general_config (id, data) VALUES (1, ?1)",
            params![data],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Runtime state ---

    pub fn get_runtime_state(&self, server_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT data FROM runtime_state WHERE server_id = ?1",
            params![server_id],
            |row| row.get::<_, String>(0),
        ) {
            Ok(data) => Ok(Some(data)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn upsert_runtime_state(&self, server_id: &str, data: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO runtime_state (server_id, data, updated_at) VALUES (?1, ?2, ?3)",
            params![server_id, data, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn delete_runtime_state(&self, server_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM runtime_state WHERE server_id = ?1", params![server_id])
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Credentials ---

    pub fn get_credential(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT value FROM credentials WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn upsert_credential(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO credentials (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn delete_credential(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM credentials WHERE key = ?1", params![key])
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn list_credentials(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key, value FROM credentials")
            .map_err(|e| Error::Other(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn delete_credentials_for_server(&self, server_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM credentials WHERE key LIKE ?1",
            params![format!("%::{}::%", server_id)],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- KV (device_id etc.) ---

    pub fn get_kv(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT value FROM kv WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn set_kv(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO kv (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Sync meta ---

    pub fn get_sync_meta(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT value FROM sync_meta WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn set_sync_meta(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO sync_meta (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn increment_local_version(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE sync_meta SET value = CAST(CAST(value AS INTEGER) + 1 AS TEXT) WHERE key = 'local_version'",
            [],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Cloud tokens ---

    pub fn get_cloud_token(&self, provider: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT data FROM cloud_tokens WHERE provider = ?1",
            params![provider],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn upsert_cloud_token(&self, provider: &str, data: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO cloud_tokens (provider, data, updated_at) VALUES (?1, ?2, ?3)",
            params![provider, data, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn delete_cloud_token(&self, provider: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM cloud_tokens WHERE provider = ?1", params![provider])
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Sync state ---

    pub fn get_sync_state(&self, provider: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT data FROM sync_state WHERE provider = ?1",
            params![provider],
            |row| row.get::<_, String>(0),
        ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn upsert_sync_state(&self, provider: &str, data: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO sync_state (provider, data) VALUES (?1, ?2)",
            params![provider, data],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Pairings (PC only) ---

    pub fn list_pairings(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT pairing_id, data FROM pairings")
            .map_err(|e| Error::Other(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|e| Error::Other(e.to_string()))?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| Error::Other(e.to_string()))?);
        }
        Ok(result)
    }

    pub fn upsert_pairing(&self, pairing_id: &str, data: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO pairings (pairing_id, data, updated_at) VALUES (?1, ?2, ?3)",
            params![pairing_id, data, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    pub fn delete_pairing(&self, pairing_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM pairings WHERE pairing_id = ?1", params![pairing_id])
            .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- ECDH keys (PC only) ---

    #[allow(clippy::type_complexity)]
    pub fn get_ecdh_key(&self) -> Result<Option<(Vec<u8>, Vec<u8>, String)>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT public_key, private_key, created_at FROM ecdh_keys WHERE id = 1",
            [],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, String>(2)?)),
        ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn upsert_ecdh_key(&self, public_key: &[u8], private_key: &[u8]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO ecdh_keys (id, public_key, private_key, created_at) VALUES (1, ?1, ?2, ?3)",
            params![public_key, private_key, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }

    // --- Device keys (PC only) ---

    #[allow(clippy::type_complexity)]
    pub fn get_device_key(&self) -> Result<Option<(Vec<u8>, Vec<u8>, String, String)>> {
        let conn = self.conn.lock().unwrap();
        match conn.query_row(
            "SELECT public_key_der, private_key, security_level, created_at FROM device_keys WHERE id = 1",
            [],
            |row| Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            )),
        ) {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(Error::Other(e.to_string())),
        }
    }

    pub fn upsert_device_key(
        &self,
        public_key_der: &[u8],
        private_key: &[u8],
        security_level: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO device_keys (id, public_key_der, private_key, security_level, created_at) VALUES (1, ?1, ?2, ?3, ?4)",
            params![public_key_der, private_key, security_level, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| Error::Other(e.to_string()))?;
        Ok(())
    }
}

// === SECTION 3 END ===

/// Open an existing DB file with the given key and verify the key is correct.
/// Only for existing DBs — new DBs accept any key (sqlite_master is empty).
/// Caller should check `path.exists()` first and use `create_new` for new DBs.
pub fn open_with_key(path: &Path, dek: &[u8; 32]) -> std::result::Result<Connection, OpenResult> {
    let conn = Connection::open(path).map_err(|e| OpenResult::Other(e.to_string()))?;
    let key_hex = hex::encode(dek);
    // Set the key — this doesn't verify it yet, SQLCipher defers to first page read
    conn.pragma_update(None, "key", format!("x'{}'", key_hex))
        .map_err(map_sqlcipher_error)?;
    // Verify key by reading sqlite_master — this triggers actual decryption
    // and will fail with "file is not a database" if the key is wrong
    match conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    }) {
        Ok(_) => {
            // Key verified — now set other pragmas
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(|e| OpenResult::Other(e.to_string()))?;
            conn.pragma_update(None, "synchronous", "FULL")
                .map_err(|e| OpenResult::Other(e.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
                let wal = path.with_extension("db-wal");
                let shm = path.with_extension("db-shm");
                if wal.exists() {
                    let _ = std::fs::set_permissions(&wal, std::fs::Permissions::from_mode(0o600));
                }
                if shm.exists() {
                    let _ = std::fs::set_permissions(&shm, std::fs::Permissions::from_mode(0o600));
                }
            }
            Ok(conn)
        }
        Err(e) => Err(map_sqlcipher_error(e)),
    }
}

/// Map rusqlite error to OpenResult, detecting SQLCipher wrong-key errors.
fn map_sqlcipher_error(e: rusqlite::Error) -> OpenResult {
    let msg = e.to_string();
    if msg.contains("not a database") || msg.contains("file is not a database") {
        OpenResult::WrongKey
    } else if msg.contains("database disk image is malformed")
        || msg.contains("database is corrupt")
        || msg.contains("corrupt")
    {
        OpenResult::Corrupt
    } else {
        match e {
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ffi::ErrorCode::NotADatabase =>
            {
                OpenResult::WrongKey
            }
            rusqlite::Error::SqliteFailure(err, _)
                if err.code == rusqlite::ffi::ErrorCode::DatabaseCorrupt =>
            {
                OpenResult::Corrupt
            }
            _ => OpenResult::Other(msg),
        }
    }
}

/// Open DB with automatic crash recovery from .db.bak.
/// Both WrongKey and Corrupt trigger recovery attempts (rekey partial
/// completion can produce either error).
pub fn open_or_recover(
    db_path: &Path,
    dek: &[u8; 32],
) -> std::result::Result<Connection, OpenResult> {
    match open_with_key(db_path, dek) {
        Ok(conn) => Ok(conn),
        Err(e @ (OpenResult::WrongKey | OpenResult::Corrupt)) => {
            let bak_path = db_path.with_extension("db.bak");
            if !bak_path.exists() {
                return Err(e);
            }
            tracing::error!("DB open failed ({:?}), attempting recovery from .db.bak", e);
            match open_with_key(&bak_path, dek) {
                Ok(conn) => {
                    drop(conn);
                    // Replace corrupt DB with backup
                    std::fs::rename(&bak_path, db_path).or_else(|_| {
                        std::fs::copy(&bak_path, db_path)?;
                        std::fs::remove_file(&bak_path)?;
                        Ok::<(), std::io::Error>(())
                    }).map_err(|e| OpenResult::Other(e.to_string()))?;
                    tracing::info!("DB recovered from .db.bak successfully");
                    open_with_key(db_path, dek)
                }
                Err(OpenResult::WrongKey) => {
                    tracing::warn!("backup also rejected with WrongKey, not a rekey crash scenario");
                    Err(OpenResult::WrongKey)
                }
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

// === SECTION 4 END ===

/// ConfigStorage implementation backed by SQLCipher.
/// Wraps SqlCipherStorage and implements the ConfigStorage trait.
pub struct SqlCipherConfigStorage {
    storage: std::sync::Arc<SqlCipherStorage>,
}

impl SqlCipherConfigStorage {
    pub fn new(storage: std::sync::Arc<SqlCipherStorage>) -> Self {
        Self { storage }
    }
}

use super::config::{Config, GeneralConfig, ServerConfig, TriggerTemplate};
use super::storage::ConfigStorage;

fn load_config_from_storage(storage: &SqlCipherStorage) -> Result<Config> {
    // General config
    let general = storage
        .get_general_config()?
        .and_then(|data| serde_json::from_str::<GeneralConfig>(&data).ok())
        .unwrap_or_default();

    // Servers
    let mut servers = Vec::new();
    for (id, _name, data, _sort_order) in storage.list_servers()? {
        match serde_json::from_str::<ServerConfig>(&data) {
            Ok(s) => servers.push(s),
            Err(e) => tracing::warn!("failed to deserialize server {}: {}", id, e),
        }
    }

    // Trigger templates
    let mut templates = Vec::new();
    for (id, _name, data, _is_builtin, _sort_order) in storage.list_templates()? {
        match serde_json::from_str::<TriggerTemplate>(&data) {
            Ok(t) => templates.push(t),
            Err(e) => tracing::warn!("failed to deserialize template {}: {}", id, e),
        }
    }

    // Local triggers — stored as JSON array
    let local_triggers = storage.get_kv("local_triggers")?;
    let local_triggers = local_triggers
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default();

    Ok(Config {
        version: 2,
        general,
        trigger_templates: templates,
        servers,
        local_triggers,
    })
}

fn save_config_to_storage(storage: &SqlCipherStorage, config: &Config) -> Result<()> {
    // General config (single-row JSON blob)
    let general_json = serde_json::to_string(&config.general)?;
    storage.upsert_general_config(&general_json)?;

    // Servers — full replace (delete all + re-insert)
    // This is simpler than diffing and handles deletions correctly.
    let conn_rows = storage.list_servers()?;
    let _ = conn_rows; // we delete all and re-insert
    for (i, server) in config.servers.iter().enumerate() {
        let data = serde_json::to_string(server)?;
        storage.upsert_server(&server.id, &server.name, &data, i as i64)?;
    }
    // Delete servers that are no longer in config
    let existing_ids: std::collections::HashSet<String> =
        config.servers.iter().map(|s| s.id.clone()).collect();
    for (id, _, _, _) in storage.list_servers()? {
        if !existing_ids.contains(&id) {
            storage.delete_server(&id)?;
        }
    }

    // Trigger templates
    for (i, template) in config.trigger_templates.iter().enumerate() {
        let data = serde_json::to_string(template)?;
        storage.upsert_template(
            &template.id,
            &template.name,
            &data,
            template.built_in,
            i as i64,
        )?;
    }
    // Delete templates no longer in config
    let existing_template_ids: std::collections::HashSet<String> =
        config.trigger_templates.iter().map(|t| t.id.clone()).collect();
    for (id, _, _, _, _) in storage.list_templates()? {
        if !existing_template_ids.contains(&id) {
            storage.delete_template(&id)?;
        }
    }

    // Local triggers — stored as JSON array in kv
    let local_triggers_json = serde_json::to_string(&config.local_triggers)?;
    storage.set_kv("local_triggers", &local_triggers_json)?;

    // Increment local version for sync conflict detection
    storage.increment_local_version()?;

    Ok(())
}

impl ConfigStorage for SqlCipherConfigStorage {
    fn load(&self) -> Result<Config> {
        load_config_from_storage(&self.storage)
    }

    fn save(&self, config: &Config) -> Result<()> {
        save_config_to_storage(&self.storage, config)
    }

    fn exists(&self) -> bool {
        self.storage.db_file_exists()
    }

    fn backup(&self) -> Result<PathBuf> {
        self.storage.backup()
    }
}

// === SECTION 5 END ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_storage() -> (tempfile::TempDir, SqlCipherStorage) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap();
        (dir, storage)
    }

    #[test]
    fn test_create_and_open() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap();
        assert!(path.exists());
        // Can load empty config
        let cfg = load_config_from_storage(&storage).unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn test_open_with_wrong_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let _storage = SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap();
        let wrong_key = [0u8; 32];
        let result = open_with_key(&path, &wrong_key);
        assert!(matches!(result, Err(OpenResult::WrongKey)));
    }

    #[test]
    fn test_open_with_correct_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let _storage = SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap();
        let result = open_with_key(&path, &DEFAULT_DEK);
        assert!(result.is_ok());
    }

    #[test]
    fn test_server_crud() {
        let (_dir, storage) = test_storage();
        storage.upsert_server("srv1", "My Server", "{}", 0).unwrap();
        storage.upsert_server("srv2", "Other", "{}", 1).unwrap();
        let servers = storage.list_servers().unwrap();
        assert_eq!(servers.len(), 2);
        storage.delete_server("srv1").unwrap();
        let servers = storage.list_servers().unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].0, "srv2");
    }

    #[test]
    fn test_credential_crud() {
        let (_dir, storage) = test_storage();
        storage.upsert_credential("key1", "secret1").unwrap();
        assert_eq!(
            storage.get_credential("key1").unwrap(),
            Some("secret1".to_string())
        );
        assert_eq!(storage.get_credential("nonexistent").unwrap(), None);
        storage.delete_credential("key1").unwrap();
        assert_eq!(storage.get_credential("key1").unwrap(), None);
    }

    #[test]
    fn test_runtime_state_crud() {
        let (_dir, storage) = test_storage();
        storage
            .upsert_runtime_state("srv1", r#"{"last_known_ip":"1.2.3.4"}"#)
            .unwrap();
        let data = storage.get_runtime_state("srv1").unwrap();
        assert!(data.is_some());
        assert!(data.unwrap().contains("1.2.3.4"));
        storage.delete_runtime_state("srv1").unwrap();
        assert!(storage.get_runtime_state("srv1").unwrap().is_none());
    }

    #[test]
    fn test_kv() {
        let (_dir, storage) = test_storage();
        storage.set_kv("device_id", "abc123").unwrap();
        assert_eq!(
            storage.get_kv("device_id").unwrap(),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn test_sync_meta_version() {
        let (_dir, storage) = test_storage();
        // Initial version should be 0
        assert_eq!(
            storage.get_sync_meta("local_version").unwrap(),
            Some("0".to_string())
        );
        storage.increment_local_version().unwrap();
        assert_eq!(
            storage.get_sync_meta("local_version").unwrap(),
            Some("1".to_string())
        );
        storage.increment_local_version().unwrap();
        assert_eq!(
            storage.get_sync_meta("local_version").unwrap(),
            Some("2".to_string())
        );
    }

    #[test]
    fn test_rekey() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap();
        storage.upsert_credential("key1", "secret").unwrap();

        let new_dek = [0xAA; 32];
        storage.rekey(&new_dek).unwrap();

        // Old key should fail
        let result = open_with_key(&path, &DEFAULT_DEK);
        assert!(matches!(result, Err(OpenResult::WrongKey)));

        // New key should work
        let conn = open_with_key(&path, &new_dek).unwrap();
        let val: String = conn
            .query_row(
                "SELECT value FROM credentials WHERE key = 'key1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(val, "secret");

        // .db.bak should be deleted after successful rekey
        assert!(!path.with_extension("db.bak").exists());
    }

    #[test]
    fn test_is_using_default_dek() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let storage = SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap();
        assert!(storage.is_using_default_dek());

        let new_dek = [0xBB; 32];
        storage.rekey(&new_dek).unwrap();

        // Need to reopen with new key
        let storage2 = SqlCipherStorage::open(&path, &new_dek).unwrap();
        assert!(!storage2.is_using_default_dek());
    }

    #[test]
    fn test_config_save_load_round_trip() {
        let (_dir, storage) = test_storage();
        let config_storage = SqlCipherConfigStorage::new(std::sync::Arc::new(storage));

        let mut config = Config::default();
        // Use JSON deserialization to create a ServerConfig with defaults
        let server: ServerConfig = serde_json::from_str(
            r#"{"id":"srv1","name":"Test Server","ssh":{"host":"","port":22,"user":""},"proxy":{"type":"none"}}"#
        ).unwrap();
        config.servers.push(server);
        config.general.theme = "dark".into();

        config_storage.save(&config).unwrap();
        let loaded = config_storage.load().unwrap();

        assert_eq!(loaded.servers.len(), 1);
        assert_eq!(loaded.servers[0].id, "srv1");
        assert_eq!(loaded.servers[0].name, "Test Server");
        assert_eq!(loaded.general.theme, "dark");
    }

    #[test]
    fn test_open_or_recover_no_backup() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.db");
        let _storage = SqlCipherStorage::create_new(&path, &DEFAULT_DEK).unwrap();
        let wrong_key = [0u8; 32];
        // No backup exists, should return WrongKey
        let result = open_or_recover(&path, &wrong_key);
        assert!(matches!(result, Err(OpenResult::WrongKey)));
    }

    #[test]
    fn test_delete_credentials_for_server() {
        let (_dir, storage) = test_storage();
        storage
            .upsert_credential("termfast::srv1::password", "pass1")
            .unwrap();
        storage
            .upsert_credential("termfast::srv2::password", "pass2")
            .unwrap();
        storage.delete_credentials_for_server("srv1").unwrap();
        assert_eq!(
            storage.get_credential("termfast::srv1::password").unwrap(),
            None
        );
        assert_eq!(
            storage.get_credential("termfast::srv2::password").unwrap(),
            Some("pass2".to_string())
        );
    }

    #[test]
    fn test_ecdh_key_crud() {
        let (_dir, storage) = test_storage();
        assert!(storage.get_ecdh_key().unwrap().is_none());
        storage
            .upsert_ecdh_key(&[1, 2, 3], &[4, 5, 6])
            .unwrap();
        let (pub_key, priv_key, _) = storage.get_ecdh_key().unwrap().unwrap();
        assert_eq!(pub_key, vec![1, 2, 3]);
        assert_eq!(priv_key, vec![4, 5, 6]);
    }

    #[test]
    fn test_device_key_crud() {
        let (_dir, storage) = test_storage();
        assert!(storage.get_device_key().unwrap().is_none());
        storage
            .upsert_device_key(&[1, 2, 3], &[4, 5, 6], "high")
            .unwrap();
        let (pub_key, priv_key, level, _) = storage.get_device_key().unwrap().unwrap();
        assert_eq!(pub_key, vec![1, 2, 3]);
        assert_eq!(priv_key, vec![4, 5, 6]);
        assert_eq!(level, "high");
    }
}

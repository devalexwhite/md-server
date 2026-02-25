use anyhow::{Context, Result, anyhow};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
// rand_core 0.6 is what password-hash/argon2 depends on; must match that version.
use rand_core::OsRng;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use std::{path::Path, str::FromStr};

#[derive(Debug, Default, Clone)]
pub struct RequestStats {
    pub last_7m: i64,
    pub last_1h: i64,
    pub last_24h: i64,
}

pub async fn init_pool(db_path: &Path) -> Result<SqlitePool> {
    let url = format!("sqlite:{}", db_path.display());
    let opts = SqliteConnectOptions::from_str(&url)
        .context("Invalid DB path")?
        .create_if_missing(true);

    let pool = SqlitePool::connect_with(opts)
        .await
        .context("Failed to open SQLite database")?;

    init_schema(&pool).await?;
    prune_old_requests(&pool).await?;

    Ok(pool)
}

async fn init_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            username      TEXT    NOT NULL UNIQUE,
            password_hash TEXT    NOT NULL,
            created_at    TEXT    NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await
    .context("Failed to create users table")?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS requests (
            id        INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT    NOT NULL DEFAULT (datetime('now')),
            route     TEXT    NOT NULL,
            referer   TEXT,
            ip_hash   TEXT,
            browser   TEXT,
            os        TEXT
        )",
    )
    .execute(pool)
    .await
    .context("Failed to create requests table")?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_requests_timestamp ON requests(timestamp)")
        .execute(pool)
        .await
        .context("Failed to create requests index")?;

    Ok(())
}

async fn prune_old_requests(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM requests WHERE timestamp < datetime('now', '-3 months')")
        .execute(pool)
        .await
        .context("Failed to prune old requests")?;
    Ok(())
}

pub async fn get_request_stats(pool: &SqlitePool) -> Result<RequestStats> {
    let row = sqlx::query(
        "SELECT
            COUNT(CASE WHEN timestamp > datetime('now', '-7 minutes') THEN 1 END) AS last_7m,
            COUNT(CASE WHEN timestamp > datetime('now', '-1 hour')    THEN 1 END) AS last_1h,
            COUNT(CASE WHEN timestamp > datetime('now', '-24 hours')  THEN 1 END) AS last_24h
         FROM requests",
    )
    .fetch_one(pool)
    .await?;

    Ok(RequestStats {
        last_7m: row.get::<i64, _>("last_7m"),
        last_1h: row.get::<i64, _>("last_1h"),
        last_24h: row.get::<i64, _>("last_24h"),
    })
}

pub async fn insert_request(
    pool: &SqlitePool,
    route: &str,
    referer: Option<&str>,
    ip_hash: Option<&str>,
    browser: Option<&str>,
    os: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO requests (route, referer, ip_hash, browser, os) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(route)
    .bind(referer)
    .bind(ip_hash)
    .bind(browser)
    .bind(os)
    .execute(pool)
    .await
    .context("Failed to insert request")?;
    Ok(())
}

/// Hash a password with argon2id and return the PHC string.
pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("Password hashing failed: {}", e))?
        .to_string();
    Ok(hash)
}

/// Add a user to the database. Returns an error if the username already exists.
pub async fn add_user(pool: &SqlitePool, username: &str, password: &str) -> Result<()> {
    let hash = hash_password(password)?;
    sqlx::query("INSERT INTO users (username, password_hash) VALUES (?, ?)")
        .bind(username)
        .bind(&hash)
        .execute(pool)
        .await
        .context("Failed to insert user — username may already exist")?;
    Ok(())
}

/// Verify a username/password pair against the database.
/// Returns `false` on any error or if credentials are wrong.
pub async fn verify_user(pool: &SqlitePool, username: &str, password: &str) -> bool {
    let row = sqlx::query("SELECT password_hash FROM users WHERE username = ?")
        .bind(username)
        .fetch_optional(pool)
        .await;

    let hash_str: String = match row {
        Ok(Some(r)) => r.get("password_hash"),
        _ => return false,
    };

    let parsed = match PasswordHash::new(&hash_str) {
        Ok(h) => h,
        Err(_) => return false,
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

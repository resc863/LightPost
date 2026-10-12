//! Schema migrations tracked with `PRAGMA user_version`.
//!
//! Each step runs once, in order, inside its own transaction. Steps are never
//! edited after release; a schema change is always a new step at the end.

use rusqlite::{Connection, Transaction};

use super::replace_tags;
use crate::{error::AppError, markdown::render_markdown};

enum Step {
    Sql(&'static str),
    Rust(fn(&Transaction<'_>) -> rusqlite::Result<()>),
}

const MIGRATIONS: &[Step] = &[
    // 1. The schema of the first release. Databases created by that release
    //    already have it, so it is written to be a no-op for them.
    Step::Sql(
        "CREATE TABLE IF NOT EXISTS posts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            slug TEXT NOT NULL UNIQUE,
            title TEXT NOT NULL,
            excerpt TEXT NOT NULL DEFAULT '',
            body_markdown TEXT NOT NULL,
            tags TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL CHECK(status IN ('draft', 'published')),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            published_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_posts_status_created ON posts(status, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_posts_slug ON posts(slug);",
    ),
    // 2. Tags move from a comma-joined column to their own tables.
    Step::Rust(move_tags_to_tables),
    // 3. Cached HTML, publish-date ordering, and no duplicate slug index.
    Step::Rust(cache_html_and_reindex),
];

pub fn run(connection: &mut Connection) -> Result<(), AppError> {
    let current: usize = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current > MIGRATIONS.len() {
        return Err(AppError::Internal(format!(
            "database schema version {current} is newer than this build supports ({})",
            MIGRATIONS.len()
        )));
    }
    for (index, step) in MIGRATIONS.iter().enumerate().skip(current) {
        let transaction = connection.transaction()?;
        match step {
            Step::Sql(sql) => transaction.execute_batch(sql)?,
            Step::Rust(migrate) => migrate(&transaction)?,
        }
        transaction.pragma_update(None, "user_version", index + 1)?;
        transaction.commit()?;
    }
    Ok(())
}

pub fn latest_version() -> usize {
    MIGRATIONS.len()
}

fn move_tags_to_tables(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE tags (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );
        CREATE TABLE post_tags (
            post_id INTEGER NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
            tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            position INTEGER NOT NULL,
            PRIMARY KEY (post_id, tag_id)
        );
        CREATE INDEX idx_post_tags_tag ON post_tags(tag_id);",
    )?;
    let legacy: Vec<(i64, String)> = {
        let mut statement = transaction.prepare("SELECT id, tags FROM posts")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (post_id, tags) in legacy {
        let tags = crate::domain::parse_tag_list(&tags);
        replace_tags(transaction, post_id, &tags)?;
    }
    transaction.execute_batch("ALTER TABLE posts DROP COLUMN tags;")
}

fn cache_html_and_reindex(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE posts ADD COLUMN body_html TEXT NOT NULL DEFAULT '';
        UPDATE posts SET published_at = created_at WHERE status = 'published' AND published_at IS NULL;
        DROP INDEX IF EXISTS idx_posts_slug;
        DROP INDEX IF EXISTS idx_posts_status_created;
        CREATE INDEX idx_posts_status_published ON posts(status, published_at DESC);",
    )?;
    let bodies: Vec<(i64, String)> = {
        let mut statement = transaction.prepare("SELECT id, body_markdown FROM posts")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let mut update = transaction.prepare("UPDATE posts SET body_html = ?1 WHERE id = ?2")?;
    for (id, markdown) in bodies {
        update.execute(rusqlite::params![render_markdown(&markdown), id])?;
    }
    Ok(())
}

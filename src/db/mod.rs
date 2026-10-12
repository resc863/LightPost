mod migrations;

use std::{
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex, MutexGuard, PoisonError,
    },
    time::Duration,
};

use rusqlite::{
    params, params_from_iter, types::Value, Connection, OpenFlags, OptionalExtension, Row,
    Transaction,
};

use crate::{
    domain::{normalize_tag, Page, Post, PostInput, PostStatus, PostSummary},
    error::AppError,
};

pub use migrations::latest_version as schema_version;

const READER_CONNECTIONS: usize = 4;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
/// Separates tag names inside `group_concat`; tags never contain control characters.
const TAG_SEPARATOR: char = '\u{1f}';

const SUMMARY_COLUMNS: &str =
    "p.id, p.slug, p.title, p.excerpt, p.status, p.created_at, p.updated_at, p.published_at,
    (SELECT group_concat(t.name, char(31) ORDER BY pt.position)
       FROM post_tags pt JOIN tags t ON t.id = pt.tag_id
      WHERE pt.post_id = p.id) AS tags";
const POST_COLUMNS: &str =
    "p.id, p.slug, p.title, p.excerpt, p.status, p.created_at, p.updated_at, p.published_at,
    (SELECT group_concat(t.name, char(31) ORDER BY pt.position)
       FROM post_tags pt JOIN tags t ON t.id = pt.tag_id
      WHERE pt.post_id = p.id) AS tags,
    p.body_markdown, p.body_html";
const PUBLIC_ORDER: &str = "ORDER BY p.published_at DESC, p.id DESC";

/// SQLite storage with one writer and a few read-only connections.
///
/// Every method blocks; async code calls them through `AppState::db`, which
/// runs them on Tokio's blocking pool.
pub struct Repository {
    writer: Mutex<Connection>,
    /// Empty for in-memory databases, where the writer also serves reads.
    readers: Vec<Mutex<Connection>>,
    next_reader: AtomicUsize,
}

impl Repository {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref();
        let mut writer = Connection::open(path)?;
        configure(&writer)?;
        writer.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        writer.pragma_update(None, "synchronous", "NORMAL")?;
        migrations::run(&mut writer)?;

        let readers = (0..READER_CONNECTIONS)
            .map(|_| {
                let reader = Connection::open_with_flags(
                    path,
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )?;
                configure(&reader)?;
                Ok(Mutex::new(reader))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            next_reader: AtomicUsize::new(0),
        })
    }

    pub fn in_memory() -> Result<Self, AppError> {
        let mut writer = Connection::open_in_memory()?;
        configure(&writer)?;
        migrations::run(&mut writer)?;
        Ok(Self {
            writer: Mutex::new(writer),
            readers: Vec::new(),
            next_reader: AtomicUsize::new(0),
        })
    }

    pub fn schema_version(&self) -> Result<usize, AppError> {
        Ok(self
            .reader()
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    pub fn create_post(&self, input: &PostInput) -> Result<Post, AppError> {
        let post = input.validate()?;
        let mut connection = self.writer();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO posts (slug, title, excerpt, body_markdown, body_html, status, published_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, CASE WHEN ?6 = 'published' THEN CURRENT_TIMESTAMP END)",
            params![post.slug, post.title, post.excerpt, post.body_markdown, post.body_html, post.status],
        )?;
        let id = transaction.last_insert_rowid();
        replace_tags(&transaction, id, &post.tags)?;
        transaction.commit()?;
        find_post(&connection, "p.id = ?1", params![id])?.ok_or(AppError::NotFound)
    }

    /// Replaces a post's content. The first publish sets `published_at`; later
    /// edits keep it so the post stays in place in the list.
    pub fn update_post(&self, id: i64, input: &PostInput) -> Result<Post, AppError> {
        let post = input.validate()?;
        let mut connection = self.writer();
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE posts
                SET slug = ?1, title = ?2, excerpt = ?3, body_markdown = ?4, body_html = ?5, status = ?6,
                    updated_at = CURRENT_TIMESTAMP,
                    published_at = CASE WHEN ?6 = 'published' THEN COALESCE(published_at, CURRENT_TIMESTAMP)
                                        ELSE published_at END
              WHERE id = ?7",
            params![post.slug, post.title, post.excerpt, post.body_markdown, post.body_html, post.status, id],
        )?;
        if changed == 0 {
            return Err(AppError::NotFound);
        }
        replace_tags(&transaction, id, &post.tags)?;
        transaction.commit()?;
        find_post(&connection, "p.id = ?1", params![id])?.ok_or(AppError::NotFound)
    }

    pub fn delete_post(&self, id: i64) -> Result<(), AppError> {
        let mut connection = self.writer();
        let transaction = connection.transaction()?;
        if transaction.execute("DELETE FROM posts WHERE id = ?1", [id])? == 0 {
            return Err(AppError::NotFound);
        }
        delete_unused_tags(&transaction)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn get_by_id(&self, id: i64) -> Result<Option<Post>, AppError> {
        find_post(&self.reader(), "p.id = ?1", params![id])
    }

    pub fn get_by_slug(&self, slug: &str) -> Result<Option<Post>, AppError> {
        find_post(&self.reader(), "p.slug = ?1", params![slug])
    }

    pub fn get_published_by_slug(&self, slug: &str) -> Result<Option<Post>, AppError> {
        find_post(
            &self.reader(),
            "p.slug = ?1 AND p.status = 'published'",
            params![slug],
        )
    }

    pub fn list_published(&self, page: u32, per_page: u32) -> Result<Page<PostSummary>, AppError> {
        self.page_of_published("1 = 1", None, page, per_page)
    }

    /// Published posts, newest first; `limit` keeps feeds short.
    pub fn published_summaries(&self, limit: Option<u32>) -> Result<Vec<PostSummary>, AppError> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM posts p WHERE p.status = 'published' {PUBLIC_ORDER} LIMIT ?1"
        );
        let limit = limit.map_or(-1, i64::from);
        query_summaries(&self.reader(), &sql, params![limit])
    }

    /// Every post including drafts, most recently edited first, for the admin screen.
    pub fn list_all(&self) -> Result<Vec<PostSummary>, AppError> {
        let sql =
            format!("SELECT {SUMMARY_COLUMNS} FROM posts p ORDER BY p.updated_at DESC, p.id DESC");
        query_summaries(&self.reader(), &sql, [])
    }

    /// Substring search over title, excerpt, body and tags. An empty term finds nothing.
    pub fn search_published(
        &self,
        term: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Page<PostSummary>, AppError> {
        let term = term.trim();
        if term.is_empty() {
            return Ok(Page::empty(per_page));
        }
        let pattern = format!("%{}%", escape_like(term));
        self.page_of_published(
            "(p.title LIKE ?1 ESCAPE '\\' OR p.excerpt LIKE ?1 ESCAPE '\\' OR p.body_markdown LIKE ?1 ESCAPE '\\'
              OR EXISTS (SELECT 1 FROM post_tags pt JOIN tags t ON t.id = pt.tag_id
                          WHERE pt.post_id = p.id AND t.name LIKE ?1 ESCAPE '\\'))",
            Some(&pattern),
            page,
            per_page,
        )
    }

    pub fn list_by_tag(
        &self,
        tag: &str,
        page: u32,
        per_page: u32,
    ) -> Result<Page<PostSummary>, AppError> {
        self.page_of_published(
            "EXISTS (SELECT 1 FROM post_tags pt JOIN tags t ON t.id = pt.tag_id
                      WHERE pt.post_id = p.id AND t.name = ?1)",
            Some(&normalize_tag(tag)),
            page,
            per_page,
        )
    }

    /// Runs a published-only query. `filter` may refer to `argument` as `?1`.
    fn page_of_published(
        &self,
        filter: &str,
        argument: Option<&str>,
        page: u32,
        per_page: u32,
    ) -> Result<Page<PostSummary>, AppError> {
        let page = page.max(1);
        let per_page = per_page.max(1);
        let offset = i64::from(page - 1) * i64::from(per_page);
        let filter_params: Vec<Value> = argument
            .map(|value| Value::Text(value.to_owned()))
            .into_iter()
            .collect();
        let connection = self.reader();
        let total: u64 = connection.query_row(
            &format!("SELECT COUNT(*) FROM posts p WHERE p.status = 'published' AND {filter}"),
            params_from_iter(&filter_params),
            |row| row.get(0),
        )?;
        // Unnumbered `?` placeholders continue after the filter's `?1`.
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM posts p
              WHERE p.status = 'published' AND {filter}
              {PUBLIC_ORDER} LIMIT ? OFFSET ?"
        );
        let mut query_params = filter_params;
        query_params.extend([Value::Integer(i64::from(per_page)), Value::Integer(offset)]);
        let items = query_summaries(&connection, &sql, params_from_iter(&query_params))?;
        Ok(Page {
            items,
            page,
            per_page,
            total,
        })
    }

    fn writer(&self) -> MutexGuard<'_, Connection> {
        self.writer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn reader(&self) -> MutexGuard<'_, Connection> {
        if self.readers.is_empty() {
            return self.writer();
        }
        let start = self.next_reader.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        for offset in 0..self.readers.len() {
            if let Ok(guard) = self.readers[(start + offset) % self.readers.len()].try_lock() {
                return guard;
            }
        }
        self.readers[start]
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

fn configure(connection: &Connection) -> rusqlite::Result<()> {
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.pragma_update(None, "foreign_keys", "ON")
}

/// Sets a post's tags in the given order and removes tags no post uses any more.
pub(crate) fn replace_tags(
    transaction: &Transaction<'_>,
    post_id: i64,
    tags: &[String],
) -> rusqlite::Result<()> {
    transaction.execute("DELETE FROM post_tags WHERE post_id = ?1", [post_id])?;
    let mut insert_tag = transaction
        .prepare_cached("INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING")?;
    let mut find_tag = transaction.prepare_cached("SELECT id FROM tags WHERE name = ?1")?;
    let mut link = transaction
        .prepare_cached("INSERT INTO post_tags (post_id, tag_id, position) VALUES (?1, ?2, ?3)")?;
    for (position, tag) in tags.iter().enumerate() {
        insert_tag.execute([tag])?;
        let tag_id: i64 = find_tag.query_row([tag], |row| row.get(0))?;
        link.execute(params![post_id, tag_id, position as i64])?;
    }
    delete_unused_tags(transaction)
}

fn delete_unused_tags(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute(
        "DELETE FROM tags WHERE NOT EXISTS (SELECT 1 FROM post_tags WHERE post_tags.tag_id = tags.id)",
        [],
    )?;
    Ok(())
}

/// Escapes `%`, `_` and the escape character itself for `LIKE ... ESCAPE '\'`.
pub fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn find_post(
    connection: &Connection,
    filter: &str,
    params: impl rusqlite::Params,
) -> Result<Option<Post>, AppError> {
    let sql = format!("SELECT {POST_COLUMNS} FROM posts p WHERE {filter}");
    Ok(connection
        .query_row(&sql, params, post_from_row)
        .optional()?)
}

fn query_summaries(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<PostSummary>, AppError> {
    let mut statement = connection.prepare_cached(sql)?;
    let rows = statement.query_map(params, summary_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<PostSummary> {
    Ok(PostSummary {
        id: row.get(0)?,
        slug: row.get(1)?,
        title: row.get(2)?,
        excerpt: row.get(3)?,
        status: row.get::<_, PostStatus>(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        published_at: row.get(7)?,
        tags: split_tags(row.get::<_, Option<String>>(8)?),
    })
}

fn post_from_row(row: &Row<'_>) -> rusqlite::Result<Post> {
    let summary = summary_from_row(row)?;
    Ok(Post {
        id: summary.id,
        slug: summary.slug,
        title: summary.title,
        excerpt: summary.excerpt,
        status: summary.status,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
        published_at: summary.published_at,
        tags: summary.tags,
        body_markdown: row.get(9)?,
        body_html: row.get(10)?,
    })
}

fn split_tags(tags: Option<String>) -> Vec<String> {
    tags.map(|tags| {
        tags.split(TAG_SEPARATOR)
            .filter(|tag| !tag.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(title: &str, status: PostStatus, tags: &[&str]) -> PostInput {
        PostInput {
            title: title.to_owned(),
            excerpt: "요약".to_owned(),
            body_markdown: "# 본문\n\n내용".to_owned(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            status,
            ..PostInput::default()
        }
    }

    fn temp_db_path(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lightpost-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("blog.sqlite")
    }

    #[test]
    fn repository_hides_drafts_from_public_queries() {
        let repository = Repository::in_memory().unwrap();
        repository
            .create_post(&input("공개 글", PostStatus::Published, &[]))
            .unwrap();
        repository
            .create_post(&input("비공개 글", PostStatus::Draft, &[]))
            .unwrap();
        assert_eq!(repository.list_published(1, 10).unwrap().total, 1);
        assert!(repository
            .get_published_by_slug("비공개-글")
            .unwrap()
            .is_none());
        assert_eq!(
            repository.get_by_slug("비공개-글").unwrap().unwrap().status,
            PostStatus::Draft
        );
    }

    #[test]
    fn tags_keep_order_and_commas_and_match_exactly() {
        let repository = Repository::in_memory().unwrap();
        let post = repository
            .create_post(&input(
                "태그",
                PostStatus::Published,
                &["Rust", "a,b", "rust", "c#"],
            ))
            .unwrap();
        assert_eq!(post.tags, vec!["rust", "a,b", "c#"]);
        repository
            .create_post(&input("다른 글", PostStatus::Published, &["rustacean"]))
            .unwrap();
        let tagged = repository.list_by_tag("RUST", 1, 10).unwrap();
        assert_eq!(tagged.items.len(), 1);
        assert_eq!(tagged.items[0].title, "태그");
        assert_eq!(repository.list_by_tag("a,b", 1, 10).unwrap().total, 1);
        assert_eq!(repository.list_by_tag("a", 1, 10).unwrap().total, 0);
    }

    #[test]
    fn search_treats_like_wildcards_literally() {
        let repository = Repository::in_memory().unwrap();
        repository
            .create_post(&input("100% 완료", PostStatus::Published, &[]))
            .unwrap();
        repository
            .create_post(&input("snake_case", PostStatus::Published, &[]))
            .unwrap();
        repository
            .create_post(&input("평범한 글", PostStatus::Published, &[]))
            .unwrap();
        assert_eq!(repository.search_published("%", 1, 10).unwrap().total, 1);
        assert_eq!(repository.search_published("_", 1, 10).unwrap().total, 1);
        assert_eq!(repository.search_published("  ", 1, 10).unwrap().total, 0);
        assert_eq!(repository.search_published("평범", 1, 10).unwrap().total, 1);
    }

    #[test]
    fn publish_date_is_set_once() {
        let repository = Repository::in_memory().unwrap();
        let draft = repository
            .create_post(&input("초안", PostStatus::Draft, &[]))
            .unwrap();
        assert!(draft.published_at.is_none());
        let published = repository
            .update_post(draft.id, &input("초안", PostStatus::Published, &[]))
            .unwrap();
        let published_at = published
            .published_at
            .clone()
            .expect("published_at set on publish");
        repository
            .connection_for_tests()
            .execute(
                "UPDATE posts SET published_at = '2000-01-01 00:00:00' WHERE id = ?1",
                [draft.id],
            )
            .unwrap();
        let edited = repository
            .update_post(draft.id, &input("수정", PostStatus::Published, &[]))
            .unwrap();
        assert_eq!(edited.published_at.as_deref(), Some("2000-01-01 00:00:00"));
        assert_ne!(published_at, "");
    }

    #[test]
    fn deleting_a_post_removes_unused_tags() {
        let repository = Repository::in_memory().unwrap();
        let post = repository
            .create_post(&input("글", PostStatus::Published, &["혼자"]))
            .unwrap();
        repository.delete_post(post.id).unwrap();
        let tags: i64 = repository
            .connection_for_tests()
            .query_row("SELECT COUNT(*) FROM tags", [], |row| row.get(0))
            .unwrap();
        assert_eq!(tags, 0);
        assert!(matches!(
            repository.delete_post(post.id),
            Err(AppError::NotFound)
        ));
    }

    #[test]
    fn duplicate_slug_is_a_conflict() {
        let repository = Repository::in_memory().unwrap();
        repository
            .create_post(&input("같은 제목", PostStatus::Draft, &[]))
            .unwrap();
        let error = repository
            .create_post(&input("같은 제목", PostStatus::Draft, &[]))
            .unwrap_err();
        assert!(matches!(error, AppError::Conflict(_)));
    }

    #[test]
    fn pages_published_posts() {
        let repository = Repository::in_memory().unwrap();
        for index in 0..5 {
            repository
                .create_post(&input(&format!("글 {index}"), PostStatus::Published, &[]))
                .unwrap();
        }
        let first = repository.list_published(1, 2).unwrap();
        assert_eq!(
            (first.items.len(), first.total, first.total_pages()),
            (2, 5, 3)
        );
        assert_eq!(first.items[0].title, "글 4");
        let last = repository.list_published(3, 2).unwrap();
        assert_eq!(last.items.len(), 1);
        assert_eq!(last.items[0].title, "글 0");
    }

    #[test]
    fn file_database_survives_reopen() {
        let path = temp_db_path("reopen");
        {
            let repository = Repository::open(&path).unwrap();
            repository
                .create_post(&input("남는 글", PostStatus::Published, &["기록"]))
                .unwrap();
        }
        let repository = Repository::open(&path).unwrap();
        let post = repository
            .get_published_by_slug("남는-글")
            .unwrap()
            .unwrap();
        assert_eq!(post.tags, vec!["기록"]);
        assert_eq!(repository.schema_version().unwrap(), schema_version());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn migrates_database_from_first_release() {
        let path = temp_db_path("legacy");
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE posts (
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
                    CREATE INDEX idx_posts_status_created ON posts(status, created_at DESC);
                    CREATE INDEX idx_posts_slug ON posts(slug);
                    INSERT INTO posts (slug, title, body_markdown, tags, status, published_at)
                    VALUES ('old', '예전 글', '<b>굵게</b> **강조**', 'rust,axum', 'published', NULL);",
                )
                .unwrap();
        }
        let repository = Repository::open(&path).unwrap();
        let post = repository.get_published_by_slug("old").unwrap().unwrap();
        assert_eq!(post.tags, vec!["rust", "axum"]);
        assert!(post.published_at.is_some());
        assert!(post.body_html.contains("<strong>강조</strong>"));
        assert!(!post.body_html.contains("<b>"));
        assert_eq!(repository.list_by_tag("axum", 1, 10).unwrap().total, 1);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    impl Repository {
        fn connection_for_tests(&self) -> MutexGuard<'_, Connection> {
            self.writer()
        }
    }
}

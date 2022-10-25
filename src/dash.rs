use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use sqlx::sqlite::{SqliteConnectOptions, SqliteRow};
use tokio::fs::{read_dir};
use tokio::task::spawn_blocking;

use crate::provider::{DocProvider, DocSet, SearchEntry};

const EXTRA_KEYWORDS: &[(&'static str, &'static str)] = &[
    ("Android", "droid"),
];

pub struct Dash {
    doc_sets: Vec<DashDocSet>,
}

impl Dash {
    pub async fn new_with_default() -> anyhow::Result<Self> {
        let root = dirs::data_dir()
            .context("Unable to find data dir")?
            .join("Zeal")
            .join("Zeal")
            .join("docsets");

        let mut entries = read_dir(&root).await.context("Listing docset folder")?;
        let mut doc_sets = vec![];
        while let Some(entry) = entries.next_entry().await? {
            let set = match DashDocSet::new(entry.path()).await {
                Ok(v) => v,
                Err(e) => {
                    log::error!("Ignoring docset folder {}: {e:?}", entry.path().display());
                    continue;
                }
            };
            doc_sets.push(set);
        }
        log::debug!("Parsed doc sets: {doc_sets:#?}");
        Ok(Self { doc_sets })
    }
}

#[derive(Debug)]
struct DashDocSet {
    name: Arc<str>,
    title: Arc<str>,
    db: SqlitePool,
    icon: Option<Arc<str>>,
    keywords: Vec<Arc<str>>,
    _resource_root: PathBuf,
}

impl DashDocSet {
    async fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let meta_path = path.as_ref().join("meta.json");
        let meta = spawn_blocking(move || -> anyhow::Result<Value> {
            Ok(serde_json::from_reader(std::fs::File::open(meta_path).context("Opening meta.json")?)?)
        }).await??;

        let res_dir = path.as_ref().join("Contents").join("Resources");
        let db = SqlitePool::connect_with(SqliteConnectOptions::default().filename(res_dir.join("docSet.dsidx")).read_only(true)).await.context("Opening database")?;

        let name: Arc<str> = meta.get("name").context("Reading name")?.as_str().context("name is not string")?.into();
        let title = meta.get("name").context("Reading title")?.as_str().context("title is not string")?.into();
        let keywords = meta.get("extra").and_then(|extra| extra.get("keywords")).and_then(|keywords| keywords.as_array())
            .iter()
            .flat_map(|v| v.iter())
            .filter_map(|v| v.as_str())
            .map(|s| s.to_ascii_lowercase().into())
            .chain(vec![name.to_ascii_lowercase().into()].into_iter())
            .chain(EXTRA_KEYWORDS.iter().filter(|item| item.0.eq(name.as_ref())).map(|item| item.1.into()))
            .collect();

        Ok(Self {
            name,
            db,
            title,
            keywords,
            icon: Some(path.as_ref().join("icon@2x.png"))
                .iter()
                .filter(|p| p.is_file())
                .filter_map(|p| p.to_str())
                .map(|s| s.into())
                .next(),
            _resource_root: res_dir.join("Documents"),
        })
    }

    fn contains_keyword(&self, kw_lc: &str) -> bool {
        self.keywords.iter().find(|k| k.starts_with(kw_lc)).is_some()
    }

    fn to_doc_set(&self) -> DocSet {
        DocSet {
            id: self.name.clone(),
            keywords: self.keywords.iter().map(|v| v.clone()).collect(),
            name: self.name.clone(),
            description: self.title.clone(),
            icon: self.icon.clone().unwrap_or_else(|| Arc::from("")),
        }
    }
}

#[async_trait]
impl DocProvider for Dash {
    fn name(&self) -> &str {
        "Dash"
    }

    async fn search_doc_sets(&self, keyword: &str) -> anyhow::Result<Vec<DocSet>> {
        let keyword = keyword.to_ascii_lowercase();

        let rs = self.doc_sets
            .iter()
            .filter(|set| set.contains_keyword(&keyword))
            .map(DashDocSet::to_doc_set)
            .collect();
        log::debug!("DocSet search result for q = {keyword}: {rs:?}");
        Ok(rs)
    }

    async fn search(&self, doc_set_id: &str, q: &str) -> anyhow::Result<Vec<SearchEntry>> {
        let doc_set = match self.doc_sets.iter().find(|ds| ds.name.as_ref().eq(doc_set_id)) {
            Some(v) => v,
            None => return Ok(vec![]),
        };

        let entries: Vec<SqliteRow> = sqlx::query(r"
            WITH cte AS (
                SELECT
                    *,
                    CASE
                        WHEN name = trim(?1) THEN 100
                        WHEN name = trim(?1) COLLATE NOCASE THEN 90
                        WHEN name LIKE trim(?1) || '%' THEN 80
                        WHEN name LIKE '%' || trim(?1) THEN 70
                        WHEN name COLLATE NOCASE LIKE trim(?1) || '%' THEN 60
                        WHEN name COLLATE NOCASE LIKE '%' || trim(?1) THEN 50
                        ELSE 0
                    END as relevance
                FROM searchIndex
            )
            SELECT * FROM cte WHERE relevance > 0 ORDER by relevance DESC LIMIT 30
        ")
            .bind(q)
            .fetch_all(&doc_set.db).await.context("Running search SQL")?;
        log::debug!("Searching for {q} got {} results", entries.len());
        Ok(entries.into_iter().map(|row| SearchEntry {
            entry_type: row.get::<&str, _>("type").parse().unwrap(),
            title: row.get::<String, _>("name").into(),
            desc: Arc::from(""),
            url: format!("{}#{}", row.get::<&str, _>("path"), row.try_get::<&str, _>("fragment").unwrap_or("")).into(),
            relevance: row.get::<i64, _>("relevance") as usize,
        }).collect())
    }

    async fn clean_up(&self) {}
}

use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{File, FileType};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::Context;
use async_trait::async_trait;
use serde::Deserialize;
use sqlx::{FromRow, SqlitePool, Type};
use tokio::fs::read_dir;

use crate::provider::{DocProvider, DocSet, EntryType, SearchEntry};

pub struct Dash {
    doc_set_path: PathBuf,
    pools: RwLock<HashMap<PathBuf, Arc<SqlitePool>>>,
}

impl Dash {
    pub fn new_with_default() -> anyhow::Result<Self> {
        Ok(Self {
            doc_set_path: dirs::data_dir()
                .context("Unable to find data dir")?
                .join("Zeal")
                .join("Zeal")
                .join("docsets"),
            pools: Default::default(),
        })
    }
}

const KEYWORD_TO_NAME_MAPPINGS: &[(&'static str, &'static str)] = &[
    ("droid", "Android"),
    ("android", "Android"),
    ("py", "Python_3"),
];

fn find_names_by_keyword(kw: &str) -> impl Iterator<Item = &str> {
    let kw_lc = kw.to_ascii_lowercase();
    KEYWORD_TO_NAME_MAPPINGS
        .iter()
        .filter(move |item| item.0.starts_with(&kw_lc))
        .map(|v| v.1)
}

fn find_keywords_by_name<'a>(name: &str) -> impl Iterator<Item = &str> {
    KEYWORD_TO_NAME_MAPPINGS
        .iter()
        .filter(|item| item.1.eq_ignore_ascii_case(name))
        .map(|v| v.0)
}

#[derive(Deserialize, Default)]
struct DocSetExtras {
    keywords: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct DocSetMeta {
    #[serde(default)]
    extra: DocSetExtras,
    name: String,
    title: String,
    version: String,
}

#[derive(FromRow, Debug)]
struct IndexEntry {
    name: String,
    #[sqlx(rename="type")]
    t: String,
    path: String,
    fragment: String,
    relevance: i64,
}

#[async_trait]
impl DocProvider for Dash {
    fn name(&self) -> &str {
        "Dash"
    }

    async fn search_doc_sets(&self, keyword: &str) -> anyhow::Result<Vec<DocSet>> {
        let mut entries = read_dir(&self.doc_set_path).await?;
        let mut rs = vec![];
        while let Some(entry) = entries.next_entry().await? {
            if !entry.file_type().await?.is_dir() {
                continue;
            }

            // Try to parse meta
            let meta_file = entry.path().join("meta.json");
            let db_file = entry.path().join("Contents").join("Resources").join("docSet.dsidx");
            if meta_file.is_file() && db_file.is_file() {
                log::debug!("Opening {}", meta_file.display());
                let file = File::open(meta_file).context("Opening meta file")?;
                let DocSetMeta { name, title, extra, .. }: DocSetMeta = serde_json::from_reader(file).context("Parsing meta file")?;
                let id = db_file.to_str().context("Converting database path")?.to_string();
                if find_names_by_keyword(keyword).filter(|n| n.eq_ignore_ascii_case(&name)).next().is_some() {
                    let icon = entry.path().join("icon@2x.png").to_str().context("Converting icon path")?.to_string();
                    let docset_specified_keywords = extra.keywords.unwrap_or_default();
                    rs.extend(docset_specified_keywords.iter().map(|v| v.as_ref()).chain(find_keywords_by_name(&name)).map(|kw| DocSet{
                        id: id.clone().into(),
                        keyword: kw.to_string().into(),
                        name: title.clone().into(),
                        description: title.clone().into(),
                        icon: icon.clone().into(),
                    }));
                }
            } else {
                log::warn!("{entry:?} is not a DocSet folder");
                continue;
            }
        }
        Ok(rs)
    }



    async fn search(&self, doc_set_id: &str, q: &str) -> anyhow::Result<Vec<SearchEntry>> {
        let  conn = SqlitePool::connect(&format!("sqlite:{doc_set_id}")).await.with_context(|| format!("Opening sqlite db: {doc_set_id}"))?;
        let entries: Vec<IndexEntry> = sqlx::query_as(r"
            WITH cte AS (
                SELECT *, 100 as relevance FROM searchIndex WHERE name = trim(?1) COLLATE NOCASE
                UNION
                SELECT *, 70 as relevance FROM searchIndex WHERE name LIKE trim(?1) || '%' COLLATE NOCASE
                UNION
                SELECT *, 50 as relevance FROM searchIndex WHERE name LIKE trim(?1) || '%' COLLATE NOCASE
            )
            SELECT DISTINCT(name) AS name, type, path, fragment, relevance FROM cte ORDER by relevance DESC LIMIT 10
        ")
            .bind(q)
            .fetch_all(&conn).await.context("Running search SQL")?;
        log::debug!("Searching for {q} got {} results", entries.len());
        Ok(entries.into_iter().map(|IndexEntry { name, t, path, fragment, relevance }| SearchEntry {
            entry_type: EntryType::Class,
            title: name.into(),
            desc: Cow::Borrowed(""),
            url: format!("{path}#{fragment}").into(),
            relevance: relevance as usize,
        }).collect())
    }

    async fn clean_up(&self) {
        todo!()
    }
}
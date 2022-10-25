use std::sync::Arc;
use async_trait::async_trait;
use serde_enum_str::Deserialize_enum_str;

#[derive(Debug, PartialEq, Eq, Deserialize_enum_str, Clone)]
pub enum EntryType {
    Class,
    Function,
    Method,
    Enum,
    Constant,
    Option,
    Guide,
    Module,
    #[serde(other)]
    Other(Arc<str>),
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct SearchEntry {
    pub entry_type: EntryType,
    pub title: Arc<str>,
    pub desc: Arc<str>,
    pub url: Arc<str>,
    pub relevance: usize,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct DocSet {
    pub id: Arc<str>,
    pub keywords: Vec<Arc<str>>,
    pub name: Arc<str>,
    pub description: Arc<str>,
    pub icon: Arc<str>,
}

#[async_trait]
pub trait DocProvider {
    fn name(&self) -> &str;
    async fn search_doc_sets(&self, keyword: &str) -> anyhow::Result<Vec<DocSet>>;
    async fn search(&self, doc_set_id: &str, q: &str) -> anyhow::Result<Vec<SearchEntry>>;
    async fn clean_up(&self);
}

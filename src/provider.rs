use std::borrow::Cow;
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug, PartialEq, Eq, Deserialize)]
pub enum EntryType {
    Class,
    Function,
    Method,
    Enum,
    Constant,
    Option,
    Guide,
    Module,
    Other(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct SearchEntry {
    pub entry_type: EntryType,
    pub title: Cow<'static, str>,
    pub desc: Cow<'static, str>,
    pub url: Cow<'static, str>,
    pub relevance: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DocSet {
    pub id: Cow<'static, str>,
    pub keyword: Cow<'static, str>,
    pub name: Cow<'static, str>,
    pub description: Cow<'static, str>,
    pub icon: Cow<'static, str>,
}

#[async_trait]
pub trait DocProvider {
    fn name(&self) -> &str;
    async fn search_doc_sets(&self, keyword: &str) -> anyhow::Result<Vec<DocSet>>;
    async fn search(&self, doc_set_id: &str, q: &str) -> anyhow::Result<Vec<SearchEntry>>;
    async fn clean_up(&self);
}

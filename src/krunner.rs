use std::{collections::HashMap, sync::Arc, vec};
use anyhow::Context;

use maplit::hashmap;
use serde::{Deserialize, Serialize};
use tokio::task::{JoinError, JoinSet};
use zbus::{
    dbus_interface,
    fdo::Result,
    zvariant::{Type, Value},
    ConnectionBuilder,
};
use zbus::fdo::Error;

use crate::provider::{DocProvider, DocSet, EntryType, SearchEntry};

pub struct KRunnerPlugin {
    providers: Vec<Arc<dyn DocProvider + Send + Sync + 'static>>,
}

impl KRunnerPlugin {
    pub async fn new(
        providers: Vec<Arc<dyn DocProvider + Send + Sync + 'static>>,
        object_path: &str,
    ) -> anyhow::Result<()> {
        ConnectionBuilder::session()?
            .name("dev.fanchao.DashDoc")?
            .serve_at(object_path, Self { providers })?
            .build()
            .await?;

        Ok(())
    }
}

type MatchType = i32;

const MATCH_TYPE_COMPLETION: MatchType = 10;
const MATCH_TYPE_EXACT: MatchType = 100;


#[derive(Serialize, Debug, Type, Clone, Eq, PartialEq, Hash)]
#[zvariant(signature = "s")]
#[serde(rename_all = "lowercase")]
enum QueryPropertyField {
    Category,
    // Urls,
    Subtext,
}

#[derive(Serialize, Type)]
struct QueryEntry {
    data: Arc<str>,
    display_text: Arc<str>,
    icon_name: Arc<str>,
    match_type: MatchType,
    relevance: f64,
    properties: HashMap<QueryPropertyField, Value<'static>>,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq, Clone)]
enum EntryData {
    DocSet {
        provider: Arc<str>,
        id: Arc<str>,
    },
    Entry {
        provider: Arc<str>,
        doc_set_id: Arc<str>,
        url: Arc<str>,
    },
}

type VariantMap<'a> = HashMap<&'a str, Value<'static>>;

#[dbus_interface(name = "org.kde.krunner1")]
impl KRunnerPlugin {
    #[dbus_interface(name = "Match")]
    async fn query(&self, query: &str) -> Result<Vec<QueryEntry>> {
        log::debug!("Querying {query}");

        let mut splits = query.trim().split_ascii_whitespace();
        let (kw, query) = match (splits.next(), splits.next()) {
            (Some(v), q) => (v, q.unwrap_or_default()),
            _ => return Ok(vec![]),
        };

        if kw.len() < 1 {
            return Ok(vec![]);
        }

        let kw: Arc<str> = kw.into();
        let query: Arc<str> = query.into();

        // Search concurrently in all providers
        let mut task_set = JoinSet::new();
        for p in &self.providers {
            let kw = kw.clone();
            let p = p.clone();
            let query = query.clone();
            task_set.spawn(async move {
                let doc_sets = match p.search_doc_sets(kw.as_ref()).await {
                    Ok(doc_sets) if !doc_sets.is_empty() => doc_sets,
                    Ok(_) => return vec![],
                    Err(e) => {
                        log::error!("Error searching doc provider(name={}): {e:?}", p.name());
                        return vec![];
                    }
                };

                if query.is_empty() {
                    return doc_sets
                        .into_iter()
                        .flat_map(
                            |DocSet {
                                 id,
                                 name,
                                 keywords,
                                 icon,
                                 ..
                             }| {
                                let provider_name: Arc<str> = p.name().into();
                                keywords.into_iter().map(move |keyword| QueryEntry {
                                    data: serde_json::to_string(&EntryData::DocSet {
                                        provider: provider_name.clone(),
                                        id: id.clone(),
                                    }).unwrap().into(),
                                    display_text: format!("Type \"{keyword} keyword\" to search {name}").into(),
                                    icon_name: icon.clone(),
                                    match_type: MATCH_TYPE_COMPLETION,
                                    relevance: 1.0,
                                    properties: Default::default(),
                                })
                            },
                        )
                        .collect();
                }

                match search_in_doc_sets(p.clone(), doc_sets, query).await {
                    Ok(v) => v,
                    Err(e) => {
                        log::error!("Error searching in doc {}: {e:?}", p.name());
                        vec![]
                    }
                }
            });
        }

        collect_join_set(task_set, |rs, buf| {
            buf.extend(rs?);
            Ok(())
        }).await.map_err(|e| Error::Failed(e.to_string()))
    }

    async fn config(&self) -> VariantMap {
        log::debug!("Get config");
        hashmap! {
            "MinLetterCount" => "1".into(),
        }
    }

    async fn run(&self, data: &str, action_id: &str) -> Result<()> {
        let data: EntryData = serde_json::from_str(data)
            .context("Parsing entry data")
            .map_err(|e| Error::Failed(format!("{e:?}")))?;
        log::debug!("Run {data:?} with {action_id}");

        if let EntryData::Entry { provider, doc_set_id, url } = data {
            if let Some(provider) = self.providers.iter().find(|p| p.name() == provider.as_ref()) {
                provider.open(doc_set_id.as_ref(), url.as_ref()).await
                    .map_err(|e| Error::Failed(format!("{e:?}")))?;
            }
        }

        Ok(())
    }

    async fn teardown(&self) {
        log::debug!("Tear down");
    }
}

impl EntryType {
    fn get_krunner_icon(&self) -> Arc<str> {
        match self {
            EntryType::Class => Arc::from("class-or-package"),
            EntryType::Method | EntryType::Function => Arc::from("code-function"),
            EntryType::Enum => Arc::from("enum"),
            EntryType::Constant => Arc::from("code-variable"),
            _ => Arc::from("")
        }
    }
}

async fn search_in_doc_sets(
    doc_provider: Arc<dyn DocProvider + Send + Sync + 'static>,
    doc_sets: Vec<DocSet>,
    q: Arc<str>,
) -> anyhow::Result<Vec<QueryEntry>> {
    log::debug!("Search {q} in doc sets: {doc_sets:?}");
    let mut join_set = JoinSet::new();
    for ds in doc_sets {
        let doc_provider = doc_provider.clone();
        let q = q.clone();
        join_set.spawn(async move {
            doc_provider.search(&ds.id, q.as_ref()).await
                .map(move |entries| entries.into_iter().map(move |SearchEntry { entry_type, title, desc, id: url, relevance }| QueryEntry {
                    data: serde_json::to_string(&EntryData::Entry { provider: doc_provider.name().into(), doc_set_id: ds.id.clone(), url }).unwrap().into(),
                    display_text: title,
                    icon_name: entry_type.get_krunner_icon(),
                    match_type: MATCH_TYPE_EXACT,
                    relevance: (relevance as f64) / 100.0,
                    properties: hashmap! {
                        QueryPropertyField::Category => ds.name.to_string().into(),
                        QueryPropertyField::Subtext => desc.to_string().into(),
                        // QueryPropertyField::Urls => vec![url.to_string()].into(),
                    },
                }))
        });
    }

    collect_join_set(join_set, |r, buf| {
        buf.extend(r??);
        Ok(())
    }).await
}

async fn collect_join_set<S: 'static, T>(mut js: JoinSet<S>, map: impl Fn(std::result::Result<S, JoinError>, &mut Vec<T>) -> anyhow::Result<()>) -> anyhow::Result<Vec<T>> {
    let mut rs = vec![];
    while let Some(s) = js.join_next().await {
        map(s, &mut rs)?;
    }
    Ok(rs)
}

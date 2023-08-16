use std::mem;

use anyhow::Result;
use turbo_tasks::{ReadRef, TryJoinIterExt, Value, Vc};
use turbopack_core::introspect::{Introspectable, IntrospectableChildren};

use super::{
    specificity::Specificity, ContentSource, ContentSourceData, ContentSourceResult, NeededData,
};
use crate::source::ContentSources;

/// Combines multiple [ContentSource]s by trying all content sources in order.
/// The content source which responds with the most specific response (that is
/// not a [ContentSourceContent::NotFound]) will be returned.
#[turbo_tasks::value(shared)]
pub struct CombinedContentSource {
    pub sources: Vec<Vc<Box<dyn ContentSource>>>,
}

/// A helper source which allows the [CombinedContentSource] to be paused while
/// we ask for vary data.
#[turbo_tasks::value(shared)]
pub struct PausableCombinedContentSource {
    /// The index of the item which requested vary data. When running [get], we
    /// will skip to exactly this item to resume iteration.
    index: usize,

    /// The paused state (partially processed path, content source, vary data)
    /// of the internal content source which asked for vary data.
    pending: Option<PendingState>,

    /// A [CombinedContentSource] which we are querying for content.
    inner: Vc<CombinedContentSource>,

    /// The current most-specific content result.
    max: Option<(ReadRef<Specificity>, Vc<ContentSourceResult>)>,
}

/// Stores partially computed data that an inner [ContentSource] returned when
/// it requested more data.
#[derive(Clone)]
#[turbo_tasks::value(shared)]
struct PendingState {
    /// A partially computed path. Note that this may be any value and not
    /// necessarily equal to the path we receive from the dev server.
    path: String,

    /// A partially computed content source to receive the requested data. Note
    /// that this is not necessarily the same content source value that
    /// exists inside the [CombinedContentSource]'s sources vector.
    source: Vc<Box<dyn ContentSource>>,
}

impl CombinedContentSource {
    pub fn new(sources: Vec<Vc<Box<dyn ContentSource>>>) -> Vc<Self> {
        CombinedContentSource { sources }.cell()
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for CombinedContentSource {
    #[turbo_tasks::function]
    async fn get(
        self: Vc<Self>,
        path: String,
        data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        let pauseable = PausableCombinedContentSource::new(self);
        pauseable.pauseable_get(path.as_str(), data).await
    }

    #[turbo_tasks::function]
    fn get_children(&self) -> Vc<ContentSources> {
        Vc::cell(self.sources.clone())
    }
}

impl PausableCombinedContentSource {
    fn new(inner: Vc<CombinedContentSource>) -> Self {
        PausableCombinedContentSource {
            inner,
            index: 0,
            pending: None,
            max: None,
        }
    }

    /// Queries each content source in turn, returning a new pauseable instance
    /// if any source requests additional vary data.
    async fn pauseable_get(
        &self,
        path: &str,
        mut data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        let inner = self.inner;
        let mut max = self.max.clone();
        let mut pending = self.pending.clone();

        for (index, source) in inner.await?.sources.iter().enumerate().skip(self.index) {
            // If there is pending state, then this is the first iteration of the resume and
            // we've skipped to exactly the source which requested data. Requery the source
            // with it's partially computed path and needed data.
            let result = match pending.take() {
                Some(pending) => pending
                    .source
                    .resolve()
                    .await?
                    .get(pending.path, mem::take(&mut data)),
                None => source
                    .resolve()
                    .await?
                    .get(path.to_string(), Default::default()),
            };

            let res = result.await?;
            match &*res {
                ContentSourceResult::NeedData(data) => {
                    // We create a partially computed content source which will be able to resume
                    // iteration at this exact content source after getting data.
                    let paused = PausableCombinedContentSource {
                        inner,
                        index,
                        pending: Some(PendingState::from(data)),
                        max,
                    };

                    return Ok(ContentSourceResult::need_data(Value::new(NeededData {
                        // We do not return data.path because that would affect later content source
                        // requests. However, when we resume, we'll use the path stored in pending
                        // to correctly requery this source.
                        path: path.to_string(),
                        source: Vc::upcast(paused.cell()),
                        vary: data.vary.clone(),
                    })));
                }
                ContentSourceResult::NotFound => {
                    // we can keep the current max
                }
                ContentSourceResult::Result { specificity, .. } => {
                    let specificity = specificity.await?;
                    if specificity.is_exact() {
                        return Ok(result);
                    }
                    if let Some((max, _)) = max.as_ref() {
                        if *max >= specificity {
                            // we can keep the current max
                            continue;
                        }
                    }
                    max = Some((specificity, result));
                }
            }
        }

        if let Some((_, result)) = max {
            Ok(result)
        } else {
            Ok(ContentSourceResult::not_found())
        }
    }
}

impl From<&NeededData> for PendingState {
    fn from(value: &NeededData) -> Self {
        PendingState {
            path: value.path.clone(),
            source: value.source,
        }
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for PausableCombinedContentSource {
    #[turbo_tasks::function]
    async fn get(
        &self,
        path: String,
        data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        self.pauseable_get(&path, data).await
    }
}

#[turbo_tasks::value_impl]
impl Introspectable for CombinedContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        Vc::cell("combined content source".to_string())
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<Vc<String>> {
        let titles = self
            .sources
            .iter()
            .map(|&source| async move {
                Ok(
                    if let Some(source) =
                        Vc::try_resolve_sidecast::<Box<dyn Introspectable>>(source).await?
                    {
                        Some(source.title().await?)
                    } else {
                        None
                    },
                )
            })
            .try_join()
            .await?;
        let mut titles = titles.into_iter().flatten().collect::<Vec<_>>();
        titles.sort();
        const NUMBER_OF_TITLES_TO_DISPLAY: usize = 5;
        let mut titles = titles
            .iter()
            .map(|t| t.as_str())
            .filter(|t| !t.is_empty())
            .take(NUMBER_OF_TITLES_TO_DISPLAY + 1)
            .collect::<Vec<_>>();
        if titles.len() > NUMBER_OF_TITLES_TO_DISPLAY {
            titles[NUMBER_OF_TITLES_TO_DISPLAY] = "...";
        }
        Ok(Vc::cell(titles.join(", ")))
    }

    #[turbo_tasks::function]
    async fn children(&self) -> Result<Vc<IntrospectableChildren>> {
        let source = Vc::cell("source".to_string());
        Ok(Vc::cell(
            self.sources
                .iter()
                .copied()
                .map(|s| async move {
                    Ok(Vc::try_resolve_sidecast::<Box<dyn Introspectable>>(s).await?)
                })
                .try_join()
                .await?
                .into_iter()
                .flatten()
                .map(|i| (source, i))
                .collect(),
        ))
    }
}

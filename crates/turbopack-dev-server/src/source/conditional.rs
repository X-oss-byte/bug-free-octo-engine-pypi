use anyhow::Result;
use turbo_tasks::{ReadRef, State, Value, Vc};
use turbopack_core::introspect::{Introspectable, IntrospectableChildren};

use super::{
    combined::CombinedContentSource, ContentSource, ContentSourceData, ContentSourceDataVary,
    ContentSourceResult, GetContentSourceContent,
};
use crate::source::{ContentSourceContent, ContentSources};

/// Combines two [ContentSource]s like the [CombinedContentSource], but only
/// allows to serve from the second source when the first source has
/// successfully served something once.
/// This is a laziness optimization when the content of the second source can
/// only be reached via references from the first source.
///
/// For example, we use that in the content source that handles SSR rendering of
/// pages. Here HTML and "other assets" are in different content sources. So we
/// use this source to only serve (and process) "other assets" when the HTML was
/// served once.
#[turbo_tasks::value(serialization = "none", eq = "manual", cell = "new")]
pub struct ConditionalContentSource {
    activator: Vc<Box<dyn ContentSource>>,
    action: Vc<Box<dyn ContentSource>>,
    activated: State<bool>,
}

#[turbo_tasks::value_impl]
impl ConditionalContentSource {
    #[turbo_tasks::function]
    pub fn new(
        activator: Vc<Box<dyn ContentSource>>,
        action: Vc<Box<dyn ContentSource>>,
    ) -> Vc<Self> {
        ConditionalContentSource {
            activator,
            action,
            activated: State::new(false),
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for ConditionalContentSource {
    #[turbo_tasks::function]
    async fn get(
        self: Vc<Self>,
        path: String,
        data: turbo_tasks::Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        let this = self.await?;
        if !*this.activated.get() {
            let first = this.activator.get(path, data.clone());
            let first_value = first.await?;
            return Ok(match &*first_value {
                &ContentSourceResult::Result {
                    get_content,
                    specificity,
                } => ContentSourceResult::Result {
                    get_content: Vc::upcast(
                        ActivateOnGetContentSource {
                            source: this,
                            get_content,
                        }
                        .cell(),
                    ),
                    specificity,
                }
                .cell(),
                _ => first,
            });
        }
        Ok(CombinedContentSource {
            sources: vec![this.activator, this.action],
        }
        .cell()
        .get(path, data))
    }

    #[turbo_tasks::function]
    fn get_children(&self) -> Vc<ContentSources> {
        Vc::cell(vec![self.activator, self.action])
    }
}

#[turbo_tasks::function]
fn introspectable_type() -> Vc<String> {
    Vc::cell("conditional content source".to_string())
}

#[turbo_tasks::function]
fn activator_key() -> Vc<String> {
    Vc::cell("activator".to_string())
}

#[turbo_tasks::function]
fn action_key() -> Vc<String> {
    Vc::cell("action".to_string())
}

#[turbo_tasks::value_impl]
impl Introspectable for ConditionalContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        introspectable_type()
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<Vc<String>> {
        if let Some(activator) =
            Vc::try_resolve_sidecast::<Box<dyn Introspectable>>(self.activator).await?
        {
            Ok(activator.title())
        } else {
            Ok(Vc::<String>::empty())
        }
    }

    #[turbo_tasks::function]
    async fn children(&self) -> Result<Vc<IntrospectableChildren>> {
        Ok(Vc::cell(
            [
                Vc::try_resolve_sidecast::<Box<dyn Introspectable>>(self.activator)
                    .await?
                    .map(|i| (activator_key(), i)),
                Vc::try_resolve_sidecast::<Box<dyn Introspectable>>(self.action)
                    .await?
                    .map(|i| (action_key(), i)),
            ]
            .into_iter()
            .flatten()
            .collect(),
        ))
    }
}

#[turbo_tasks::value(serialization = "none", eq = "manual", cell = "new")]
struct ActivateOnGetContentSource {
    source: ReadRef<ConditionalContentSource>,
    get_content: Vc<Box<dyn GetContentSourceContent>>,
}

#[turbo_tasks::value_impl]
impl GetContentSourceContent for ActivateOnGetContentSource {
    #[turbo_tasks::function]
    fn vary(&self) -> Vc<ContentSourceDataVary> {
        self.get_content.vary()
    }

    #[turbo_tasks::function]
    fn get(&self, data: Value<ContentSourceData>) -> Vc<ContentSourceContent> {
        self.source.activated.set(true);
        self.get_content.get(data)
    }
}

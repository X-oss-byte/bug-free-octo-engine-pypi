use anyhow::Result;
use turbo_tasks::{TryJoinIterExt, Value, Vc};
use turbopack_core::introspect::{Introspectable, IntrospectableChildren};

use super::{ContentSource, ContentSourceData, ContentSourceResult};
use crate::source::ContentSources;

/// Binds different ContentSources to different subpaths. A fallback
/// ContentSource will serve all other subpaths.
// TODO(WEB-1151): Remove this and migrate all users to PrefixedRouterContentSource.
#[turbo_tasks::value(shared)]
pub struct RouterContentSource {
    pub routes: Vec<(String, Vc<Box<dyn ContentSource>>)>,
    pub fallback: Vc<Box<dyn ContentSource>>,
}

/// Binds different ContentSources to different subpaths. The request path must
/// begin with the prefix, which will be stripped (along with the subpath)
/// before querying the ContentSource. A fallback ContentSource will serve all
/// other subpaths, including if the request path does not include the prefix.
#[turbo_tasks::value(shared)]
pub struct PrefixedRouterContentSource {
    prefix: Vc<String>,
    routes: Vec<(String, Vc<Box<dyn ContentSource>>)>,
    fallback: Vc<Box<dyn ContentSource>>,
}

#[turbo_tasks::value_impl]
impl PrefixedRouterContentSource {
    #[turbo_tasks::function]
    pub async fn new(
        prefix: Vc<String>,
        routes: Vec<(String, Vc<Box<dyn ContentSource>>)>,
        fallback: Vc<Box<dyn ContentSource>>,
    ) -> Result<Vc<Self>> {
        if cfg!(debug_assertions) {
            let prefix_string = prefix.await?;
            debug_assert!(prefix_string.is_empty() || prefix_string.ends_with('/'));
            debug_assert!(!prefix_string.starts_with('/'));
        }
        Ok(PrefixedRouterContentSource {
            prefix,
            routes,
            fallback,
        }
        .cell())
    }
}

/// If the `path` starts with `prefix`, then it will search each route to see if
/// any subpath matches. If so, the remaining path (after removing the prefix
/// and subpath) is used to query the matching ContentSource. If no match is
/// found, then the fallback is queried with the original path.
async fn get(
    routes: &[(String, Vc<Box<dyn ContentSource>>)],
    fallback: &Vc<Box<dyn ContentSource>>,
    prefix: &str,
    path: &str,
    data: Value<ContentSourceData>,
) -> Result<Vc<ContentSourceResult>> {
    let mut found = None;

    if let Some(path) = path.strip_prefix(prefix) {
        for (subpath, source) in routes {
            if let Some(path) = path.strip_prefix(subpath) {
                found = Some((source, path));
                break;
            }
        }
    }

    let (source, path) = found.unwrap_or((fallback, path));
    Ok(source.resolve().await?.get(path.to_string(), data))
}

fn get_children(
    routes: &[(String, Vc<Box<dyn ContentSource>>)],
    fallback: &Vc<Box<dyn ContentSource>>,
) -> Vc<ContentSources> {
    Vc::cell(
        routes
            .iter()
            .map(|r| r.1)
            .chain(std::iter::once(*fallback))
            .collect(),
    )
}

async fn get_introspection_children(
    routes: &[(String, Vc<Box<dyn ContentSource>>)],
    fallback: &Vc<Box<dyn ContentSource>>,
) -> Result<Vc<IntrospectableChildren>> {
    Ok(Vc::cell(
        routes
            .iter()
            .cloned()
            .chain(std::iter::once((String::new(), *fallback)))
            .map(|(path, source)| async move {
                Ok(Vc::try_resolve_sidecast::<Box<dyn Introspectable>>(source)
                    .await?
                    .map(|i| (Vc::cell(path), i)))
            })
            .try_join()
            .await?
            .into_iter()
            .flatten()
            .collect(),
    ))
}

#[turbo_tasks::value_impl]
impl ContentSource for RouterContentSource {
    #[turbo_tasks::function]
    async fn get(
        &self,
        path: String,
        data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        get(&self.routes, &self.fallback, "", &path, data).await
    }

    #[turbo_tasks::function]
    fn get_children(&self) -> Vc<ContentSources> {
        get_children(&self.routes, &self.fallback)
    }
}

#[turbo_tasks::value_impl]
impl Introspectable for RouterContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        Vc::cell("router content source".to_string())
    }

    #[turbo_tasks::function]
    async fn children(&self) -> Result<Vc<IntrospectableChildren>> {
        get_introspection_children(&self.routes, &self.fallback).await
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for PrefixedRouterContentSource {
    #[turbo_tasks::function]
    async fn get(
        &self,
        path: String,
        data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        let prefix = self.prefix.await?;
        get(&self.routes, &self.fallback, &prefix, &path, data).await
    }

    #[turbo_tasks::function]
    fn get_children(&self) -> Vc<ContentSources> {
        get_children(&self.routes, &self.fallback)
    }
}

#[turbo_tasks::value_impl]
impl Introspectable for PrefixedRouterContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        Vc::cell("prefixed router content source".to_string())
    }

    #[turbo_tasks::function]
    async fn details(&self) -> Result<Vc<String>> {
        let prefix = self.prefix.await?;
        Ok(Vc::cell(format!("prefix: '{}'", prefix)))
    }

    #[turbo_tasks::function]
    async fn children(&self) -> Result<Vc<IntrospectableChildren>> {
        get_introspection_children(&self.routes, &self.fallback).await
    }
}

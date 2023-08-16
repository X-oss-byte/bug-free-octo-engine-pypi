use anyhow::Result;
use turbo_tasks::{primitives::StringVc, Value, ValueToString, ValueToStringVc};
use turbopack_core::{
    chunk::{ChunkableAssetReference, ChunkableAssetReferenceVc},
    reference::{AssetReference, AssetReferenceVc},
    reference_type::CssReferenceSubType,
    resolve::{origin::ResolveOriginVc, parse::RequestVc, ResolveResultVc},
};

use crate::references::css_resolve;

/// A `composes: ... from ...` CSS module reference.
#[turbo_tasks::value]
#[derive(Hash, Debug)]
pub struct CssModuleComposeReference {
    pub origin: ResolveOriginVc,
    pub request: RequestVc,
}

#[turbo_tasks::value_impl]
impl CssModuleComposeReferenceVc {
    /// Creates a new [`CssModuleComposeReference`].
    #[turbo_tasks::function]
    pub fn new(origin: ResolveOriginVc, request: RequestVc) -> Self {
        Self::cell(CssModuleComposeReference { origin, request })
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for CssModuleComposeReference {
    #[turbo_tasks::function]
    fn resolve_reference(&self) -> ResolveResultVc {
        css_resolve(
            self.origin,
            self.request,
            Value::new(CssReferenceSubType::Compose),
        )
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for CssModuleComposeReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        Ok(StringVc::cell(format!(
            "compose(url) {}",
            self.request.to_string().await?,
        )))
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAssetReference for CssModuleComposeReference {}

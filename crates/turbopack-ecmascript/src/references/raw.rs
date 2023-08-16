use anyhow::Result;
use turbo_tasks::{ValueToString, Vc};
use turbopack_core::{
    asset::Asset,
    reference::AssetReference,
    resolve::{pattern::Pattern, resolve_raw, ResolveResult},
};

#[turbo_tasks::value]
#[derive(Hash, Debug)]
pub struct SourceAssetReference {
    pub source: Vc<Box<dyn Asset>>,
    pub path: Vc<Pattern>,
}

#[turbo_tasks::value_impl]
impl SourceAssetReference {
    #[turbo_tasks::function]
    pub fn new(source: Vc<Box<dyn Asset>>, path: Vc<Pattern>) -> Vc<Self> {
        Self::cell(SourceAssetReference { source, path })
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for SourceAssetReference {
    #[turbo_tasks::function]
    async fn resolve_reference(&self) -> Result<Vc<ResolveResult>> {
        let context = self.source.ident().path().parent();

        Ok(resolve_raw(context, self.path, false))
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for SourceAssetReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(format!(
            "raw asset {}",
            self.path.to_string().await?,
        )))
    }
}

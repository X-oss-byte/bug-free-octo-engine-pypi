use anyhow::Result;
use indexmap::IndexSet;
use turbo_tasks::{ValueToString, Vc};
use turbo_tasks_fs::FileContent;

use super::{Introspectable, IntrospectableChildren};
use crate::{
    asset::{Asset, AssetContent},
    chunk::{ChunkableAssetReference, ChunkingType},
    reference::{AssetReference, AssetReferences},
    resolve::PrimaryResolveResult,
};

#[turbo_tasks::value]
pub struct IntrospectableAsset(Vc<Box<dyn Asset>>);

#[turbo_tasks::value_impl]
impl IntrospectableAsset {
    #[turbo_tasks::function]
    pub async fn new(asset: Vc<Box<dyn Asset>>) -> Result<Vc<Box<dyn Introspectable>>> {
        Ok(Vc::try_resolve_sidecast::<Box<dyn Introspectable>>(asset)
            .await?
            .unwrap_or_else(|| Vc::upcast(IntrospectableAsset(asset).cell())))
    }
}

#[turbo_tasks::function]
fn asset_ty() -> Vc<String> {
    Vc::cell("asset".to_string())
}

#[turbo_tasks::function]
fn reference_ty() -> Vc<String> {
    Vc::cell("reference".to_string())
}

#[turbo_tasks::function]
fn placed_or_parallel_reference_ty() -> Vc<String> {
    Vc::cell("placed/parallel reference".to_string())
}

#[turbo_tasks::function]
fn placed_reference_ty() -> Vc<String> {
    Vc::cell("placed reference".to_string())
}

#[turbo_tasks::function]
fn parallel_reference_ty() -> Vc<String> {
    Vc::cell("parallel reference".to_string())
}

#[turbo_tasks::function]
fn isolated_parallel_reference_ty() -> Vc<String> {
    Vc::cell("isolated parallel reference".to_string())
}

#[turbo_tasks::function]
fn separate_reference_ty() -> Vc<String> {
    Vc::cell("separate reference".to_string())
}

#[turbo_tasks::function]
fn async_reference_ty() -> Vc<String> {
    Vc::cell("async reference".to_string())
}

#[turbo_tasks::value_impl]
impl Introspectable for IntrospectableAsset {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        asset_ty()
    }

    #[turbo_tasks::function]
    fn title(&self) -> Vc<String> {
        self.0.ident().to_string()
    }

    #[turbo_tasks::function]
    fn details(&self) -> Vc<String> {
        content_to_details(self.0.content())
    }

    #[turbo_tasks::function]
    fn children(&self) -> Vc<IntrospectableChildren> {
        children_from_asset_references(self.0.references())
    }
}

#[turbo_tasks::function]
pub async fn content_to_details(content: Vc<AssetContent>) -> Result<Vc<String>> {
    Ok(match &*content.await? {
        AssetContent::File(file_content) => match &*file_content.await? {
            FileContent::Content(file) => {
                let content = file.content();
                match content.to_str() {
                    Ok(str) => Vc::cell(str.into_owned()),
                    Err(_) => Vc::cell(format!("{} binary bytes", content.len())),
                }
            }
            FileContent::NotFound => Vc::cell("not found".to_string()),
        },
        AssetContent::Redirect { target, link_type } => {
            Vc::cell(format!("redirect to {target} with type {link_type:?}"))
        }
    })
}

#[turbo_tasks::function]
pub async fn children_from_asset_references(
    references: Vc<AssetReferences>,
) -> Result<Vc<IntrospectableChildren>> {
    let key = reference_ty();
    let mut children = IndexSet::new();
    let references = references.await?;
    for reference in &*references {
        let mut key = key;
        if let Some(chunkable) =
            Vc::try_resolve_downcast::<Box<dyn ChunkableAssetReference>>(*reference).await?
        {
            match &*chunkable.chunking_type().await? {
                None => {}
                Some(ChunkingType::Placed) => key = placed_reference_ty(),
                Some(ChunkingType::Parallel) => key = parallel_reference_ty(),
                Some(ChunkingType::IsolatedParallel) => key = isolated_parallel_reference_ty(),
                Some(ChunkingType::Separate) => key = separate_reference_ty(),
                Some(ChunkingType::PlacedOrParallel) => key = placed_or_parallel_reference_ty(),
                Some(ChunkingType::SeparateAsync) => key = async_reference_ty(),
            }
        }

        for result in reference.resolve_reference().await?.primary.iter() {
            if let PrimaryResolveResult::Asset(asset) = result {
                children.insert((key, IntrospectableAsset::new(*asset)));
            }
        }
    }
    Ok(Vc::cell(children))
}

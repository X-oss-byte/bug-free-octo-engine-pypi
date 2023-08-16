use anyhow::Result;
use indexmap::IndexSet;
use turbo_tasks::{primitives::StringVc, ValueToString};
use turbo_tasks_fs::FileContent;

use super::{Introspectable, IntrospectableChildrenVc, IntrospectableVc};
use crate::{
    asset::{Asset, AssetContent, AssetContentVc, AssetVc},
    reference::{AssetReference, AssetReferencesVc},
    resolve::PrimaryResolveResult,
};

#[turbo_tasks::value]
pub struct IntrospectableAsset(AssetVc);

#[turbo_tasks::value_impl]
impl IntrospectableAssetVc {
    #[turbo_tasks::function]
    pub async fn new(asset: AssetVc) -> Result<IntrospectableVc> {
        Ok(IntrospectableVc::resolve_from(asset)
            .await?
            .unwrap_or_else(|| IntrospectableAsset(asset).cell().into()))
    }
}

#[turbo_tasks::function]
fn asset_ty() -> StringVc {
    StringVc::cell("asset".to_string())
}

#[turbo_tasks::function]
fn reference_ty() -> StringVc {
    StringVc::cell("reference".to_string())
}

#[turbo_tasks::value_impl]
impl Introspectable for IntrospectableAsset {
    #[turbo_tasks::function]
    fn ty(&self) -> StringVc {
        asset_ty()
    }

    #[turbo_tasks::function]
    fn title(&self) -> StringVc {
        self.0.ident().to_string()
    }

    #[turbo_tasks::function]
    fn details(&self) -> StringVc {
        content_to_details(self.0.content())
    }

    #[turbo_tasks::function]
    fn children(&self) -> IntrospectableChildrenVc {
        children_from_asset_references(self.0.references())
    }
}

#[turbo_tasks::function]
pub async fn content_to_details(content: AssetContentVc) -> Result<StringVc> {
    Ok(match &*content.await? {
        AssetContent::File(file_content) => match &*file_content.await? {
            FileContent::Content(file) => {
                let content = file.content();
                match content.to_str() {
                    Ok(str) => StringVc::cell(str.into_owned()),
                    Err(_) => StringVc::cell(format!("{} binary bytes", content.len())),
                }
            }
            FileContent::NotFound => StringVc::cell("not found".to_string()),
        },
        AssetContent::Redirect { target, link_type } => {
            StringVc::cell(format!("redirect to {target} with type {link_type:?}"))
        }
    })
}

#[turbo_tasks::function]
pub async fn children_from_asset_references(
    references: AssetReferencesVc,
) -> Result<IntrospectableChildrenVc> {
    let key = reference_ty();
    let mut children = IndexSet::new();
    let references = references.await?;
    for reference in &*references {
        for result in reference.resolve_reference().await?.primary.iter() {
            if let PrimaryResolveResult::Asset(asset) = result {
                children.insert((key, IntrospectableAssetVc::new(*asset)));
            }
        }
    }
    Ok(IntrospectableChildrenVc::cell(children))
}

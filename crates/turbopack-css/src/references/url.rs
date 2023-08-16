use anyhow::Result;
use swc_core::{
    common::DUMMY_SP,
    css::ast::{Str, UrlValue},
};
use turbo_tasks::{primitives::StringVc, Value, ValueToString, ValueToStringVc};
use turbopack_core::{
    asset::{Asset, AssetVc},
    chunk::{ChunkingContext, ChunkingContextVc},
    reference::{AssetReference, AssetReferenceVc},
    reference_type::UrlReferenceSubType,
    resolve::{
        origin::{ResolveOrigin, ResolveOriginVc},
        parse::RequestVc,
        PrimaryResolveResult, ResolveResultVc,
    },
};
use turbopack_ecmascript::resolve::url_resolve;

use crate::{
    code_gen::{CodeGenerateable, CodeGenerateableVc, CodeGeneration, CodeGenerationVc},
    create_visitor,
    embed::{CssEmbed, CssEmbeddable, CssEmbeddableVc},
    references::AstPathVc,
};

#[turbo_tasks::value(into = "new")]
pub enum ReferencedAsset {
    Some(AssetVc),
    None,
}

#[turbo_tasks::value]
#[derive(Hash, Debug)]
pub struct UrlAssetReference {
    pub origin: ResolveOriginVc,
    pub request: RequestVc,
    pub path: AstPathVc,
}

#[turbo_tasks::value_impl]
impl UrlAssetReferenceVc {
    #[turbo_tasks::function]
    pub fn new(origin: ResolveOriginVc, request: RequestVc, path: AstPathVc) -> Self {
        Self::cell(UrlAssetReference {
            origin,
            request,
            path,
        })
    }

    #[turbo_tasks::function]
    async fn get_referenced_asset(self, context: ChunkingContextVc) -> Result<ReferencedAssetVc> {
        for result in self.resolve_reference().await?.primary.iter() {
            if let PrimaryResolveResult::Asset(asset) = result {
                if let Some(embeddable) = CssEmbeddableVc::resolve_from(asset).await? {
                    return Ok(ReferencedAsset::Some(
                        embeddable.as_css_embed(context).embeddable_asset(),
                    )
                    .into());
                }
            }
        }
        Ok(ReferencedAssetVc::cell(ReferencedAsset::None))
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for UrlAssetReference {
    #[turbo_tasks::function]
    fn resolve_reference(&self) -> ResolveResultVc {
        url_resolve(
            self.origin,
            self.request,
            Value::new(UrlReferenceSubType::CssUrl),
        )
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for UrlAssetReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        Ok(StringVc::cell(format!(
            "url {}",
            self.request.to_string().await?,
        )))
    }
}

#[turbo_tasks::value_impl]
impl CodeGenerateable for UrlAssetReference {
    #[turbo_tasks::function]
    async fn code_generation(
        self_vc: UrlAssetReferenceVc,
        context: ChunkingContextVc,
    ) -> Result<CodeGenerationVc> {
        let this = self_vc.await?;
        let chunk_path = context.chunk_path(this.origin.origin_path(), ".css");
        let context_path = chunk_path.parent().await?;

        let mut visitors = Vec::new();

        if let ReferencedAsset::Some(asset) = &*self_vc.get_referenced_asset(context).await? {
            let path = asset.path().await?;
            let relative_path = context_path
                .get_relative_path_to(&path)
                .unwrap_or_else(|| format!("/{}", path.path));

            visitors.push(
                create_visitor!((&this.path.await?), visit_mut_url(u: &mut Url) {
                    u.value = Some(box UrlValue::Str(Str {
                        span: DUMMY_SP,
                        value: relative_path.as_str().into(),
                        raw: None,
                    }))
                }),
            );
        }

        Ok(CodeGeneration {
            visitors,
            imports: vec![],
        }
        .into())
    }
}

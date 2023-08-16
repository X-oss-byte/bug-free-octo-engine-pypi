use anyhow::Result;
use swc_core::{ecma::ast::Expr, quote};
use turbo_tasks::{ValueToString, Vc};
use turbopack_core::{
    chunk::{ChunkableAssetReference, ChunkingTypeOption, ModuleId},
    reference::AssetReference,
    resolve::ResolveResult,
};

use super::{base::ReferencedAsset, EsmAssetReference};
use crate::{
    chunk::{item::EcmascriptChunkItemExt, EcmascriptChunkPlaceable, EcmascriptChunkingContext},
    code_gen::{CodeGenerateable, CodeGeneration},
    create_visitor,
    references::AstPath,
};

#[turbo_tasks::value]
#[derive(Hash, Debug)]
pub struct EsmModuleIdAssetReference {
    inner: Vc<EsmAssetReference>,
    ast_path: Vc<AstPath>,
}

#[turbo_tasks::value_impl]
impl EsmModuleIdAssetReference {
    #[turbo_tasks::function]
    pub fn new(inner: Vc<EsmAssetReference>, ast_path: Vc<AstPath>) -> Vc<Self> {
        Self::cell(EsmModuleIdAssetReference { inner, ast_path })
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for EsmModuleIdAssetReference {
    #[turbo_tasks::function]
    fn resolve_reference(&self) -> Vc<ResolveResult> {
        self.inner.resolve_reference()
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for EsmModuleIdAssetReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(format!(
            "module id of {}",
            self.inner.to_string().await?,
        )))
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAssetReference for EsmModuleIdAssetReference {
    #[turbo_tasks::function]
    fn chunking_type(&self) -> Vc<ChunkingTypeOption> {
        self.inner.chunking_type()
    }
}

#[turbo_tasks::value_impl]
impl CodeGenerateable for EsmModuleIdAssetReference {
    #[turbo_tasks::function]
    async fn code_generation(
        &self,
        context: Vc<Box<dyn EcmascriptChunkingContext>>,
    ) -> Result<Vc<CodeGeneration>> {
        let mut visitors = Vec::new();

        if let ReferencedAsset::Some(asset) = &*self.inner.get_referenced_asset().await? {
            let id = asset.as_chunk_item(context).id().await?;
            visitors.push(
                create_visitor!(self.ast_path.await?, visit_mut_expr(expr: &mut Expr) {
                    *expr = Expr::Lit(match &*id {
                        ModuleId::String(s) => s.clone().into(),
                        ModuleId::Number(n) => (*n as f64).into(),
                    })
                }),
            );
        } else {
            // If the referenced asset can't be found, replace the expression with null.
            // This can happen if the referenced asset is an external, or doesn't resolve
            // to anything.
            visitors.push(
                create_visitor!(self.ast_path.await?, visit_mut_expr(expr: &mut Expr) {
                    *expr = quote!("null" as Expr);
                }),
            );
        }

        Ok(CodeGeneration { visitors }.into())
    }
}

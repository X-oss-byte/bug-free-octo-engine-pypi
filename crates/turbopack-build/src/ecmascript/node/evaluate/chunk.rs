use std::io::Write;

use anyhow::{bail, Result};
use indoc::writedoc;
use turbo_tasks::{primitives::StringVc, Value, ValueToString, ValueToStringVc};
use turbo_tasks_fs::File;
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc, AssetsVc},
    chunk::{ChunkVc, ChunkingContext, EvaluatableAssetsVc},
    code_builder::{CodeBuilder, CodeVc},
    ident::AssetIdentVc,
    reference::{AssetReferencesVc, SingleAssetReferenceVc},
    source_map::{
        GenerateSourceMap, GenerateSourceMapVc, OptionSourceMapVc, SourceMapAssetReferenceVc,
    },
};
use turbopack_ecmascript::{
    chunk::{EcmascriptChunkPlaceable, EcmascriptChunkPlaceableVc},
    utils::StringifyJs,
    EcmascriptModuleAssetVc,
};

use super::runtime::EcmascriptBuildNodeRuntimeReferenceVc;
use crate::BuildChunkingContextVc;

/// An Ecmascript chunk that loads a list of parallel chunks, then instantiates
/// runtime entries.
#[turbo_tasks::value(shared)]
pub(crate) struct EcmascriptBuildNodeEvaluateChunk {
    chunking_context: BuildChunkingContextVc,
    entry_chunk: ChunkVc,
    other_chunks: AssetsVc,
    evaluatable_assets: EvaluatableAssetsVc,
    exported_module: Option<EcmascriptModuleAssetVc>,
}

#[turbo_tasks::value_impl]
impl EcmascriptBuildNodeEvaluateChunkVc {
    /// Creates a new [`EcmascriptBuildNodeEvaluateChunkVc`].
    #[turbo_tasks::function]
    pub fn new(
        chunking_context: BuildChunkingContextVc,
        entry_chunk: ChunkVc,
        other_chunks: AssetsVc,
        evaluatable_assets: EvaluatableAssetsVc,
        exported_module: Option<EcmascriptModuleAssetVc>,
    ) -> Self {
        EcmascriptBuildNodeEvaluateChunk {
            chunking_context,
            entry_chunk,
            other_chunks,
            evaluatable_assets,
            exported_module,
        }
        .cell()
    }

    #[turbo_tasks::function]
    async fn code(self) -> Result<CodeVc> {
        let this = self.await?;

        let output_root = this.chunking_context.output_root().await?;
        let chunk_path = self.ident().path().await?;
        let chunk_directory = self.ident().path().parent().await?;
        let runtime_path = self
            .runtime_reference()
            .runtime_chunk()
            .ident()
            .path()
            .await?;
        let runtime_relative_path =
            if let Some(path) = chunk_directory.get_relative_path_to(&runtime_path) {
                path
            } else {
                bail!(
                    "cannot find a relative path from the chunk ({}) to the runtime chunk ({})",
                    chunk_path.to_string(),
                    runtime_path.to_string(),
                );
            };
        let chunk_public_path = if let Some(path) = output_root.get_path_to(&chunk_path) {
            path
        } else {
            bail!(
                "chunk path ({}) is not in output root ({})",
                chunk_path.to_string(),
                output_root.to_string()
            );
        };

        let mut code = CodeBuilder::default();

        writedoc!(
            code,
            r#"
                const CHUNK_PUBLIC_PATH = {};
                const runtime = require({});
            "#,
            StringifyJs(chunk_public_path),
            StringifyJs(&*runtime_relative_path)
        )?;

        let other_chunks = this.other_chunks.await?;
        for other_chunk in &*other_chunks {
            let other_chunk_path = &*other_chunk.ident().path().await?;
            if let Some(other_chunk_public_path) = output_root.get_path_to(other_chunk_path) {
                writedoc!(
                    code,
                    // TODO(WEB-1112) This should call `require()` directly, perhaps as an argument
                    // to `loadChunk`.
                    r#"
                        runtime.loadChunk({});
                    "#,
                    StringifyJs(&other_chunk_public_path)
                )?;
            }
        }

        let evaluatable_assets = this.evaluatable_assets.await?;
        for evaluatable_asset in &*evaluatable_assets {
            if let Some(placeable) =
                EcmascriptChunkPlaceableVc::resolve_from(evaluatable_asset).await?
            {
                let runtime_module_id = placeable
                    .as_chunk_item(this.chunking_context.into())
                    .id()
                    .await?;

                writedoc!(
                    code,
                    r#"
                        runtime.getOrInstantiateRuntimeModule({}, CHUNK_PUBLIC_PATH);
                    "#,
                    StringifyJs(&*runtime_module_id),
                )?;
            }
        }

        if let Some(exported_module) = this.exported_module {
            let runtime_module_id = exported_module
                .as_chunk_item(this.chunking_context.into())
                .id()
                .await?;

            writedoc!(
                code,
                r#"
                    module.exports = runtime.getOrInstantiateRuntimeModule({}, CHUNK_PUBLIC_PATH).exports;
                "#,
                StringifyJs(&*runtime_module_id),
            )?;
        }

        Ok(CodeVc::cell(code.build()))
    }

    #[turbo_tasks::function]
    async fn runtime_reference(self) -> Result<EcmascriptBuildNodeRuntimeReferenceVc> {
        let this = self.await?;
        Ok(EcmascriptBuildNodeRuntimeReferenceVc::new(
            this.chunking_context,
        ))
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for EcmascriptBuildNodeEvaluateChunk {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        Ok(StringVc::cell(
            "Ecmascript Build Node Evaluate Chunk".to_string(),
        ))
    }
}

#[turbo_tasks::function]
fn modifier() -> StringVc {
    StringVc::cell("ecmascript build node evaluate chunk".to_string())
}

#[turbo_tasks::function]
fn chunk_reference_description() -> StringVc {
    StringVc::cell("chunk".to_string())
}

#[turbo_tasks::value_impl]
impl Asset for EcmascriptBuildNodeEvaluateChunk {
    #[turbo_tasks::function]
    async fn ident(&self) -> Result<AssetIdentVc> {
        let mut ident = self.entry_chunk.ident().await?.clone_value();

        ident.add_modifier(modifier());

        ident.modifiers.extend(
            self.evaluatable_assets
                .await?
                .iter()
                .map(|entry| entry.ident().to_string()),
        );

        for chunk in &*self.other_chunks.await? {
            ident.add_modifier(chunk.ident().to_string());
        }

        let ident = AssetIdentVc::new(Value::new(ident));
        Ok(AssetIdentVc::from_path(
            self.chunking_context.chunk_path(ident, ".js"),
        ))
    }

    #[turbo_tasks::function]
    async fn references(self_vc: EcmascriptBuildNodeEvaluateChunkVc) -> Result<AssetReferencesVc> {
        let this = self_vc.await?;
        let mut references = vec![self_vc.runtime_reference().into()];

        if *this
            .chunking_context
            .reference_chunk_source_maps(self_vc.into())
            .await?
        {
            references.push(SourceMapAssetReferenceVc::new(self_vc.into()).into())
        }

        let other_chunks = this.other_chunks.await?;
        for other_chunk in &*other_chunks {
            references.push(
                SingleAssetReferenceVc::new(*other_chunk, chunk_reference_description()).into(),
            );
        }

        Ok(AssetReferencesVc::cell(references))
    }

    #[turbo_tasks::function]
    async fn content(self_vc: EcmascriptBuildNodeEvaluateChunkVc) -> Result<AssetContentVc> {
        let code = self_vc.code().await?;
        Ok(File::from(code.source_code().clone()).into())
    }
}

#[turbo_tasks::value_impl]
impl GenerateSourceMap for EcmascriptBuildNodeEvaluateChunk {
    #[turbo_tasks::function]
    fn generate_source_map(self_vc: EcmascriptBuildNodeEvaluateChunkVc) -> OptionSourceMapVc {
        self_vc.code().generate_source_map()
    }
}

use anyhow::Result;
use indexmap::IndexSet;
use turbo_tasks::{TryJoinIterExt, Value, ValueToString, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    asset::{Asset, AssetContent, Assets},
    chunk::{
        availability_info::AvailabilityInfo, Chunk, ChunkItem, ChunkableAsset, ChunkingContext,
        EvaluatableAssets,
    },
    ident::AssetIdent,
    introspect::{
        asset::{content_to_details, IntrospectableAsset},
        Introspectable, IntrospectableChildren,
    },
    reference::{AssetReferences, SingleAssetReference},
};

use crate::{
    chunk::{
        EcmascriptChunk, EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkPlaceable,
        EcmascriptChunkingContext, EcmascriptExports,
    },
    utils::StringifyJs,
    EcmascriptModuleAsset,
};

#[turbo_tasks::function]
fn modifier() -> Vc<String> {
    Vc::cell("chunk group files".to_string())
}

/// An asset that exports a list of chunk URLs by putting the [asset] into a
/// ChunkGroup with the provided ChunkingContext.
#[turbo_tasks::value(shared)]
pub struct ChunkGroupFilesAsset {
    pub asset: Vc<Box<dyn ChunkableAsset>>,
    pub client_root: Vc<FileSystemPath>,
    pub chunking_context: Vc<Box<dyn ChunkingContext>>,
    pub runtime_entries: Option<Vc<EvaluatableAssets>>,
}

#[turbo_tasks::value_impl]
impl ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    async fn chunks(self: Vc<Self>) -> Result<Vc<Assets>> {
        let this = self.await?;
        Ok(
            if let Some(ecma) =
                Vc::try_resolve_downcast_type::<EcmascriptModuleAsset>(this.asset).await?
            {
                this.chunking_context.evaluated_chunk_group(
                    ecma.as_root_chunk(this.chunking_context),
                    this.runtime_entries
                        .unwrap_or_else(EvaluatableAssets::empty)
                        .with_entry(Vc::upcast(ecma)),
                )
            } else {
                this.chunking_context
                    .chunk_group(this.asset.as_root_chunk(this.chunking_context))
            },
        )
    }
}

#[turbo_tasks::function]
fn chunk_group_chunk_reference_description() -> Vc<String> {
    Vc::cell("chunk group chunk".to_string())
}

#[turbo_tasks::value_impl]
impl Asset for ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    fn ident(&self) -> Vc<AssetIdent> {
        self.asset.ident().with_modifier(modifier())
    }

    #[turbo_tasks::function]
    fn content(&self) -> Vc<AssetContent> {
        unimplemented!()
    }

    #[turbo_tasks::function]
    async fn references(self: Vc<Self>) -> Result<Vc<AssetReferences>> {
        let chunks = self.chunks();

        Ok(Vc::cell(
            chunks
                .await?
                .iter()
                .copied()
                .map(|chunk| {
                    SingleAssetReference::new(chunk, chunk_group_chunk_reference_description())
                })
                .map(Vc::upcast)
                .collect(),
        ))
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    fn as_chunk(
        self: Vc<Self>,
        context: Vc<Box<dyn ChunkingContext>>,
        availability_info: Value<AvailabilityInfo>,
    ) -> Vc<Box<dyn Chunk>> {
        Vc::upcast(EcmascriptChunk::new(
            context,
            Vc::upcast(self),
            availability_info,
        ))
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    async fn as_chunk_item(
        self: Vc<Self>,
        context: Vc<Box<dyn EcmascriptChunkingContext>>,
    ) -> Result<Vc<Box<dyn EcmascriptChunkItem>>> {
        let this = self.await?;
        Ok(Vc::upcast(
            ChunkGroupFilesChunkItem {
                context,
                client_root: this.client_root,
                inner: self,
                chunk: this.asset.as_chunk(
                    Vc::upcast(context),
                    Value::new(AvailabilityInfo::Root {
                        current_availability_root: Vc::upcast(this.asset),
                    }),
                ),
            }
            .cell(),
        ))
    }

    #[turbo_tasks::function]
    fn get_exports(&self) -> Vc<EcmascriptExports> {
        EcmascriptExports::Value.cell()
    }
}

#[turbo_tasks::value]
struct ChunkGroupFilesChunkItem {
    context: Vc<Box<dyn EcmascriptChunkingContext>>,
    client_root: Vc<FileSystemPath>,
    inner: Vc<ChunkGroupFilesAsset>,
    chunk: Vc<Box<dyn Chunk>>,
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for ChunkGroupFilesChunkItem {
    #[turbo_tasks::function]
    fn chunking_context(&self) -> Vc<Box<dyn EcmascriptChunkingContext>> {
        self.context
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<Vc<EcmascriptChunkItemContent>> {
        let chunks = self.inner.chunks();
        let client_root = self.client_root.await?;
        let chunks_paths = chunks
            .await?
            .iter()
            .map(|chunk| chunk.ident().path())
            .try_join()
            .await?;
        let chunks_paths: Vec<_> = chunks_paths
            .iter()
            .filter_map(|path| client_root.get_path_to(path))
            .collect();
        Ok(EcmascriptChunkItemContent {
            inner_code: format!(
                "__turbopack_export_value__({:#});\n",
                StringifyJs(&chunks_paths)
            )
            .into(),
            ..Default::default()
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl ChunkItem for ChunkGroupFilesChunkItem {
    #[turbo_tasks::function]
    fn asset_ident(&self) -> Vc<AssetIdent> {
        self.inner.ident()
    }

    #[turbo_tasks::function]
    fn references(&self) -> Vc<AssetReferences> {
        self.inner.references()
    }
}

#[turbo_tasks::value_impl]
impl Introspectable for ChunkGroupFilesAsset {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        Vc::cell("chunk group files asset".to_string())
    }

    #[turbo_tasks::function]
    fn details(self: Vc<Self>) -> Vc<String> {
        content_to_details(self.content())
    }

    #[turbo_tasks::function]
    fn title(self: Vc<Self>) -> Vc<String> {
        self.ident().to_string()
    }

    #[turbo_tasks::function]
    async fn children(self: Vc<Self>) -> Result<Vc<IntrospectableChildren>> {
        let mut children = IndexSet::new();
        let chunk_ty = Vc::cell("chunk".to_string());
        for &chunk in self.chunks().await?.iter() {
            children.insert((chunk_ty, IntrospectableAsset::new(chunk)));
        }
        children.insert((
            Vc::cell("inner asset".to_string()),
            IntrospectableAsset::new(Vc::upcast(self.await?.asset)),
        ));
        Ok(Vc::cell(children))
    }
}

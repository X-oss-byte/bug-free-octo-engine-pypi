pub mod availability_info;
pub mod available_assets;
pub mod chunking;
pub(crate) mod chunking_context;
pub(crate) mod containment_tree;
pub(crate) mod data;
pub(crate) mod evaluate;
pub mod optimize;
pub(crate) mod passthrough_asset;

use std::{
    fmt::{Debug, Display, Write},
    future::Future,
    hash::Hash,
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use turbo_tasks::{
    debug::ValueDebugFormat,
    graph::{GraphTraversal, Visit},
    primitives::StringVc,
    trace::TraceRawVcs,
    TryJoinIterExt, ValueToString, ValueToStringVc,
};
use turbo_tasks_hash::DeterministicHash;

pub use self::{
    chunking_context::{ChunkingContext, ChunkingContextVc},
    data::{ChunkData, ChunkDataOption, ChunkDataOptionVc, ChunkDataVc, ChunksData, ChunksDataVc},
    evaluate::{EvaluatableAsset, EvaluatableAssetVc, EvaluatableAssets, EvaluatableAssetsVc},
    passthrough_asset::{PassthroughAsset, PassthroughAssetVc},
};
use crate::{
    asset::{Asset, AssetVc, AssetsVc},
    ident::AssetIdentVc,
    introspect::{
        asset::children_from_asset_references, Introspectable, IntrospectableChildrenVc,
        IntrospectableVc,
    },
    reference::{AssetReference, AssetReferenceVc, AssetReferencesVc},
};

/// A module id, which can be a number or string
#[turbo_tasks::value(shared)]
#[derive(Debug, Clone, Hash, Ord, PartialOrd, DeterministicHash)]
#[serde(untagged)]
pub enum ModuleId {
    Number(u32),
    String(String),
}

impl Display for ModuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModuleId::Number(i) => write!(f, "{}", i),
            ModuleId::String(s) => write!(f, "{}", s),
        }
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for ModuleId {
    #[turbo_tasks::function]
    fn to_string(&self) -> StringVc {
        StringVc::cell(self.to_string())
    }
}

impl ModuleId {
    pub fn parse(id: &str) -> Result<ModuleId> {
        Ok(match id.parse::<u32>() {
            Ok(i) => ModuleId::Number(i),
            Err(_) => ModuleId::String(id.to_string()),
        })
    }
}

/// A list of module ids.
#[turbo_tasks::value(transparent, shared)]
pub struct ModuleIds(Vec<ModuleIdVc>);

/// An [Asset] that can be converted into a [Chunk].
#[turbo_tasks::value_trait]
pub trait ChunkableAsset: Asset {
    fn as_chunk_item(&self, context: ChunkingContextVc) -> ChunkItemVc;
}

#[turbo_tasks::value]
pub struct ChunkIdent {
    pub entry_asset_ident: AssetIdentVc,
    pub available_assets_hash: String,
    pub split_piece: String,
}

#[turbo_tasks::value_impl]
impl ChunkIdentVc {
    #[turbo_tasks::function]
    pub fn new(
        entry_asset_ident: AssetIdentVc,
        available_assets_hash: StringVc,
        split_piece: StringVc,
    ) -> ChunkIdentVc {
        Self::cell(ChunkIdent {
            entry_asset_ident,
            available_assets_hash: available_assets_hash.into(),
            split_piece: split_piece.into(),
        })
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for ChunkIdent {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        Ok(StringVc::cell(format!(
            "{}-{}-{}",
            self.available_assets_hash,
            self.entry_asset_ident.to_string().await?,
            self.split_piece
        )))
    }
}

#[turbo_tasks::value(transparent)]
pub struct Chunks(Vec<ChunkVc>);

#[turbo_tasks::value_impl]
impl ChunksVc {
    /// Creates a new empty [ChunksVc].
    #[turbo_tasks::function]
    pub fn empty() -> ChunksVc {
        Self::cell(vec![])
    }
}

/// A chunk is a collections of chunk items. The ChunkingContext will convert a
/// Chunk into an Asset by using a runtime specific implementation.
#[turbo_tasks::value]
pub struct Chunk {
    pub ty: ChunkTypeVc,
    pub chunking_context: ChunkingContextVc,
    /// An identifier of the chunk to make it uniquely identifiable. This is the
    /// source of creating a chunk id and chunk filename.
    pub ident: ChunkIdentVc,
    /// All the items in the chunk
    pub items: ChunkItemsVc,
    /// Chunk external references of all chunk items of the whole chunk group.
    pub references: AssetReferencesVc,
}

#[turbo_tasks::value_impl]
impl ChunkVc {
    #[turbo_tasks::function]
    pub async fn ident(self) -> Result<ChunkIdentVc> {
        Ok(self.await?.ident)
    }
}

#[turbo_tasks::function]
fn introspectable_type() -> StringVc {
    StringVc::cell("chunk".to_string())
}

#[turbo_tasks::value_impl]
impl Introspectable for Chunk {
    #[turbo_tasks::function]
    fn ty(&self) -> StringVc {
        introspectable_type()
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<StringVc> {
        Ok(StringVc::cell(format!(
            "{} {}",
            self.ty.name().await?,
            self.ident.to_string().await?
        )))
    }

    #[turbo_tasks::function]
    async fn details(&self) -> Result<StringVc> {
        let mut details = String::new();
        for chunk_item in self.items.await?.iter() {
            writeln!(details, "- {}", chunk_item.asset_ident().to_string().await?)?;
        }
        Ok(StringVc::cell(details))
    }

    #[turbo_tasks::function]
    async fn children(&self) -> IntrospectableChildrenVc {
        children_from_asset_references(self.references)
    }
}

/// Aggregated information about a chunk content that can be used by the runtime
/// code to optimize chunk loading.
#[turbo_tasks::value(shared)]
#[derive(Default)]
pub struct OutputChunkRuntimeInfo {
    pub included_ids: Option<ModuleIdsVc>,
    pub excluded_ids: Option<ModuleIdsVc>,
    /// List of paths of chunks containing individual modules that are part of
    /// this chunk. This is useful for selectively loading modules from a chunk
    /// without loading the whole chunk.
    pub module_chunks: Option<AssetsVc>,
    pub placeholder_for_future_extensions: (),
}

#[turbo_tasks::value_trait]
pub trait OutputChunk: Asset {
    fn runtime_info(&self) -> OutputChunkRuntimeInfoVc;
}

/// Specifies how a chunk interacts with other chunks when building a chunk
/// group
#[derive(
    Copy, Default, Clone, Hash, TraceRawVcs, Serialize, Deserialize, Eq, PartialEq, ValueDebugFormat,
)]
pub enum ChunkingType {
    /// Asset is always placed into the referencing chunk and loaded with it.
    Placed,
    /// A heuristic determines if the asset is placed into the referencing chunk
    /// or in a separate chunk that is loaded in parallel.
    #[default]
    PlacedOrParallel,
    /// Asset is always placed in a separate chunk that is loaded in parallel.
    Parallel,
    /// Asset is always placed in a separate chunk that is loaded in parallel.
    /// Referenced asset will not inherit the available modules, but form a
    /// new availability root.
    IsolatedParallel,
    /// An async loader is placed into the referencing chunk and loads the
    /// separate chunk group in which the asset is placed.
    Async,
}

#[turbo_tasks::value(transparent)]
pub struct ChunkingTypeOption(Option<ChunkingType>);

/// An [AssetReference] implementing this trait and returning true for
/// [ChunkableAssetReference::is_chunkable] are considered as potentially
/// chunkable references. When all [Asset]s of such a reference implement
/// [ChunkableAsset] they are placed in [Chunk]s during chunking.
/// They are even potentially placed in the same [Chunk] when a chunk type
/// specific interface is implemented.
#[turbo_tasks::value_trait]
pub trait ChunkableAssetReference: AssetReference + ValueToString {
    fn chunking_type(&self) -> ChunkingTypeOptionVc {
        ChunkingTypeOptionVc::cell(Some(ChunkingType::default()))
    }
}

/// A reference to multiple chunks from a [ChunkGroup]
#[turbo_tasks::value]
pub struct ChunkGroupReference {
    chunking_context: ChunkingContextVc,
    entry: ChunkVc,
}

#[turbo_tasks::value_impl]
impl ChunkGroupReferenceVc {
    #[turbo_tasks::function]
    pub fn new(chunking_context: ChunkingContextVc, entry: ChunkVc) -> Self {
        Self::cell(ChunkGroupReference {
            chunking_context,
            entry,
        })
    }

    #[turbo_tasks::function]
    async fn chunks(self) -> Result<AssetsVc> {
        let this = self.await?;
        Ok(this.chunking_context.chunk_group(this.entry))
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for ChunkGroupReference {
    #[turbo_tasks::function]
    async fn resolve_reference(self_vc: ChunkGroupReferenceVc) -> Result<ResolveResultVc> {
        let set = self_vc.chunks().await?.clone_value();
        Ok(ResolveResult::assets(set).into())
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for ChunkGroupReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        Ok(StringVc::cell(format!(
            "chunk group ({})",
            self.entry.ident().to_string().await?
        )))
    }
}

#[turbo_tasks::value_trait]
pub trait ChunkItem {
    /// The [AssetIdent] of the [Asset] that this [ChunkItem] was created from.
    /// For most chunk types this must uniquely identify the asset as it's the
    /// source of the module id used at runtime.
    fn asset_ident(&self) -> AssetIdentVc;
    /// A [ChunkItem] can describe different `references` than its original
    /// [Asset].
    /// TODO(alexkirsz) This should have a default impl that returns empty
    /// references.
    fn references(&self) -> AssetReferencesVc;
    /// The [ChunkType] of this [ChunkItem]. This will be used to combine
    /// multiple chunk items into a chunk of a specific type.
    fn chunk_type(&self) -> ChunkTypeVc;
}

#[turbo_tasks::value(transparent)]
pub struct ChunkItems(Vec<ChunkItemVc>);

#[turbo_tasks::value_trait]
pub trait ChunkType {
    fn name(&self) -> StringVc;
}

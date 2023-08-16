pub mod availability_info;
pub mod available_assets;
pub(crate) mod chunking_context;
pub(crate) mod containment_tree;
pub(crate) mod data;
pub(crate) mod evaluate;
pub mod optimize;

use std::{
    collections::HashSet,
    fmt::{Debug, Display},
    future::Future,
    hash::Hash,
    marker::PhantomData,
};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tracing::{info_span, Span};
use turbo_tasks::{
    debug::ValueDebugFormat,
    graph::{GraphTraversal, GraphTraversalResult, ReverseTopological, Visit, VisitControlFlow},
    trace::TraceRawVcs,
    ReadRef, TryJoinIterExt, Value, ValueToString, Vc,
};
use turbo_tasks_fs::FileSystemPath;
use turbo_tasks_hash::DeterministicHash;

use self::availability_info::AvailabilityInfo;
pub use self::{
    chunking_context::ChunkingContext,
    data::{ChunkData, ChunkDataOption, ChunksData},
    evaluate::{EvaluatableAsset, EvaluatableAssetExt, EvaluatableAssets},
};
use crate::{
    asset::{Asset, Assets},
    ident::AssetIdent,
    reference::{AssetReference, AssetReferences},
    resolve::{PrimaryResolveResult, ResolveResult},
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
    fn to_string(&self) -> Vc<String> {
        Vc::cell(self.to_string())
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
pub struct ModuleIds(Vec<Vc<ModuleId>>);

/// An [Asset] that can be converted into a [Chunk].
#[turbo_tasks::value_trait]
pub trait ChunkableAsset: Asset {
    fn as_chunk(
        self: Vc<Self>,
        context: Vc<Box<dyn ChunkingContext>>,
        availability_info: Value<AvailabilityInfo>,
    ) -> Vc<Box<dyn Chunk>>;

    fn as_root_chunk(self: Vc<Self>, context: Vc<Box<dyn ChunkingContext>>) -> Vc<Box<dyn Chunk>> {
        self.as_chunk(
            context,
            Value::new(AvailabilityInfo::Root {
                current_availability_root: Vc::upcast(self),
            }),
        )
    }
}

#[turbo_tasks::value(transparent)]
pub struct Chunks(Vec<Vc<Box<dyn Chunk>>>);

#[turbo_tasks::value_impl]
impl Chunks {
    /// Creates a new empty [Vc<Chunks>].
    #[turbo_tasks::function]
    pub fn empty() -> Vc<Self> {
        Vc::cell(vec![])
    }
}

/// A chunk is one type of asset.
/// It usually contains multiple chunk items.
#[turbo_tasks::value_trait]
pub trait Chunk: Asset {
    fn chunking_context(self: Vc<Self>) -> Vc<Box<dyn ChunkingContext>>;
    // TODO Once output assets have their own trait, this path() method will move
    // into that trait and ident() will be removed from that. Assets on the
    // output-level only have a path and no complex ident.
    /// The path of the chunk.
    fn path(self: Vc<Self>) -> Vc<FileSystemPath> {
        self.ident().path()
    }
    /// Returns a list of chunks that should be loaded in parallel to this
    /// chunk.
    fn parallel_chunks(self: Vc<Self>) -> Vc<Chunks> {
        Chunks::empty()
    }
}

/// Aggregated information about a chunk content that can be used by the runtime
/// code to optimize chunk loading.
#[turbo_tasks::value(shared)]
#[derive(Default)]
pub struct OutputChunkRuntimeInfo {
    pub included_ids: Option<Vc<ModuleIds>>,
    pub excluded_ids: Option<Vc<ModuleIds>>,
    /// List of paths of chunks containing individual modules that are part of
    /// this chunk. This is useful for selectively loading modules from a chunk
    /// without loading the whole chunk.
    pub module_chunks: Option<Vc<Assets>>,
    pub placeholder_for_future_extensions: (),
}

#[turbo_tasks::value_trait]
pub trait OutputChunk: Asset {
    fn runtime_info(self: Vc<Self>) -> Vc<OutputChunkRuntimeInfo>;
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
    /// Asset is placed in a separate chunk group that is referenced from the
    /// referencing chunk group, but not loaded.
    /// Note: Separate chunks need to be loaded by something external to current
    /// reference.
    Separate,
    /// An async loader is placed into the referencing chunk and loads the
    /// separate chunk group in which the asset is placed.
    SeparateAsync,
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
    fn chunking_type(self: Vc<Self>) -> Vc<ChunkingTypeOption> {
        Vc::cell(Some(ChunkingType::default()))
    }
}

/// A reference to multiple chunks from a [ChunkGroup]
#[turbo_tasks::value]
pub struct ChunkGroupReference {
    chunking_context: Vc<Box<dyn ChunkingContext>>,
    entry: Vc<Box<dyn Chunk>>,
}

#[turbo_tasks::value_impl]
impl ChunkGroupReference {
    #[turbo_tasks::function]
    pub fn new(
        chunking_context: Vc<Box<dyn ChunkingContext>>,
        entry: Vc<Box<dyn Chunk>>,
    ) -> Vc<Self> {
        Self::cell(ChunkGroupReference {
            chunking_context,
            entry,
        })
    }

    #[turbo_tasks::function]
    async fn chunks(self: Vc<Self>) -> Result<Vc<Assets>> {
        let this = self.await?;
        Ok(this.chunking_context.chunk_group(this.entry))
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for ChunkGroupReference {
    #[turbo_tasks::function]
    async fn resolve_reference(self: Vc<Self>) -> Result<Vc<ResolveResult>> {
        let set = self.chunks().await?.clone_value();
        Ok(ResolveResult::assets(set).into())
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for ChunkGroupReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(format!(
            "chunk group ({})",
            self.entry.ident().to_string().await?
        )))
    }
}

pub struct ChunkContentResult<I> {
    pub chunk_items: Vec<I>,
    pub chunks: Vec<Vc<Box<dyn Chunk>>>,
    pub async_chunk_group_entries: Vec<Vc<Box<dyn Chunk>>>,
    pub external_asset_references: Vec<Vc<Box<dyn AssetReference>>>,
    pub availability_info: AvailabilityInfo,
}

#[async_trait::async_trait]
pub trait FromChunkableAsset: ChunkItem {
    async fn from_asset(
        context: Vc<Box<dyn ChunkingContext>>,
        asset: Vc<Box<dyn Asset>>,
    ) -> Result<Option<Vc<Self>>>;
    async fn from_async_asset(
        context: Vc<Box<dyn ChunkingContext>>,
        asset: Vc<Box<dyn ChunkableAsset>>,
        availability_info: Value<AvailabilityInfo>,
    ) -> Result<Option<Vc<Self>>>;
}

pub async fn chunk_content_split<I>(
    context: Vc<Box<dyn ChunkingContext>>,
    entry: Vc<Box<dyn Asset>>,
    additional_entries: Option<Vc<Assets>>,
    availability_info: Value<AvailabilityInfo>,
) -> Result<ChunkContentResult<Vc<I>>>
where
    I: FromChunkableAsset,
{
    chunk_content_internal_parallel(context, entry, additional_entries, availability_info, true)
        .await
        .map(|o| o.unwrap())
}

pub async fn chunk_content<I>(
    context: Vc<Box<dyn ChunkingContext>>,
    entry: Vc<Box<dyn Asset>>,
    additional_entries: Option<Vc<Assets>>,
    availability_info: Value<AvailabilityInfo>,
) -> Result<Option<ChunkContentResult<Vc<I>>>>
where
    I: FromChunkableAsset,
{
    chunk_content_internal_parallel(context, entry, additional_entries, availability_info, false)
        .await
}

#[derive(Eq, PartialEq, Clone, Hash)]
enum ChunkContentGraphNode<I> {
    // Chunk items that are placed into the current chunk
    ChunkItem { item: I, ident: ReadRef<String> },
    // Asset that is already available and doesn't need to be included
    AvailableAsset(Vc<Box<dyn Asset>>),
    // Chunks that are loaded in parallel to the current chunk
    Chunk(Vc<Box<dyn Chunk>>),
    // Chunk groups that are referenced from the current chunk, but
    // not loaded in parallel
    AsyncChunkGroup { entry: Vc<Box<dyn Chunk>> },
    ExternalAssetReference(Vc<Box<dyn AssetReference>>),
}

#[derive(Clone, Copy)]
struct ChunkContentContext {
    chunking_context: Vc<Box<dyn ChunkingContext>>,
    entry: Vc<Box<dyn Asset>>,
    availability_info: Value<AvailabilityInfo>,
    split: bool,
}

async fn reference_to_graph_nodes<I>(
    context: ChunkContentContext,
    reference: Vc<Box<dyn AssetReference>>,
) -> Result<
    Vec<(
        Option<(Vc<Box<dyn Asset>>, ChunkingType)>,
        ChunkContentGraphNode<Vc<I>>,
    )>,
>
where
    I: FromChunkableAsset,
{
    let Some(chunkable_asset_reference) = Vc::try_resolve_downcast::<Box<dyn ChunkableAssetReference>>(reference).await? else {
        return Ok(vec![(None, ChunkContentGraphNode::ExternalAssetReference(reference))]);
    };

    let Some(chunking_type) = *chunkable_asset_reference.chunking_type().await? else {
        return Ok(vec![(None, ChunkContentGraphNode::ExternalAssetReference(reference))]);
    };

    let result = reference.resolve_reference().await?;

    let assets = result.primary.iter().filter_map({
        |result| {
            if let PrimaryResolveResult::Asset(asset) = *result {
                return Some(asset);
            }
            None
        }
    });

    let mut graph_nodes = vec![];

    for asset in assets {
        if let Some(available_assets) = context.availability_info.available_assets() {
            if *available_assets.includes(asset).await? {
                graph_nodes.push((
                    Some((asset, chunking_type)),
                    ChunkContentGraphNode::AvailableAsset(asset),
                ));
                continue;
            }
        }

        let chunkable_asset =
            match Vc::try_resolve_sidecast::<Box<dyn ChunkableAsset>>(asset).await? {
                Some(chunkable_asset) => chunkable_asset,
                _ => {
                    return Ok(vec![(
                        None,
                        ChunkContentGraphNode::ExternalAssetReference(reference),
                    )]);
                }
            };

        match chunking_type {
            ChunkingType::Placed => {
                if let Some(chunk_item) = I::from_asset(context.chunking_context, asset).await? {
                    graph_nodes.push((
                        Some((asset, chunking_type)),
                        ChunkContentGraphNode::ChunkItem {
                            item: chunk_item,
                            ident: asset.ident().to_string().await?,
                        },
                    ));
                } else {
                    return Err(anyhow!(
                        "Asset {} was requested to be placed into the  same chunk, but this \
                         wasn't possible",
                        asset.ident().to_string().await?
                    ));
                }
            }
            ChunkingType::Parallel => {
                let chunk =
                    chunkable_asset.as_chunk(context.chunking_context, context.availability_info);
                graph_nodes.push((
                    Some((asset, chunking_type)),
                    ChunkContentGraphNode::Chunk(chunk),
                ));
            }
            ChunkingType::IsolatedParallel => {
                let chunk = chunkable_asset.as_root_chunk(context.chunking_context);
                graph_nodes.push((
                    Some((asset, chunking_type)),
                    ChunkContentGraphNode::Chunk(chunk),
                ));
            }
            ChunkingType::PlacedOrParallel => {
                // heuristic for being in the same chunk
                if !context.split
                    && *context
                        .chunking_context
                        .can_be_in_same_chunk(context.entry, asset)
                        .await?
                {
                    // chunk item, chunk or other asset?
                    if let Some(chunk_item) = I::from_asset(context.chunking_context, asset).await?
                    {
                        graph_nodes.push((
                            Some((asset, chunking_type)),
                            ChunkContentGraphNode::ChunkItem {
                                item: chunk_item,
                                ident: asset.ident().to_string().await?,
                            },
                        ));
                        continue;
                    }
                }

                let chunk =
                    chunkable_asset.as_chunk(context.chunking_context, context.availability_info);
                graph_nodes.push((
                    Some((asset, chunking_type)),
                    ChunkContentGraphNode::Chunk(chunk),
                ));
            }
            ChunkingType::Separate => {
                graph_nodes.push((
                    Some((asset, chunking_type)),
                    ChunkContentGraphNode::AsyncChunkGroup {
                        entry: chunkable_asset
                            .as_chunk(context.chunking_context, context.availability_info),
                    },
                ));
            }
            ChunkingType::SeparateAsync => {
                if let Some(manifest_loader_item) = I::from_async_asset(
                    context.chunking_context,
                    chunkable_asset,
                    context.availability_info,
                )
                .await?
                {
                    graph_nodes.push((
                        Some((asset, chunking_type)),
                        ChunkContentGraphNode::ChunkItem {
                            item: manifest_loader_item,
                            ident: asset.ident().to_string().await?,
                        },
                    ));
                } else {
                    return Ok(vec![(
                        None,
                        ChunkContentGraphNode::ExternalAssetReference(reference),
                    )]);
                }
            }
        }
    }

    Ok(graph_nodes)
}

/// The maximum number of chunk items that can be in a chunk before we split it
/// into multiple chunks.
const MAX_CHUNK_ITEMS_COUNT: usize = 5000;

struct ChunkContentVisit<I> {
    context: ChunkContentContext,
    chunk_items_count: usize,
    processed_assets: HashSet<(ChunkingType, Vc<Box<dyn Asset>>)>,
    _phantom: PhantomData<I>,
}

type ChunkItemToGraphNodesEdges<I> = impl Iterator<
    Item = (
        Option<(Vc<Box<dyn Asset>>, ChunkingType)>,
        ChunkContentGraphNode<Vc<I>>,
    ),
>;

type ChunkItemToGraphNodesFuture<I: FromChunkableAsset> =
    impl Future<Output = Result<ChunkItemToGraphNodesEdges<I>>>;

impl<I> Visit<ChunkContentGraphNode<Vc<I>>, ()> for ChunkContentVisit<Vc<I>>
where
    I: FromChunkableAsset,
{
    type Edge = (
        Option<(Vc<Box<dyn Asset>>, ChunkingType)>,
        ChunkContentGraphNode<Vc<I>>,
    );
    type EdgesIntoIter = ChunkItemToGraphNodesEdges<I>;
    type EdgesFuture = ChunkItemToGraphNodesFuture<I>;

    fn visit(
        &mut self,
        (option_key, node): (
            Option<(Vc<Box<dyn Asset>>, ChunkingType)>,
            ChunkContentGraphNode<Vc<I>>,
        ),
    ) -> VisitControlFlow<ChunkContentGraphNode<Vc<I>>, ()> {
        let Some((asset, chunking_type)) = option_key else {
            return VisitControlFlow::Continue(node);
        };

        if !self.processed_assets.insert((chunking_type, asset)) {
            return VisitControlFlow::Skip(node);
        }

        if let ChunkContentGraphNode::ChunkItem { .. } = &node {
            self.chunk_items_count += 1;

            // Make sure the chunk doesn't become too large.
            // This will hurt performance in many aspects.
            if !self.context.split && self.chunk_items_count >= MAX_CHUNK_ITEMS_COUNT {
                // Chunk is too large, cancel this algorithm and restart with splitting from the
                // start.
                return VisitControlFlow::Abort(());
            }
        }

        VisitControlFlow::Continue(node)
    }

    fn edges(&mut self, node: &ChunkContentGraphNode<Vc<I>>) -> Self::EdgesFuture {
        let chunk_item = if let ChunkContentGraphNode::ChunkItem {
            item: chunk_item, ..
        } = node
        {
            Some(*chunk_item)
        } else {
            None
        };

        let context = self.context;

        async move {
            let Some(chunk_item) = chunk_item else {
                return Ok(vec![].into_iter().flatten());
            };

            let references = chunk_item.references().await?;

            Ok(references
                .into_iter()
                .map(|reference| reference_to_graph_nodes::<I>(context, *reference))
                .try_join()
                .await?
                .into_iter()
                .flatten())
        }
    }

    fn span(&mut self, node: &ChunkContentGraphNode<Vc<I>>) -> Span {
        if let ChunkContentGraphNode::ChunkItem { ident, .. } = node {
            info_span!("module", name = display(ident))
        } else {
            Span::current()
        }
    }
}

async fn chunk_content_internal_parallel<I>(
    chunking_context: Vc<Box<dyn ChunkingContext>>,
    entry: Vc<Box<dyn Asset>>,
    additional_entries: Option<Vc<Assets>>,
    availability_info: Value<AvailabilityInfo>,
    split: bool,
) -> Result<Option<ChunkContentResult<Vc<I>>>>
where
    I: FromChunkableAsset,
{
    let additional_entries = if let Some(additional_entries) = additional_entries {
        additional_entries.await?.clone_value().into_iter()
    } else {
        vec![].into_iter()
    };

    let root_edges = [entry]
        .into_iter()
        .chain(additional_entries)
        .map(|entry| async move {
            Ok((
                Some((entry, ChunkingType::Placed)),
                ChunkContentGraphNode::ChunkItem {
                    item: I::from_asset(chunking_context, entry).await?.unwrap(),
                    ident: entry.ident().to_string().await?,
                },
            ))
        })
        .try_join()
        .await?;

    let context = ChunkContentContext {
        chunking_context,
        entry,
        split,
        availability_info,
    };

    let visit = ChunkContentVisit {
        context,
        chunk_items_count: 0,
        processed_assets: Default::default(),
        _phantom: PhantomData,
    };

    let GraphTraversalResult::Completed(traversal_result) = ReverseTopological::new().visit(root_edges, visit).await else {
        return Ok(None);
    };

    let graph_nodes: Vec<_> = traversal_result?.into_iter().collect();

    let mut chunk_items = Vec::new();
    let mut chunks = Vec::new();
    let mut async_chunk_group_entries = Vec::new();
    let mut external_asset_references = Vec::new();

    for graph_node in graph_nodes {
        match graph_node {
            ChunkContentGraphNode::AvailableAsset(_asset) => {}
            ChunkContentGraphNode::ChunkItem { item, .. } => {
                chunk_items.push(item);
            }
            ChunkContentGraphNode::Chunk(chunk) => {
                chunks.push(chunk);
            }
            ChunkContentGraphNode::AsyncChunkGroup { entry } => {
                async_chunk_group_entries.push(entry);
            }
            ChunkContentGraphNode::ExternalAssetReference(reference) => {
                external_asset_references.push(reference);
            }
        }
    }

    Ok(Some(ChunkContentResult {
        chunk_items,
        chunks,
        async_chunk_group_entries,
        external_asset_references,
        availability_info: availability_info.into_value(),
    }))
}

#[turbo_tasks::value_trait]
pub trait ChunkItem {
    /// The [AssetIdent] of the [Asset] that this [ChunkItem] was created from.
    /// For most chunk types this must uniquely identify the asset as it's the
    /// source of the module id used at runtime.
    fn asset_ident(self: Vc<Self>) -> Vc<AssetIdent>;
    /// A [ChunkItem] can describe different `references` than its original
    /// [Asset].
    /// TODO(alexkirsz) This should have a default impl that returns empty
    /// references.
    fn references(self: Vc<Self>) -> Vc<AssetReferences>;
}

#[turbo_tasks::value(transparent)]
pub struct ChunkItems(Vec<Vc<Box<dyn ChunkItem>>>);

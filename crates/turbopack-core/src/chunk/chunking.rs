use std::mem::take;

use anyhow::{bail, Result};
use indexmap::{IndexMap, IndexSet};
use turbo_tasks::{
    graph::{AdjacencyMap, GraphTraversal},
    primitives::StringVc,
    util::FormatIter,
    TryJoinIterExt, Value, ValueToString, ValueToStringVc,
};

use super::{
    availability_info::AvailabilityInfo,
    available_assets::{AvailableAssets, AvailableAssetsVc},
    Chunk, ChunkIdentVc, ChunkItemVc, ChunkItemsVc, ChunkVc, ChunkableAssetReference,
    ChunkableAssetReferenceVc, ChunkingContextVc, ChunksVc,
};
use crate::{
    asset::{Asset, AssetVc},
    chunk::{ChunkItem, ChunkType, ChunkableAsset, ChunkableAssetVc, ChunkingType},
    ident::AssetIdentVc,
    reference::{AssetReference, AssetReferenceVc},
    resolve::{ResolveResult, ResolveResultVc},
};

#[turbo_tasks::value]
struct ChunkingResult {
    chunk_items: Vec<ChunkItemVc>,
    available_assets: AvailableAssetsVc,
    isolated_parallel_chunk_groups: Vec<ChunkableAssetVc>,
    external_references: Vec<AssetReferenceVc>,
    async_assets: Vec<ChunkableAssetVc>,
}

#[turbo_tasks::function]
async fn chunking(
    entries: Vec<ChunkableAssetVc>,
    chunking_context: ChunkingContextVc,
    availability_info: Value<AvailabilityInfo>,
) -> Result<ChunkingResultVc> {
    #[derive(Clone, PartialEq, Eq, Hash)]
    enum ResultItem {
        ChunkItem(ChunkItemVc, AssetVc),
        External(AssetReferenceVc),
        IsolatedParallel(ChunkableAssetVc),
        Async(ChunkableAssetVc),
    }
    let roots = entries.iter().map(|&asset| {
        ResultItem::ChunkItem(
            asset.as_chunk_item(chunking_context, availability_info),
            asset.into(),
        )
    });
    let results = AdjacencyMap::new()
        .skip_duplicates()
        .visit(roots, |result: &ResultItem| {
          let chunk_item = if let &ResultItem::ChunkItem(chunk_item, _) = result {
            Some(chunk_item)
          } else {
            None
          };
          async move {
              let Some(chunk_item) = chunk_item else {
                  return Ok(Vec::new());
              };
              let mut results = Vec::new();
              for &reference in chunk_item.references().await?.iter() {
                  if let Some(chunkable) = ChunkableAssetReferenceVc::resolve_from(reference).await? {
                      match &*chunkable.chunking_type().await? {
                          None => results.push(ResultItem::External(reference)),
                          Some(
                              ChunkingType::Parallel
                              | ChunkingType::PlacedOrParallel
                              | ChunkingType::Placed,
                          ) => {
                              for &asset in &*chunkable.resolve_reference().primary_assets().await? {
                                  let Some(chunkable) = ChunkableAssetVc::resolve_from(asset).await? else {
                                      bail!(
                                          "asset {} must be a ChunkableAsset when it's referenced from a ChunkableAssetReference",
                                          asset.ident().to_string().await?
                                      );
                                  };
                                  results.push(ResultItem::ChunkItem(
                                      chunkable.as_chunk_item(chunking_context, availability_info),
                                      chunkable.into(),
                                  ));
                              }
                          }
                          Some(ChunkingType::IsolatedParallel) => {
                              for &asset in &*chunkable.resolve_reference().primary_assets().await? {
                                  let Some(chunkable) = ChunkableAssetVc::resolve_from(asset).await? else {
                                      bail!(
                                          "asset {} must be a ChunkableAsset when it's referenced from a ChunkableAssetReference",
                                          asset.ident().to_string().await?
                                      );
                                  };
                                  results.push(ResultItem::IsolatedParallel(chunkable));
                              }
                          }
                          Some(ChunkingType::Async) => {
                              for &asset in &*chunkable.resolve_reference().primary_assets().await? {
                                  let Some(chunkable) = ChunkableAssetVc::resolve_from(asset).await? else {
                                      bail!(
                                          "asset {} must be a ChunkableAsset when it's referenced from a ChunkableAssetReference",
                                          asset.ident().to_string().await?
                                      );
                                  };
                                  results.push(ResultItem::Async(chunkable));
                              }
                          }
                      }
                  } else {
                      results.push(ResultItem::External(reference));
                  }
              }
              Ok(results)
          }
      })
        .await
        .completed()?;

    let mut chunk_items = Vec::new();
    let mut isolated_parallel_chunk_groups = Vec::new();
    let mut external_references = Vec::new();
    let mut async_assets = Vec::new();
    let mut available_assets = IndexSet::new();
    for item in results.into_inner().into_iter() {
        match item {
            ResultItem::ChunkItem(chunk_item, asset) => {
                chunk_items.push(chunk_item);
                available_assets.insert(asset);
            }
            ResultItem::External(reference) => {
                external_references.push(reference);
            }
            ResultItem::IsolatedParallel(asset) => {
                isolated_parallel_chunk_groups.push(asset);
            }
            ResultItem::Async(asset) => {
                async_assets.push(asset);
            }
        }
    }
    Ok(ChunkingResult {
        chunk_items,
        available_assets: AvailableAssets {
            parent: availability_info.available_assets(),
            assets: available_assets,
        }
        .cell(),
        isolated_parallel_chunk_groups,
        external_references,
        async_assets,
    }
    .cell())
}

const NUMBER_OF_CHUNKS_PER_CHUNK_GROUP: usize = 6;

/// Computes the chunks for a chunk group defined by a list of entries in a
/// specific context and with some availability info. The returned chunks are
/// optimized based on the optimization ability of the `context`.
#[turbo_tasks::function]
async fn chunk_group(
    entries: Vec<ChunkableAssetVc>,
    chunking_context: ChunkingContextVc,
    availability_info: Value<AvailabilityInfo>,
) -> Result<ChunksVc> {
    // Capture all chunk items and other things from the module graph
    let chunking_result = chunking(entries, chunking_context, availability_info).await?;

    // Get innner availablity info
    let inner_availability_info = AvailabilityInfo::Inner {
        available_assets: chunking_result.available_assets,
    };

    // Additional references from the main chunk
    let mut inner_references = Vec::new();

    // Async chunk groups
    for &async_chunk_group in chunking_result.async_assets.iter() {
        inner_references.push(
            AsyncChunkGroupReferenceVc::new(
                vec![async_chunk_group],
                chunking_context,
                Value::new(inner_availability_info),
            )
            .into(),
        );
    }

    // External references
    for &external_reference in chunking_result.external_references.iter() {
        inner_references.push(external_reference);
    }

    // Place chunk items in chunks in a smart way
    let mut chunks: Vec<ChunkVc> = make_chunks(
        &chunking_result.chunk_items,
        chunking_context,
        inner_references,
    )
    .await?;

    // merge parallel chunk groups
    for chunk_group in chunking_result
        .isolated_parallel_chunk_groups
        .iter()
        .map(|&asset| {
            chunk_group(
                vec![asset],
                chunking_context,
                Value::new(AvailabilityInfo::Root),
            )
        })
        .try_join()
        .await?
    {
        chunks.extend(chunk_group.iter().copied())
    }

    // return chunks
    Ok(ChunksVc::cell(chunks))
}

async fn make_chunks(
    entry_ident: AssetIdentVc,
    chunk_items: &[ChunkItemVc],
    chunking_context: ChunkingContextVc,
    mut main_references: Vec<AssetReferenceVc>,
) -> Result<Vec<ChunkVc>> {
    // Sort chunk items by chunk type
    let mut chunk_items_by_type = IndexMap::new();
    for &chunk_item in chunk_items {
        let chunk_type = chunk_item.chunk_type().resolve().await?;
        // TODO ask the chunking_context for further key of splitting
        chunk_items_by_type
            .entry(chunk_type)
            .or_insert_with(Vec::new)
            .push(chunk_item);
    }

    // Make chunks
    let chunks = chunk_items_by_type
        .into_iter()
        .map(|(ty, items)| {
            // This cell call would benefit from keyed_cell
            let items = ChunkItemsVc::cell(items);
            let ident = ChunkIdentVc::new(entry_ident, ty, "");
            Chunk {
                ty,
                chunking_context,
                ident,
                items,
                references: take(&mut main_references),
            }
            .cell()
        })
        .collect();

    Ok(chunks)
}

/// A reference to multiple chunks from a [ChunkGroup]
#[turbo_tasks::value]
pub struct AsyncChunkGroupReference {
    entries: Vec<ChunkableAssetVc>,
    chunking_context: ChunkingContextVc,
    availability_info: AvailabilityInfo,
}

#[turbo_tasks::value_impl]
impl AsyncChunkGroupReferenceVc {
    #[turbo_tasks::function]
    pub fn new(
        entries: Vec<ChunkableAssetVc>,
        chunking_context: ChunkingContextVc,
        availability_info: Value<AvailabilityInfo>,
    ) -> Self {
        Self::cell(AsyncChunkGroupReference {
            entries,
            chunking_context,
            availability_info: availability_info.into_value(),
        })
    }

    #[turbo_tasks::function]
    async fn chunk_group(self) -> Result<ChunksVc> {
        let this = self.await?;
        Ok(chunk_group(
            this.entries.clone(),
            this.chunking_context,
            Value::new(this.availability_info),
        ))
    }
}

#[turbo_tasks::value_impl]
impl AssetReference for AsyncChunkGroupReference {
    #[turbo_tasks::function]
    async fn resolve_reference(self_vc: AsyncChunkGroupReferenceVc) -> Result<ResolveResultVc> {
        let set = self_vc
            .chunk_group()
            .await?
            .iter()
            .map(|&c| c.into())
            .collect();
        Ok(ResolveResult::assets(set).into())
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for AsyncChunkGroupReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<StringVc> {
        let idents = self
            .entries
            .iter()
            .map(|a| a.ident().to_string())
            .try_join()
            .await?;
        Ok(StringVc::cell(format!(
            "chunk group ({})",
            FormatIter(|| idents.iter().map(|s| s.as_str()).intersperse(", "))
        )))
    }
}

use std::{iter::once, marker::PhantomData};

use anyhow::Result;
use turbo_tasks::{
    graph::{GraphTraversal, ReverseTopological},
    TryJoinIterExt, ValueToString, Vc,
};
use turbo_tasks_hash::Xxh3Hash64Hasher;

use super::{ChunkableAssetReference, ChunkingType};
use crate::{
    asset::{Asset, AssetsSet},
    reference::AssetReference,
};

/// Allows to gather information about which assets are already available.
/// Adding more roots will form a linked list like structure to allow caching
/// `include` queries.
#[turbo_tasks::value]
pub struct AvailableAssets {
    parent: Option<Vc<AvailableAssets>>,
    roots: Vec<Vc<Box<dyn Asset>>>,
}

#[turbo_tasks::value_impl]
impl AvailableAssets {
    #[turbo_tasks::function]
    fn new_normalized(
        parent: Option<Vc<AvailableAssets>>,
        roots: Vec<Vc<Box<dyn Asset>>>,
    ) -> Vc<Self> {
        AvailableAssets { parent, roots }.cell()
    }

    #[turbo_tasks::function]
    pub fn new(roots: Vec<Vc<Box<dyn Asset>>>) -> Vc<Self> {
        Self::new_normalized(None, roots)
    }

    #[turbo_tasks::function]
    pub async fn with_roots(self: Vc<Self>, roots: Vec<Vc<Box<dyn Asset>>>) -> Result<Vc<Self>> {
        let roots = roots
            .into_iter()
            .map(|root| async move { Ok((self.includes(root).await?, root)) })
            .try_join()
            .await?
            .into_iter()
            .filter_map(|(included, root)| (!*included).then_some(root))
            .collect();
        Ok(Self::new_normalized(Some(self), roots))
    }

    #[turbo_tasks::function]
    pub async fn hash(self: Vc<Self>) -> Result<Vc<u64>> {
        let this = self.await?;
        let mut hasher = Xxh3Hash64Hasher::new();
        if let Some(parent) = this.parent {
            hasher.write_value(parent.hash().await?);
        } else {
            hasher.write_value(0u64);
        }
        for root in &this.roots {
            hasher.write_value(root.ident().to_string().await?);
        }
        Ok(Vc::cell(hasher.finish()))
    }

    #[turbo_tasks::function]
    pub async fn includes(self: Vc<Self>, asset: Vc<Box<dyn Asset>>) -> Result<Vc<bool>> {
        let this = self.await?;
        if let Some(parent) = this.parent {
            if *parent.includes(asset).await? {
                return Ok(Vc::cell(true));
            }
        }
        for root in this.roots.iter() {
            if chunkable_assets_set(*root).await?.contains(&asset) {
                return Ok(Vc::cell(true));
            }
        }
        Ok(Vc::cell(false))
    }
}

trait What {}

struct OkWhat;

impl What for OkWhat {}

struct Ptr<T>
where
    T: ?Sized,
{
    _t: PhantomData<T>,
}

unsafe impl<T> Send for Ptr<T> where T: ?Sized {}
unsafe impl<T> Sync for Ptr<T> where T: ?Sized {}

#[turbo_tasks::function]
async fn chunkable_assets_set(root: Vc<Box<dyn Asset>>) -> Result<Vc<AssetsSet>> {
    let assets = ReverseTopological::new()
        .skip_duplicates()
        .visit(once(root), |&asset: &Vc<Box<dyn Asset>>| async move {
            let mut results = Vec::new();
            for reference in asset.references().await?.iter() {
                if let Some(chunkable) =
                    Vc::try_resolve_downcast::<Box<dyn ChunkableAssetReference>>(*reference).await?
                {
                    if matches!(
                        &*chunkable.chunking_type().await?,
                        Some(
                            ChunkingType::Parallel
                                | ChunkingType::PlacedOrParallel
                                | ChunkingType::Placed
                        )
                    ) {
                        results.extend(
                            chunkable
                                .resolve_reference()
                                .primary_assets()
                                .await?
                                .iter()
                                .copied(),
                        );
                    }
                }
            }
            Ok(results)
        })
        .await
        .completed()?;
    Ok(Vc::cell(assets.into_inner().into_iter().collect()))
}

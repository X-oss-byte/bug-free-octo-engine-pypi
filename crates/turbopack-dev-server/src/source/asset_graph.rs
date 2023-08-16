use std::{
    collections::{HashMap, HashSet, VecDeque},
    iter::once,
};

use anyhow::Result;
use indexmap::{indexset, IndexSet};
use turbo_tasks::{State, Value, ValueToString, Vc};
use turbo_tasks_fs::FileSystemPath;
use turbopack_core::{
    asset::{Asset, AssetsSet},
    introspect::{asset::IntrospectableAsset, Introspectable, IntrospectableChildren},
    reference::all_referenced_assets,
};

use super::{ContentSource, ContentSourceContent, ContentSourceData, ContentSourceResult};

#[turbo_tasks::value(transparent)]
struct AssetsMap(HashMap<String, Vc<Box<dyn Asset>>>);

type ExpandedState = State<HashSet<Vc<Box<dyn Asset>>>>;

#[turbo_tasks::value(serialization = "none", eq = "manual", cell = "new")]
pub struct AssetGraphContentSource {
    root_path: Vc<FileSystemPath>,
    root_assets: Vc<AssetsSet>,
    expanded: Option<ExpandedState>,
}

#[turbo_tasks::value_impl]
impl AssetGraphContentSource {
    /// Serves all assets references by root_asset.
    #[turbo_tasks::function]
    pub fn new_eager(root_path: Vc<FileSystemPath>, root_asset: Vc<Box<dyn Asset>>) -> Vc<Self> {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets: Vc::cell(indexset! { root_asset }),
            expanded: None,
        })
    }

    /// Serves all assets references by root_asset. Only serve references of an
    /// asset when it has served its content before.
    #[turbo_tasks::function]
    pub fn new_lazy(root_path: Vc<FileSystemPath>, root_asset: Vc<Box<dyn Asset>>) -> Vc<Self> {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets: Vc::cell(indexset! { root_asset }),
            expanded: Some(State::new(HashSet::new())),
        })
    }

    /// Serves all assets references by all root_assets.
    #[turbo_tasks::function]
    pub fn new_eager_multiple(
        root_path: Vc<FileSystemPath>,
        root_assets: Vc<AssetsSet>,
    ) -> Vc<Self> {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets,
            expanded: None,
        })
    }

    /// Serves all assets references by all root_assets. Only serve references
    /// of an asset when it has served its content before.
    #[turbo_tasks::function]
    pub fn new_lazy_multiple(
        root_path: Vc<FileSystemPath>,
        root_assets: Vc<AssetsSet>,
    ) -> Vc<Self> {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets,
            expanded: Some(State::new(HashSet::new())),
        })
    }

    #[turbo_tasks::function]
    async fn all_assets_map(self: Vc<Self>) -> Result<Vc<AssetsMap>> {
        let this = self.await?;
        Ok(Vc::cell(
            expand(
                &*this.root_assets.await?,
                &*this.root_path.await?,
                this.expanded.as_ref(),
            )
            .await?,
        ))
    }
}

async fn expand(
    root_assets: &IndexSet<Vc<Box<dyn Asset>>>,
    root_path: &FileSystemPath,
    expanded: Option<&ExpandedState>,
) -> Result<HashMap<String, Vc<Box<dyn Asset>>>> {
    let mut map = HashMap::new();
    let mut assets = Vec::new();
    let mut queue = VecDeque::with_capacity(32);
    let mut assets_set = HashSet::new();
    if let Some(expanded) = &expanded {
        let expanded = expanded.get();
        for root_asset in root_assets.iter() {
            let expanded = expanded.contains(root_asset);
            assets.push((root_asset.ident().path(), *root_asset));
            assets_set.insert(*root_asset);
            if expanded {
                queue.push_back(all_referenced_assets(*root_asset));
            }
        }
    } else {
        for root_asset in root_assets.iter() {
            assets.push((root_asset.ident().path(), *root_asset));
            assets_set.insert(*root_asset);
            queue.push_back(all_referenced_assets(*root_asset));
        }
    }

    while let Some(references) = queue.pop_front() {
        for asset in references.await?.iter() {
            if assets_set.insert(*asset) {
                let expanded = if let Some(expanded) = &expanded {
                    expanded.get().contains(asset)
                } else {
                    true
                };
                if expanded {
                    queue.push_back(all_referenced_assets(*asset));
                }
                assets.push((asset.ident().path(), *asset));
            }
        }
    }
    for (p_vc, asset) in assets {
        // For clippy -- This explicit deref is necessary
        let p = &*p_vc.await?;
        if let Some(sub_path) = root_path.get_path_to(p) {
            map.insert(sub_path.to_string(), asset);
            if sub_path == "index.html" {
                map.insert("".to_string(), asset);
            } else if let Some(p) = sub_path.strip_suffix("/index.html") {
                map.insert(p.to_string(), asset);
                map.insert(format!("{p}/"), asset);
            } else if let Some(p) = sub_path.strip_suffix(".html") {
                map.insert(p.to_string(), asset);
            }
        }
    }
    Ok(map)
}

#[turbo_tasks::value_impl]
impl ContentSource for AssetGraphContentSource {
    #[turbo_tasks::function]
    async fn get(
        self: Vc<Self>,
        path: String,
        _data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        let assets = self.all_assets_map().strongly_consistent().await?;

        if let Some(asset) = assets.get(&path) {
            {
                let this = self.await?;
                if let Some(expanded) = &this.expanded {
                    expanded.update_conditionally(|expanded| expanded.insert(*asset));
                }
            }
            return Ok(ContentSourceResult::exact(Vc::upcast(
                ContentSourceContent::static_content(asset.versioned_content()),
            )));
        }
        Ok(ContentSourceResult::not_found())
    }
}

#[turbo_tasks::function]
fn introspectable_type() -> Vc<String> {
    Vc::cell("asset graph content source".to_string())
}

#[turbo_tasks::value_impl]
impl Introspectable for AssetGraphContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        introspectable_type()
    }

    #[turbo_tasks::function]
    fn title(&self) -> Vc<String> {
        self.root_path.to_string()
    }

    #[turbo_tasks::function]
    async fn children(self: Vc<Self>) -> Result<Vc<IntrospectableChildren>> {
        let this = self.await?;
        let key = Vc::cell("root".to_string());
        let expanded_key = Vc::cell("expanded".to_string());

        let root_assets = this.root_assets.await?;
        let root_assets = root_assets
            .iter()
            .map(|&asset| (key, IntrospectableAsset::new(asset)));

        Ok(Vc::cell(
            root_assets
                .chain(once((expanded_key, Vc::upcast(FullyExpaned(self).cell()))))
                .collect(),
        ))
    }
}

#[turbo_tasks::function]
fn fully_expaned_introspectable_type() -> Vc<String> {
    Vc::cell("fully expanded asset graph content source".to_string())
}

#[turbo_tasks::value]
struct FullyExpaned(Vc<AssetGraphContentSource>);

#[turbo_tasks::value_impl]
impl Introspectable for FullyExpaned {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        fully_expaned_introspectable_type()
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<Vc<String>> {
        Ok(self.0.await?.root_path.to_string())
    }

    #[turbo_tasks::function]
    async fn children(&self) -> Result<Vc<IntrospectableChildren>> {
        let source = self.0.await?;
        let key = Vc::cell("asset".to_string());

        let expanded_assets =
            expand(&*source.root_assets.await?, &*source.root_path.await?, None).await?;
        let children = expanded_assets
            .iter()
            .map(|(_k, &v)| (key, IntrospectableAsset::new(v)))
            .collect();

        Ok(Vc::cell(children))
    }
}

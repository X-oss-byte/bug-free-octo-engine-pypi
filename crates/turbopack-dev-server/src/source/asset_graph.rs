use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::Result;
use indexmap::indexset;
use turbo_tasks::{primitives::StringVc, State, Value, ValueToString};
use turbo_tasks_fs::FileSystemPathVc;
use turbopack_core::{
    asset::{Asset, AssetVc, AssetsSetVc},
    introspect::{
        asset::IntrospectableAssetVc, Introspectable, IntrospectableChildrenVc, IntrospectableVc,
    },
    reference::all_referenced_assets,
};

use super::{
    ContentSource, ContentSourceContentVc, ContentSourceData, ContentSourceResultVc,
    ContentSourceVc,
};

#[turbo_tasks::value(transparent)]
struct AssetsMap(HashMap<String, AssetVc>);

#[turbo_tasks::value(serialization = "none", eq = "manual", cell = "new")]
pub struct AssetGraphContentSource {
    root_path: FileSystemPathVc,
    root_assets: AssetsSetVc,
    expanded: Option<State<HashSet<AssetVc>>>,
}

#[turbo_tasks::value_impl]
impl AssetGraphContentSourceVc {
    /// Serves all assets references by root_asset.
    #[turbo_tasks::function]
    pub fn new_eager(root_path: FileSystemPathVc, root_asset: AssetVc) -> Self {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets: AssetsSetVc::cell(indexset! { root_asset }),
            expanded: None,
        })
    }

    /// Serves all assets references by root_asset. Only serve references of an
    /// asset when it has served its content before.
    #[turbo_tasks::function]
    pub fn new_lazy(root_path: FileSystemPathVc, root_asset: AssetVc) -> Self {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets: AssetsSetVc::cell(indexset! { root_asset }),
            expanded: Some(State::new(HashSet::new())),
        })
    }

    /// Serves all assets references by all root_assets.
    #[turbo_tasks::function]
    pub fn new_eager_multiple(root_path: FileSystemPathVc, root_assets: AssetsSetVc) -> Self {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets,
            expanded: None,
        })
    }

    /// Serves all assets references by all root_assets. Only serve references
    /// of an asset when it has served its content before.
    #[turbo_tasks::function]
    pub fn new_lazy_multiple(root_path: FileSystemPathVc, root_assets: AssetsSetVc) -> Self {
        Self::cell(AssetGraphContentSource {
            root_path,
            root_assets,
            expanded: Some(State::new(HashSet::new())),
        })
    }

    #[turbo_tasks::function]
    async fn all_assets_map(self) -> Result<AssetsMapVc> {
        let this = self.await?;
        let mut map = HashMap::new();
        let root_path = this.root_path.await?;
        let mut assets = Vec::new();
        let mut queue = VecDeque::with_capacity(32);
        let mut assets_set = HashSet::new();
        let root_assets = this.root_assets.await?;
        if let Some(expanded) = &this.expanded {
            let expanded = expanded.get();
            for root_asset in root_assets.iter() {
                let expanded = expanded.contains(root_asset);
                assets.push((root_asset.path(), *root_asset));
                assets_set.insert(*root_asset);
                if expanded {
                    queue.push_back(all_referenced_assets(*root_asset));
                }
            }
        } else {
            for root_asset in root_assets.iter() {
                assets.push((root_asset.path(), *root_asset));
                assets_set.insert(*root_asset);
                queue.push_back(all_referenced_assets(*root_asset));
            }
        }

        while let Some(references) = queue.pop_front() {
            for asset in references.await?.iter() {
                if assets_set.insert(*asset) {
                    let expanded = if let Some(expanded) = &this.expanded {
                        expanded.get().contains(asset)
                    } else {
                        true
                    };
                    if expanded {
                        queue.push_back(all_referenced_assets(*asset));
                    }
                    assets.push((asset.path(), *asset));
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
        Ok(AssetsMapVc::cell(map))
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for AssetGraphContentSource {
    #[turbo_tasks::function]
    async fn get(
        self_vc: AssetGraphContentSourceVc,
        path: &str,
        _data: Value<ContentSourceData>,
    ) -> Result<ContentSourceResultVc> {
        let assets = self_vc.all_assets_map().strongly_consistent().await?;

        // Remove leading slash.
        let path = &path[1..];
        if let Some(asset) = assets.get(path) {
            {
                let this = self_vc.await?;
                if let Some(expanded) = &this.expanded {
                    expanded.update_conditionally(|expanded| expanded.insert(*asset));
                }
            }
            return Ok(ContentSourceResultVc::exact(
                ContentSourceContentVc::static_content(asset.versioned_content()).into(),
            ));
        }
        Ok(ContentSourceResultVc::not_found())
    }
}

#[turbo_tasks::function]
fn introspectable_type() -> StringVc {
    StringVc::cell("asset graph content source".to_string())
}

#[turbo_tasks::value_impl]
impl Introspectable for AssetGraphContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> StringVc {
        introspectable_type()
    }

    #[turbo_tasks::function]
    fn title(&self) -> StringVc {
        self.root_path.to_string()
    }

    #[turbo_tasks::function]
    async fn children(&self) -> Result<IntrospectableChildrenVc> {
        let key = StringVc::cell("root".to_string());
        Ok(IntrospectableChildrenVc::cell(
            self.root_assets
                .await?
                .iter()
                .map(|&asset| (key, IntrospectableAssetVc::new(asset)))
                .collect(),
        ))
    }
}

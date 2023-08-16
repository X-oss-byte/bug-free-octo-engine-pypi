use std::fmt::Write;

use anyhow::Result;
use indexmap::IndexSet;
use turbo_tasks::{
    graph::{GraphTraversal, ReverseTopological, SkipDuplicates},
    primitives::{BoolVc, StringVc},
    TryJoinIterExt, Value, ValueToString,
};
use turbo_tasks_fs::FileSystemPathVc;
use turbo_tasks_hash::{encode_hex, hash_xxh3_hash64, DeterministicHash, Xxh3Hash64Hasher};
use turbopack_core::{
    asset::{Asset, AssetVc, AssetsVc},
    chunk::{
        availability_info::AvailabilityInfo, optimize, ChunkVc, ChunkableAsset, ChunkableAssetVc,
        ChunkingContext, ChunkingContextVc, ChunksVc, EvaluatableAssetsVc, ParallelChunkReference,
        ParallelChunkReferenceVc,
    },
    environment::EnvironmentVc,
    ident::{AssetIdent, AssetIdentVc},
    reference::{AssetReference, AssetReferenceVc},
    resolve::{ModulePart, PrimaryResolveResult},
};
use turbopack_ecmascript::{
    chunk::{
        EcmascriptChunkItemVc, EcmascriptChunkVc, EcmascriptChunkingContext,
        EcmascriptChunkingContextVc,
    },
    EcmascriptModuleAssetVc,
};

use crate::ecmascript::node::{
    chunk::EcmascriptBuildNodeChunkVc,
    evaluate::chunk::EcmascriptBuildNodeEvaluateChunkVc,
    manifest::{chunk_asset::BuildManifestChunkAssetVc, loader_item::BuildManifestLoaderItemVc},
};

pub struct BuildChunkingContextBuilder {
    context: BuildChunkingContext,
}

impl BuildChunkingContextBuilder {
    pub fn layer(mut self, layer: &str) -> Self {
        self.context.layer = (!layer.is_empty()).then(|| layer.to_string());
        self
    }

    pub fn css_chunk_root_path(mut self, path: FileSystemPathVc) -> Self {
        self.context.css_chunk_root_path = Some(path);
        self
    }

    pub fn build(self) -> BuildChunkingContextVc {
        BuildChunkingContextVc::new(Value::new(self.context))
    }
}

/// A chunking context for build mode.
#[turbo_tasks::value(serialization = "auto_for_input")]
#[derive(Debug, Clone, Hash, PartialOrd, Ord)]
pub struct BuildChunkingContext {
    /// This path get striped off of path before creating a name out of it
    context_path: FileSystemPathVc,
    /// This path is used to compute the url to request chunks or assets from
    output_root: FileSystemPathVc,
    /// Chunks are placed at this path
    chunk_root_path: FileSystemPathVc,
    /// Css Chunks are placed at this path
    css_chunk_root_path: Option<FileSystemPathVc>,
    /// Static assets are placed at this path
    asset_root_path: FileSystemPathVc,
    /// Layer name within this context
    layer: Option<String>,
    /// The environment chunks will be evaluated in.
    environment: EnvironmentVc,
}

impl BuildChunkingContextVc {
    pub fn builder(
        context_path: FileSystemPathVc,
        output_root: FileSystemPathVc,
        chunk_root_path: FileSystemPathVc,
        asset_root_path: FileSystemPathVc,
        environment: EnvironmentVc,
    ) -> BuildChunkingContextBuilder {
        BuildChunkingContextBuilder {
            context: BuildChunkingContext {
                context_path,
                output_root,
                chunk_root_path,
                css_chunk_root_path: None,
                asset_root_path,
                layer: None,
                environment,
            },
        }
    }
}

#[turbo_tasks::value_impl]
impl BuildChunkingContextVc {
    #[turbo_tasks::function]
    fn new(this: Value<BuildChunkingContext>) -> Self {
        this.into_value().cell()
    }

    #[turbo_tasks::function]
    pub(crate) async fn generate_chunk(
        self_vc: BuildChunkingContextVc,
        chunk: ChunkVc,
    ) -> Result<AssetVc> {
        Ok(
            if let Some(ecmascript_chunk) = EcmascriptChunkVc::resolve_from(chunk).await? {
                EcmascriptBuildNodeChunkVc::new(self_vc, ecmascript_chunk).into()
            } else {
                chunk.into()
            },
        )
    }

    #[turbo_tasks::function]
    fn generate_evaluate_chunk(
        self_vc: BuildChunkingContextVc,
        entry_chunk: ChunkVc,
        other_chunks: AssetsVc,
        evaluatable_assets: EvaluatableAssetsVc,
        exported_module: Option<EcmascriptModuleAssetVc>,
    ) -> AssetVc {
        EcmascriptBuildNodeEvaluateChunkVc::new(
            self_vc,
            entry_chunk,
            other_chunks,
            evaluatable_assets,
            exported_module,
        )
        .into()
    }

    #[turbo_tasks::function]
    pub async fn generate_exported_chunk(
        self_vc: BuildChunkingContextVc,
        module: EcmascriptModuleAssetVc,
        evaluatable_assets: EvaluatableAssetsVc,
    ) -> Result<AssetVc> {
        let entry_chunk = module.as_root_chunk(self_vc.into());

        let assets = self_vc
            .get_evaluate_chunk_assets(entry_chunk, evaluatable_assets)
            .await?;

        let asset = self_vc.generate_evaluate_chunk(
            entry_chunk,
            AssetsVc::cell(assets.clone()),
            evaluatable_assets,
            Some(module),
        );

        Ok(asset)
    }
}

impl BuildChunkingContextVc {
    async fn get_evaluate_chunk_assets(
        self,
        entry_chunk: ChunkVc,
        evaluatable_assets: EvaluatableAssetsVc,
    ) -> Result<Vec<AssetVc>> {
        let evaluatable_assets_ref = evaluatable_assets.await?;

        let mut entry_assets: IndexSet<_> = evaluatable_assets_ref
            .iter()
            .map({
                move |evaluatable_asset| async move {
                    Ok(evaluatable_asset
                        .as_root_chunk(self.into())
                        .resolve()
                        .await?)
                }
            })
            .try_join()
            .await?
            .into_iter()
            .collect();

        entry_assets.insert(entry_chunk.resolve().await?);

        let chunks = get_optimized_parallel_chunks(entry_assets).await?;

        Ok(chunks
            .await?
            .iter()
            .map(|chunk| self.generate_chunk(*chunk))
            .collect())
    }
}

#[turbo_tasks::value_impl]
impl ChunkingContext for BuildChunkingContext {
    #[turbo_tasks::function]
    fn context_path(&self) -> FileSystemPathVc {
        self.context_path
    }

    #[turbo_tasks::function]
    fn output_root(&self) -> FileSystemPathVc {
        self.output_root
    }

    #[turbo_tasks::function]
    fn environment(&self) -> EnvironmentVc {
        self.environment
    }

    #[turbo_tasks::function]
    async fn chunk_path(&self, ident: AssetIdentVc, extension: &str) -> Result<FileSystemPathVc> {
        fn clean_separators(s: &str) -> String {
            s.replace('/', "_")
        }
        fn clean_additional_extensions(s: &str) -> String {
            s.replace('.', "_")
        }
        let ident = &*ident.await?;

        // For clippy -- This explicit deref is necessary
        let path = &*ident.path.await?;
        let mut name = if let Some(inner) = self.context_path.await?.get_path_to(path) {
            clean_separators(inner)
        } else {
            clean_separators(&ident.path.to_string().await?)
        };
        let removed_extension = name.ends_with(extension);
        if removed_extension {
            name.truncate(name.len() - extension.len());
        }
        let mut name = clean_additional_extensions(&name);

        let default_modifier = match extension {
            ".js" => Some("ecmascript"),
            ".css" => Some("css"),
            _ => None,
        };

        let mut hasher = Xxh3Hash64Hasher::new();
        let mut has_hash = false;
        let AssetIdent {
            path: _,
            query,
            fragment,
            assets,
            modifiers,
            part,
        } = ident;
        if let Some(query) = query {
            0_u8.deterministic_hash(&mut hasher);
            query.await?.deterministic_hash(&mut hasher);
            has_hash = true;
        }
        if let Some(fragment) = fragment {
            1_u8.deterministic_hash(&mut hasher);
            fragment.await?.deterministic_hash(&mut hasher);
            has_hash = true;
        }
        for (key, ident) in assets.iter() {
            2_u8.deterministic_hash(&mut hasher);
            key.await?.deterministic_hash(&mut hasher);
            ident.to_string().await?.deterministic_hash(&mut hasher);
            has_hash = true;
        }
        for modifier in modifiers.iter() {
            let modifier = modifier.await?;
            if let Some(default_modifier) = default_modifier {
                if *modifier == default_modifier {
                    continue;
                }
            }
            3_u8.deterministic_hash(&mut hasher);
            modifier.deterministic_hash(&mut hasher);
            has_hash = true;
        }
        if let Some(part) = part {
            4_u8.deterministic_hash(&mut hasher);
            match &*part.await? {
                ModulePart::ModuleEvaluation => {
                    1_u8.deterministic_hash(&mut hasher);
                }
                ModulePart::Export(export) => {
                    2_u8.deterministic_hash(&mut hasher);
                    export.await?.deterministic_hash(&mut hasher);
                }
                ModulePart::Internal(id) => {
                    3_u8.deterministic_hash(&mut hasher);
                    id.deterministic_hash(&mut hasher);
                }
            }

            has_hash = true;
        }

        if has_hash {
            let hash = encode_hex(hasher.finish());
            let truncated_hash = &hash[..6];
            write!(name, "_{}", truncated_hash)?;
        }

        // Location in "path" where hashed and named parts are split.
        // Everything before i is hashed and after i named.
        let mut i = 0;
        static NODE_MODULES: &str = "_node_modules_";
        if let Some(j) = name.rfind(NODE_MODULES) {
            i = j + NODE_MODULES.len();
        }
        const MAX_FILENAME: usize = 80;
        if name.len() - i > MAX_FILENAME {
            i = name.len() - MAX_FILENAME;
            if let Some(j) = name[i..].find('_') {
                if j < 20 {
                    i += j + 1;
                }
            }
        }
        if i > 0 {
            let hash = encode_hex(hash_xxh3_hash64(name[..i].as_bytes()));
            let truncated_hash = &hash[..5];
            name = format!("{}_{}", truncated_hash, &name[i..]);
        }
        // We need to make sure that `.json` and `.json.js` doesn't end up with the same
        // name. So when we add an extra extension when want to mark that with a "._"
        // suffix.
        if !removed_extension {
            name += "._";
        }
        name += extension;
        let mut root_path = self.chunk_root_path;
        #[allow(clippy::single_match, reason = "future extensions")]
        match extension {
            ".css" => {
                if let Some(path) = self.css_chunk_root_path {
                    root_path = path;
                }
            }
            _ => {}
        }
        let root_path = if let Some(layer) = self.layer.as_deref() {
            root_path.join(layer)
        } else {
            root_path
        };
        Ok(root_path.join(&name))
    }

    #[turbo_tasks::function]
    fn reference_chunk_source_maps(&self, _chunk: AssetVc) -> BoolVc {
        BoolVc::cell(true)
    }

    #[turbo_tasks::function]
    async fn can_be_in_same_chunk(&self, asset_a: AssetVc, asset_b: AssetVc) -> Result<BoolVc> {
        let parent_dir = asset_a.ident().path().parent().await?;

        let path = asset_b.ident().path().await?;
        if let Some(rel_path) = parent_dir.get_path_to(&path) {
            if !rel_path.starts_with("node_modules/") && !rel_path.contains("/node_modules/") {
                return Ok(BoolVc::cell(true));
            }
        }

        Ok(BoolVc::cell(false))
    }

    #[turbo_tasks::function]
    fn asset_path(&self, content_hash: &str, extension: &str) -> FileSystemPathVc {
        self.asset_root_path
            .join(&format!("{content_hash}.{extension}"))
    }

    #[turbo_tasks::function]
    fn layer(&self) -> StringVc {
        StringVc::cell(self.layer.clone().unwrap_or_default())
    }

    #[turbo_tasks::function]
    async fn with_layer(self_vc: BuildChunkingContextVc, layer: &str) -> Result<ChunkingContextVc> {
        let mut context = self_vc.await?.clone_value();
        context.layer = (!layer.is_empty()).then(|| layer.to_string());
        Ok(BuildChunkingContextVc::new(Value::new(context)).into())
    }

    #[turbo_tasks::function]
    async fn chunk_group(
        self_vc: BuildChunkingContextVc,
        entry_chunk: ChunkVc,
    ) -> Result<AssetsVc> {
        let chunks = get_optimized_parallel_chunks([entry_chunk]).await?;

        let assets: Vec<AssetVc> = chunks
            .await?
            .iter()
            .map(|chunk| self_vc.generate_chunk(*chunk))
            .collect();

        Ok(AssetsVc::cell(assets))
    }

    #[turbo_tasks::function]
    async fn evaluated_chunk_group(
        self_vc: BuildChunkingContextVc,
        entry_chunk: ChunkVc,
        evaluatable_assets: EvaluatableAssetsVc,
    ) -> Result<AssetsVc> {
        let mut assets = self_vc
            .get_evaluate_chunk_assets(entry_chunk, evaluatable_assets)
            .await?;

        assets.push(self_vc.generate_evaluate_chunk(
            entry_chunk,
            AssetsVc::cell(assets.clone()),
            evaluatable_assets,
            None,
        ));

        Ok(AssetsVc::cell(assets))
    }
}

impl BuildChunkingContextVc {}

// TODO:

// #[turbo_tasks::function]
// fn ecmascript_runtime(self_vc: EcmascriptChunkingContextVc) ->
// EcmascriptChunkRuntimeVc {
//     EcmascriptBuildNodeChunkRuntimeVc::new(self_vc, None, None).into()
// }

// #[turbo_tasks::function]
// fn evaluated_ecmascript_runtime(
//     self_vc: EcmascriptChunkingContextVc,
//     evaluated_entries: EcmascriptChunkPlaceablesVc,
// ) -> EcmascriptChunkRuntimeVc {
//     EcmascriptBuildNodeChunkRuntimeVc::new(self_vc, Some(evaluated_entries),
// None).into() }

#[turbo_tasks::value_impl]
impl EcmascriptChunkingContext for BuildChunkingContext {
    #[turbo_tasks::function]
    fn manifest_loader_item(
        self_vc: BuildChunkingContextVc,
        asset: ChunkableAssetVc,
        availability_info: Value<AvailabilityInfo>,
    ) -> EcmascriptChunkItemVc {
        let manifest_asset = BuildManifestChunkAssetVc::new(asset, self_vc, availability_info);
        BuildManifestLoaderItemVc::new(manifest_asset).into()
    }
}

// #[turbo_tasks::value_impl]
// impl BuildChunkingContextVc {
//     #[turbo_tasks::function]
//     pub fn exported_ecmascript_runtime(
//         self_vc: EcmascriptChunkingContextVc,
//         entry: EcmascriptChunkPlaceableVc,
//     ) -> EcmascriptChunkRuntimeVc {
//         EcmascriptBuildNodeChunkRuntimeVc::new(self_vc, None,
// Some(entry)).into()     }
// }

async fn get_optimized_parallel_chunks<I>(entries: I) -> Result<ChunksVc>
where
    I: IntoIterator<Item = ChunkVc>,
{
    let chunks: Vec<_> = GraphTraversal::<SkipDuplicates<ReverseTopological<_>, _>>::visit(
        entries,
        get_chunk_children,
    )
    .await
    .completed()?
    .into_inner()
    .into_iter()
    .collect();

    let chunks = ChunksVc::cell(chunks);
    let chunks = optimize(chunks);

    Ok(chunks)
}

/// Computes the list of all chunk children of a given chunk.
async fn get_chunk_children(parent: ChunkVc) -> Result<impl Iterator<Item = ChunkVc> + Send> {
    Ok(parent
        .references()
        .await?
        .iter()
        .copied()
        .map(reference_to_chunks)
        .try_join()
        .await?
        .into_iter()
        .flatten())
}

/// Get all parallel chunks from a parallel chunk reference.
async fn reference_to_chunks(r: AssetReferenceVc) -> Result<impl Iterator<Item = ChunkVc> + Send> {
    let mut result = Vec::new();
    if let Some(pc) = ParallelChunkReferenceVc::resolve_from(r).await? {
        if *pc.is_loaded_in_parallel().await? {
            result = r
                .resolve_reference()
                .await?
                .primary
                .iter()
                .map(|r| async move {
                    Ok(if let PrimaryResolveResult::Asset(a) = r {
                        ChunkVc::resolve_from(a).await?
                    } else {
                        None
                    })
                })
                .try_join()
                .await?;
        }
    }
    Ok(result.into_iter().flatten())
}

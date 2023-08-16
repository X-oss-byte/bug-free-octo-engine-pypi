use std::fmt::Write;

use anyhow::Result;
use turbo_tasks::{
    primitives::{BoolVc, StringVc},
    Value, ValueToString,
};
use turbo_tasks_fs::FileSystemPathVc;
use turbo_tasks_hash::{encode_hex, hash_xxh3_hash64, DeterministicHash, Xxh3Hash64Hasher};

use super::{ChunkingContext, ChunkingContextVc};
use crate::{
    asset::{Asset, AssetVc},
    environment::EnvironmentVc,
    ident::{AssetIdent, AssetIdentVc},
};

pub struct DevChunkingContextBuilder {
    context: DevChunkingContext,
}

impl DevChunkingContextBuilder {
    pub fn hot_module_replacement(mut self) -> Self {
        self.context.enable_hot_module_replacement = true;
        self
    }

    pub fn layer(mut self, layer: &str) -> Self {
        self.context.layer = (!layer.is_empty()).then(|| layer.to_string());
        self
    }

    pub fn css_chunk_root_path(mut self, path: FileSystemPathVc) -> Self {
        self.context.css_chunk_root_path = Some(path);
        self
    }

    pub fn build(self) -> ChunkingContextVc {
        DevChunkingContextVc::new(Value::new(self.context)).into()
    }
}

/// A chunking context for development mode.
/// It uses readable filenames and module ids to improve development.
/// It also uses a chunking heuristic that is incremental and cacheable.
/// It splits "node_modules" separately as these are less likely to change
/// during development
#[turbo_tasks::value(serialization = "auto_for_input")]
#[derive(Debug, Clone, Hash, PartialOrd, Ord)]
pub struct DevChunkingContext {
    /// This path get striped off of path before creating a name out of it
    context_path: FileSystemPathVc,
    /// This path is used to compute the url to request chunks or assets from
    output_root_path: FileSystemPathVc,
    /// Chunks are placed at this path
    chunk_root_path: FileSystemPathVc,
    /// Css Chunks are placed at this path
    css_chunk_root_path: Option<FileSystemPathVc>,
    /// Static assets are placed at this path
    asset_root_path: FileSystemPathVc,
    /// Layer name within this context
    layer: Option<String>,
    /// Enable HMR for this chunking
    enable_hot_module_replacement: bool,
    /// The environment chunks will be evaluated in.
    environment: EnvironmentVc,
}

impl DevChunkingContextVc {
    pub fn builder(
        context_path: FileSystemPathVc,
        output_root_path: FileSystemPathVc,
        chunk_root_path: FileSystemPathVc,
        asset_root_path: FileSystemPathVc,
        environment: EnvironmentVc,
    ) -> DevChunkingContextBuilder {
        DevChunkingContextBuilder {
            context: DevChunkingContext {
                context_path,
                output_root_path,
                chunk_root_path,
                css_chunk_root_path: None,
                asset_root_path,
                layer: None,
                enable_hot_module_replacement: false,
                environment,
            },
        }
    }
}

#[turbo_tasks::value_impl]
impl DevChunkingContextVc {
    #[turbo_tasks::function]
    fn new(this: Value<DevChunkingContext>) -> Self {
        this.into_value().cell()
    }
}

#[turbo_tasks::value_impl]
impl ChunkingContext for DevChunkingContext {
    #[turbo_tasks::function]
    fn output_root(&self) -> FileSystemPathVc {
        self.output_root_path
    }

    #[turbo_tasks::function]
    fn environment(&self) -> EnvironmentVc {
        self.environment
    }

    #[turbo_tasks::function]
    async fn chunk_path(&self, ident: AssetIdentVc, extension: &str) -> Result<FileSystemPathVc> {
        fn clean(s: &str) -> String {
            s.replace('/', "_")
        }
        let ident = &*ident.await?;

        // For clippy -- This explicit deref is necessary
        let path = &*ident.path.await?;
        let mut name = if let Some(inner) = self.context_path.await?.get_path_to(path) {
            clean(inner)
        } else {
            clean(&ident.path.to_string().await?)
        };
        let removed_extension = name.ends_with(extension);
        if removed_extension {
            name.truncate(name.len() - extension.len());
        }

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
    fn is_hot_module_replacement_enabled(&self) -> BoolVc {
        BoolVc::cell(self.enable_hot_module_replacement)
    }

    #[turbo_tasks::function]
    fn layer(&self) -> StringVc {
        StringVc::cell(self.layer.clone().unwrap_or_default())
    }

    #[turbo_tasks::function]
    async fn with_layer(self_vc: DevChunkingContextVc, layer: &str) -> Result<ChunkingContextVc> {
        let mut context = self_vc.await?.clone_value();
        context.layer = (!layer.is_empty()).then(|| layer.to_string());
        Ok(DevChunkingContextVc::new(Value::new(context)).into())
    }
}

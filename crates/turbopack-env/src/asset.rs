use std::fmt::Write as _;

use anyhow::Result;
use turbo_tasks::{primitives::StringVc, ValueToString, ValueToStringVc};
use turbo_tasks_env::{ProcessEnv, ProcessEnvVc};
use turbo_tasks_fs::FileSystemPathVc;
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc},
    chunk::{ChunkItem, ChunkItemVc, ChunkVc, ChunkableAsset, ChunkableAssetVc, ChunkingContextVc},
    reference::AssetReferencesVc,
};
use turbopack_ecmascript::{
    chunk::{
        EcmascriptChunkItem, EcmascriptChunkItemContent, EcmascriptChunkItemContentVc,
        EcmascriptChunkItemVc, EcmascriptChunkPlaceable, EcmascriptChunkPlaceableVc,
        EcmascriptChunkVc, EcmascriptExports, EcmascriptExportsVc,
    },
    utils::stringify_js,
};

/// The `process.env` asset, responsible for initializing the env (shared by all
/// chunks) during app startup.
#[turbo_tasks::value]
pub struct ProcessEnvAsset {
    /// The root path which we can construct our env asset path.
    root: FileSystemPathVc,

    /// A HashMap filled with the env key/values.
    env: ProcessEnvVc,
}

#[turbo_tasks::value_impl]
impl ProcessEnvAssetVc {
    #[turbo_tasks::function]
    pub fn new(root: FileSystemPathVc, env: ProcessEnvVc) -> Self {
        ProcessEnvAsset { root, env }.cell()
    }
}

#[turbo_tasks::value_impl]
impl Asset for ProcessEnvAsset {
    #[turbo_tasks::function]
    fn path(&self) -> FileSystemPathVc {
        self.root.join(".env.js")
    }

    #[turbo_tasks::function]
    fn content(&self) -> AssetContentVc {
        unimplemented!();
    }

    #[turbo_tasks::function]
    fn references(&self) -> AssetReferencesVc {
        unimplemented!();
    }
}

#[turbo_tasks::value_impl]
impl ChunkableAsset for ProcessEnvAsset {
    #[turbo_tasks::function]
    fn as_chunk(self_vc: ProcessEnvAssetVc, context: ChunkingContextVc) -> ChunkVc {
        EcmascriptChunkVc::new(context, self_vc.into()).into()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkPlaceable for ProcessEnvAsset {
    #[turbo_tasks::function]
    fn as_chunk_item(
        self_vc: ProcessEnvAssetVc,
        context: ChunkingContextVc,
    ) -> EcmascriptChunkItemVc {
        ProcessEnvChunkItem {
            context,
            inner: self_vc,
        }
        .cell()
        .into()
    }

    #[turbo_tasks::function]
    fn get_exports(&self) -> EcmascriptExportsVc {
        EcmascriptExports::None.cell()
    }
}

#[turbo_tasks::value]
struct ProcessEnvChunkItem {
    context: ChunkingContextVc,
    inner: ProcessEnvAssetVc,
}

#[turbo_tasks::value_impl]
impl ValueToString for ProcessEnvChunkItem {
    #[turbo_tasks::function]
    fn to_string(&self) -> StringVc {
        self.inner.path().to_string()
    }
}

#[turbo_tasks::value_impl]
impl ChunkItem for ProcessEnvChunkItem {
    #[turbo_tasks::function]
    fn references(&self) -> AssetReferencesVc {
        AssetReferencesVc::empty()
    }
}

#[turbo_tasks::value_impl]
impl EcmascriptChunkItem for ProcessEnvChunkItem {
    #[turbo_tasks::function]
    fn chunking_context(&self) -> ChunkingContextVc {
        self.context
    }

    #[turbo_tasks::function]
    fn related_path(&self) -> FileSystemPathVc {
        self.inner.path()
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<EcmascriptChunkItemContentVc> {
        let asset = self.inner.await?;
        let env = asset.env.read_all().await?;

        // TODO: In SSR, we use the native process.env, which can only contain string
        // values. We need to inject literal values (to emulate webpack's
        // DefinePlugin), so create a new regular object out of the old env.
        let mut code = "const env = process.env = {...process.env};\n\n".to_string();

        for (name, val) in &*env {
            // It's assumed the env has passed through an EmbeddableProcessEnv, so the value
            // is ready to be directly embedded. Values _after_ an embeddable
            // env can be used to inject live code into the output.
            // TODO this is not completely correct as env vars need to ignore casing
            // So `process.env.path === process.env.PATH === process.env.PaTh`
            writeln!(code, "env[{}] = {};", stringify_js(name), val)?;
        }

        Ok(EcmascriptChunkItemContent {
            inner_code: code.into(),
            ..Default::default()
        }
        .cell())
    }
}

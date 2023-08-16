use std::io::Write;

use anyhow::Result;
use turbo_tasks_env::{ProcessEnv, ProcessEnvVc};
use turbo_tasks_fs::{rope::RopeBuilder, File, FileSystemPathVc};
use turbopack_core::{
    asset::{Asset, AssetContentVc, AssetVc},
    ident::AssetIdentVc,
};
use turbopack_ecmascript::utils::StringifyJs;

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
    fn ident(&self) -> AssetIdentVc {
        AssetIdentVc::from_path(self.root.join(".env.js"))
    }

    #[turbo_tasks::function]
    async fn content(&self) -> Result<AssetContentVc> {
        let env = self.env.read_all().await?;

        // TODO: In SSR, we use the native process.env, which can only contain string
        // values. We need to inject literal values (to emulate webpack's
        // DefinePlugin), so create a new regular object out of the old env.
        let mut code = RopeBuilder::default();
        code += "const env = process.env = {...process.env};\n\n";

        for (name, val) in &*env {
            // It's assumed the env has passed through an EmbeddableProcessEnv, so the value
            // is ready to be directly embedded. Values _after_ an embeddable
            // env can be used to inject live code into the output.
            // TODO this is not completely correct as env vars need to ignore casing
            // So `process.env.path === process.env.PATH === process.env.PaTh`
            writeln!(code, "env[{}] = {};", StringifyJs(name), val)?;
        }

        Ok(File::from(code.build()).into())
    }
}

use anyhow::Result;
use turbopack_core::{
    code_builder::{CodeBuilder, CodeVc},
    environment::EnvironmentVc,
};

use crate::{asset_context::get_runtime_asset_context, embed_js::embed_static_code};

/// Returns the code for the Node.js production ECMAScript runtime.
#[turbo_tasks::function]
pub async fn get_build_runtime_code(environment: EnvironmentVc) -> Result<CodeVc> {
    let asset_context = get_runtime_asset_context(environment);

    let shared_runtime_utils_code = embed_static_code(asset_context, "shared/runtime-utils.ts");
    let shared_node_utils_code = embed_static_code(asset_context, "shared-node/require.ts");
    let runtime_code = embed_static_code(asset_context, "build/runtime.ts");

    let mut code = CodeBuilder::default();
    code.push_code(&*shared_runtime_utils_code.await?);
    code.push_code(&*shared_node_utils_code.await?);
    code.push_code(&*runtime_code.await?);

    Ok(CodeVc::cell(code.build()))
}

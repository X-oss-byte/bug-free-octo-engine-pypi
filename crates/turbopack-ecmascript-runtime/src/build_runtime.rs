use anyhow::Result;
use turbo_tasks::Vc;
use turbopack_core::{
    code_builder::{Code, CodeBuilder},
    environment::Environment,
};

use crate::{asset_context::get_runtime_asset_context, embed_js::embed_static_code};

/// Returns the code for the Node.js production ECMAScript runtime.
#[turbo_tasks::function]
pub async fn get_build_runtime_code(environment: Vc<Environment>) -> Result<Vc<Code>> {
    let asset_context = get_runtime_asset_context(environment);

    let shared_runtime_utils_code =
        embed_static_code(asset_context, "shared/runtime-utils.ts".to_string());
    let shared_node_utils_code =
        embed_static_code(asset_context, "shared-node/require.ts".to_string());
    let runtime_code = embed_static_code(asset_context, "build/runtime.ts".to_string());

    let mut code = CodeBuilder::default();
    code.push_code(&*shared_runtime_utils_code.await?);
    code.push_code(&*shared_node_utils_code.await?);
    code.push_code(&*runtime_code.await?);

    Ok(Code::cell(code.build()))
}

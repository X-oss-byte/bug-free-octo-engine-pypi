use std::{collections::HashMap, fmt};

use anyhow::Result;
use turbo_tasks_fs::{FileSystem, FileSystemPathVc};
use turbopack::{
    condition::ContextCondition,
    ecmascript::TransformPluginVc,
    module_options::{
        CustomEcmascriptTransformPlugins, CustomEcmascriptTransformPluginsVc, JsxTransformOptions,
        ModuleOptionsContext, ModuleOptionsContextVc,
    },
    resolve_options_context::{ResolveOptionsContext, ResolveOptionsContextVc},
    transition::TransitionsByNameVc,
    ModuleAssetContextVc,
};
use turbopack_core::{
    compile_time_defines,
    compile_time_info::{CompileTimeDefinesVc, CompileTimeInfo, CompileTimeInfoVc},
    context::AssetContextVc,
    environment::EnvironmentVc,
    resolve::options::{ImportMap, ImportMapVc, ImportMapping},
};
use turbopack_dev::react_refresh::assert_can_resolve_react_refresh;
use turbopack_ecmascript_plugins::transform::{
    emotion::{EmotionTransformConfig, EmotionTransformer},
    styled_components::{StyledComponentsTransformConfig, StyledComponentsTransformer},
    styled_jsx::StyledJsxTransformer,
};
use turbopack_node::execution_context::ExecutionContextVc;

#[turbo_tasks::value(shared)]
pub enum NodeEnv {
    Development,
    Production,
}

impl fmt::Display for NodeEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NodeEnv::Development => f.write_str("development"),
            NodeEnv::Production => f.write_str("production"),
        }
    }
}

async fn foreign_code_context_condition() -> Result<ContextCondition> {
    Ok(ContextCondition::InDirectory("node_modules".to_string()))
}

#[turbo_tasks::function]
pub async fn get_client_import_map(project_path: FileSystemPathVc) -> Result<ImportMapVc> {
    let mut import_map = ImportMap::empty();

    import_map.insert_singleton_alias("@swc/helpers", project_path);
    import_map.insert_singleton_alias("styled-jsx", project_path);
    import_map.insert_singleton_alias("react", project_path);
    import_map.insert_singleton_alias("react-dom", project_path);

    import_map.insert_wildcard_alias(
        "@vercel/turbopack-ecmascript-runtime/",
        ImportMapping::PrimaryAlternative(
            "./*".to_string(),
            Some(turbopack_ecmascript_runtime::embed_fs().root()),
        )
        .cell(),
    );

    Ok(import_map.cell())
}

#[turbo_tasks::function]
pub async fn get_client_resolve_options_context(
    project_path: FileSystemPathVc,
    node_env: NodeEnvVc,
) -> Result<ResolveOptionsContextVc> {
    let next_client_import_map = get_client_import_map(project_path);
    let module_options_context = ResolveOptionsContext {
        enable_node_modules: Some(project_path.root().resolve().await?),
        custom_conditions: vec![node_env.await?.to_string()],
        import_map: Some(next_client_import_map),
        browser: true,
        module: true,
        ..Default::default()
    };
    Ok(ResolveOptionsContext {
        enable_typescript: true,
        enable_react: true,
        rules: vec![(
            foreign_code_context_condition().await?,
            module_options_context.clone().cell(),
        )],
        ..module_options_context
    }
    .cell())
}

#[turbo_tasks::function]
async fn get_client_module_options_context(
    project_path: FileSystemPathVc,
    execution_context: ExecutionContextVc,
    env: EnvironmentVc,
    node_env: NodeEnvVc,
) -> Result<ModuleOptionsContextVc> {
    let module_options_context = ModuleOptionsContext {
        preset_env_versions: Some(env),
        execution_context: Some(execution_context),
        ..Default::default()
    };

    let resolve_options_context = get_client_resolve_options_context(project_path, node_env);

    let enable_react_refresh = matches!(*node_env.await?, NodeEnv::Development)
        && assert_can_resolve_react_refresh(project_path, resolve_options_context)
            .await?
            .is_found();

    let enable_jsx = Some(
        JsxTransformOptions {
            react_refresh: enable_react_refresh,
            ..Default::default()
        }
        .cell(),
    );

    let custom_ecma_transform_plugins = Some(CustomEcmascriptTransformPluginsVc::cell(
        CustomEcmascriptTransformPlugins {
            source_transforms: vec![
                TransformPluginVc::cell(Box::new(
                    EmotionTransformer::new(&EmotionTransformConfig::default())
                        .expect("Should be able to create emotion transformer"),
                )),
                TransformPluginVc::cell(Box::new(StyledComponentsTransformer::new(
                    &StyledComponentsTransformConfig::default(),
                ))),
                TransformPluginVc::cell(Box::new(StyledJsxTransformer::new())),
            ],
            output_transforms: vec![],
        },
    ));

    let module_options_context = ModuleOptionsContext {
        enable_jsx,
        enable_postcss_transform: Some(Default::default()),
        enable_typescript_transform: Some(Default::default()),
        rules: vec![(
            foreign_code_context_condition().await?,
            module_options_context.clone().cell(),
        )],
        custom_ecma_transform_plugins,
        ..module_options_context
    }
    .cell();

    Ok(module_options_context)
}

#[turbo_tasks::function]
pub fn get_client_asset_context(
    project_path: FileSystemPathVc,
    execution_context: ExecutionContextVc,
    compile_time_info: CompileTimeInfoVc,
    node_env: NodeEnvVc,
) -> AssetContextVc {
    let resolve_options_context = get_client_resolve_options_context(project_path, node_env);
    let module_options_context = get_client_module_options_context(
        project_path,
        execution_context,
        compile_time_info.environment(),
        node_env,
    );

    let context: AssetContextVc = ModuleAssetContextVc::new(
        TransitionsByNameVc::cell(HashMap::new()),
        compile_time_info,
        module_options_context,
        resolve_options_context,
    )
    .into();

    context
}

fn client_defines(node_env: &NodeEnv) -> CompileTimeDefinesVc {
    compile_time_defines!(
        process.turbopack = true,
        process.env.NODE_ENV = node_env.to_string()
    )
    .cell()
}

#[turbo_tasks::function]
pub async fn get_client_compile_time_info(
    env: EnvironmentVc,
    node_env: NodeEnvVc,
) -> Result<CompileTimeInfoVc> {
    Ok(CompileTimeInfo::builder(env)
        .defines(client_defines(&*node_env.await?))
        .cell())
}

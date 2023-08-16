use core::{default::Default, result::Result::Ok};
use std::collections::HashMap;

use anyhow::Result;
use turbo_tasks::Value;
use turbo_tasks_env::ProcessEnvVc;
use turbo_tasks_fs::FileSystemPathVc;
use turbopack::{
    module_options::{
        module_options_context::{ModuleOptionsContext, ModuleOptionsContextVc},
        ModuleRule, ModuleRuleCondition, ModuleRuleEffect,
    },
    resolve_options_context::{ResolveOptionsContext, ResolveOptionsContextVc},
    transition::TransitionsByNameVc,
    ModuleAssetContextVc,
};
use turbopack_core::{
    chunk::{dev::DevChunkingContextVc, ChunkingContextVc},
    context::AssetContextVc,
    environment::{BrowserEnvironment, EnvironmentIntention, EnvironmentVc, ExecutionEnvironment},
    resolve::{parse::RequestVc, pattern::Pattern},
};
use turbopack_ecmascript::{EcmascriptInputTransform, EcmascriptInputTransformsVc};
use turbopack_env::ProcessEnvAssetVc;

use crate::{
    embed_js::attached_next_js_package_path,
    env::env_for_js,
    next_client::runtime_entry::{RuntimeEntriesVc, RuntimeEntry},
    next_import_map::{
        get_next_client_fallback_import_map, get_next_client_import_map,
        get_next_client_resolved_map,
    },
    react_refresh::assert_can_resolve_react_refresh,
};

#[turbo_tasks::function]
pub fn get_client_environment(browserslist_query: &str) -> EnvironmentVc {
    EnvironmentVc::new(
        Value::new(ExecutionEnvironment::Browser(
            BrowserEnvironment {
                dom: true,
                web_worker: false,
                service_worker: false,
                browserslist_query: browserslist_query.to_owned(),
            }
            .into(),
        )),
        Value::new(EnvironmentIntention::Client),
    )
}

#[turbo_tasks::value(serialization = "auto_for_input")]
#[derive(Debug, Copy, Clone, Hash, PartialOrd, Ord)]
pub enum ContextType {
    Pages { pages_dir: FileSystemPathVc },
    App { app_dir: FileSystemPathVc },
    Fallback,
    Other,
}

#[turbo_tasks::function]
pub fn get_client_resolve_options_context(
    project_root: FileSystemPathVc,
    ty: Value<ContextType>,
) -> ResolveOptionsContextVc {
    let next_client_import_map = get_next_client_import_map(project_root, ty);
    let next_client_fallback_import_map = get_next_client_fallback_import_map(ty);
    let next_client_resolved_map = get_next_client_resolved_map(project_root, project_root);
    ResolveOptionsContext {
        enable_typescript: true,
        enable_react: true,
        enable_node_modules: true,
        custom_conditions: vec!["development".to_string()],
        import_map: Some(next_client_import_map),
        fallback_import_map: Some(next_client_fallback_import_map),
        resolved_map: Some(next_client_resolved_map),
        browser: true,
        module: true,
        ..Default::default()
    }
    .cell()
}

#[turbo_tasks::function]
pub async fn get_client_module_options_context(
    project_root: FileSystemPathVc,
    env: EnvironmentVc,
    ty: Value<ContextType>,
) -> Result<ModuleOptionsContextVc> {
    let resolve_options_context = get_client_resolve_options_context(project_root, ty);
    let enable_react_refresh =
        assert_can_resolve_react_refresh(project_root, resolve_options_context)
            .await?
            .is_found();

    let module_options_context = ModuleOptionsContext {
        // We don't need to resolve React Refresh for each module. Instead,
        // we try resolve it once at the root and pass down a context to all
        // the modules.
        enable_emotion: true,
        enable_react_refresh,
        enable_styled_components: true,
        enable_styled_jsx: true,
        enable_typescript_transform: true,
        preset_env_versions: Some(env),
        ..Default::default()
    };

    Ok(add_next_font_transform(module_options_context.cell()))
}

#[turbo_tasks::function]
pub async fn add_next_transforms_to_pages(
    module_options_context: ModuleOptionsContextVc,
    pages_dir: FileSystemPathVc,
) -> Result<ModuleOptionsContextVc> {
    let mut module_options_context = module_options_context.await?.clone_value();
    // Apply the Next SSG tranform to all pages.
    module_options_context.custom_rules.push(ModuleRule::new(
        ModuleRuleCondition::all(vec![
            ModuleRuleCondition::ResourcePathInExactDirectory(pages_dir.await?),
            ModuleRuleCondition::any(vec![
                ModuleRuleCondition::ResourcePathEndsWith(".js".to_string()),
                ModuleRuleCondition::ResourcePathEndsWith(".jsx".to_string()),
                ModuleRuleCondition::ResourcePathEndsWith(".ts".to_string()),
                ModuleRuleCondition::ResourcePathEndsWith(".tsx".to_string()),
            ]),
        ]),
        vec![ModuleRuleEffect::AddEcmascriptTransforms(
            EcmascriptInputTransformsVc::cell(vec![EcmascriptInputTransform::NextJsPageSsr]),
        )],
    ));
    Ok(module_options_context.cell())
}

#[turbo_tasks::function]
pub async fn add_next_font_transform(
    module_options_context: ModuleOptionsContextVc,
) -> Result<ModuleOptionsContextVc> {
    #[cfg(not(feature = "next-font"))]
    return Ok(module_options_context);

    #[cfg(feature = "next-font")]
    {
        use turbopack::module_options::{ModuleRule, ModuleRuleCondition, ModuleRuleEffect};
        use turbopack_ecmascript::{EcmascriptInputTransform, EcmascriptInputTransformsVc};

        let mut module_options_context = module_options_context.await?.clone_value();
        module_options_context.custom_rules.push(ModuleRule::new(
            // TODO: Only match in pages (not pages/api), app/, etc.
            ModuleRuleCondition::any(vec![
                ModuleRuleCondition::ResourcePathEndsWith(".js".to_string()),
                ModuleRuleCondition::ResourcePathEndsWith(".jsx".to_string()),
                ModuleRuleCondition::ResourcePathEndsWith(".ts".to_string()),
                ModuleRuleCondition::ResourcePathEndsWith(".tsx".to_string()),
            ]),
            vec![ModuleRuleEffect::AddEcmascriptTransforms(
                EcmascriptInputTransformsVc::cell(vec![EcmascriptInputTransform::NextJsFont]),
            )],
        ));
        Ok(module_options_context.cell())
    }
}

#[turbo_tasks::function]
pub fn get_client_asset_context(
    project_root: FileSystemPathVc,
    browserslist_query: &str,
    ty: Value<ContextType>,
) -> AssetContextVc {
    let environment = get_client_environment(browserslist_query);
    let resolve_options_context = get_client_resolve_options_context(project_root, ty);
    let module_options_context = get_client_module_options_context(project_root, environment, ty);

    let context: AssetContextVc = ModuleAssetContextVc::new(
        TransitionsByNameVc::cell(HashMap::new()),
        environment,
        module_options_context,
        resolve_options_context,
    )
    .into();

    context
}

#[turbo_tasks::function]
pub fn get_client_chunking_context(
    project_root: FileSystemPathVc,
    server_root: FileSystemPathVc,
    ty: Value<ContextType>,
) -> ChunkingContextVc {
    DevChunkingContextVc::builder(
        project_root,
        server_root,
        match ty.into_value() {
            ContextType::Pages { .. } | ContextType::App { .. } => {
                server_root.join("/_next/static/chunks")
            }
            ContextType::Fallback | ContextType::Other => server_root.join("/_chunks"),
        },
        get_client_assets_path(server_root, ty),
    )
    .hot_module_replacement()
    .build()
}

#[turbo_tasks::function]
pub fn get_client_assets_path(
    server_root: FileSystemPathVc,
    ty: Value<ContextType>,
) -> FileSystemPathVc {
    match ty.into_value() {
        ContextType::Pages { .. } | ContextType::App { .. } => {
            server_root.join("/_next/static/assets")
        }
        ContextType::Fallback | ContextType::Other => server_root.join("/_assets"),
    }
}

#[turbo_tasks::function]
pub async fn get_client_runtime_entries(
    project_root: FileSystemPathVc,
    env: ProcessEnvVc,
    ty: Value<ContextType>,
) -> Result<RuntimeEntriesVc> {
    let resolve_options_context = get_client_resolve_options_context(project_root, ty);
    let enable_react_refresh =
        assert_can_resolve_react_refresh(project_root, resolve_options_context)
            .await?
            .as_request();

    let mut runtime_entries = vec![RuntimeEntry::Ecmascript(
        ProcessEnvAssetVc::new(project_root, env_for_js(env, true)).into(),
    )
    .cell()];

    // It's important that React Refresh come before the regular bootstrap file,
    // because the bootstrap contains JSX which requires Refresh's global
    // functions to be available.
    if let Some(request) = enable_react_refresh {
        runtime_entries.push(RuntimeEntry::Request(request, project_root.join("_")).cell())
    };
    if matches!(ty.into_value(), ContextType::Other) {
        runtime_entries.push(
            RuntimeEntry::Request(
                RequestVc::parse(Value::new(Pattern::Constant(
                    "./dev/bootstrap.ts".to_string(),
                ))),
                attached_next_js_package_path(project_root).join("_"),
            )
            .cell(),
        );
    }

    Ok(RuntimeEntriesVc::cell(runtime_entries))
}

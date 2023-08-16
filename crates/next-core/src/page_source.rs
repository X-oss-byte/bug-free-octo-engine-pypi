use std::collections::HashMap;

use anyhow::Result;
use turbo_tasks::{
    primitives::{BoolVc, StringVc},
    Value,
};
use turbo_tasks_env::ProcessEnvVc;
use turbo_tasks_fs::{DirectoryContent, DirectoryEntry, FileSystemEntryType, FileSystemPathVc};
use turbopack::{
    module_options::ModuleOptionsContextVc, transition::TransitionsByNameVc, ModuleAssetContextVc,
};
use turbopack_core::{
    asset::AssetVc,
    chunk::{dev::DevChunkingContextVc, ChunkingContextVc},
    context::AssetContextVc,
    environment::{EnvironmentVc, ServerAddrVc},
    reference_type::{EntryReferenceSubType, ReferenceType},
    source_asset::SourceAssetVc,
    virtual_asset::VirtualAssetVc,
};
use turbopack_dev_server::{
    html::DevHtmlAssetVc,
    source::{
        asset_graph::AssetGraphContentSourceVc,
        combined::{CombinedContentSource, CombinedContentSourceVc},
        specificity::SpecificityVc,
        ContentSourceData, ContentSourceVc, NoContentSourceVc,
    },
};
use turbopack_ecmascript::{
    chunk::EcmascriptChunkPlaceablesVc, EcmascriptInputTransform, EcmascriptInputTransformsVc,
    EcmascriptModuleAssetType, EcmascriptModuleAssetVc,
};
use turbopack_env::ProcessEnvAssetVc;
use turbopack_node::{
    execution_context::ExecutionContextVc,
    render::{
        node_api_source::create_node_api_source, rendered_source::create_node_rendered_source,
    },
    NodeEntry, NodeEntryVc, NodeRenderingEntry, NodeRenderingEntryVc,
};

use crate::{
    embed_js::{next_js_file, wrap_with_next_js_fs},
    env::env_for_js,
    fallback::get_fallback_page,
    next_client::{
        context::{
            get_client_assets_path, get_client_chunking_context, get_client_environment,
            get_client_module_options_context, get_client_resolve_options_context,
            get_client_runtime_entries, ClientContextType,
        },
        transition::NextClientTransition,
    },
    next_config::NextConfigVc,
    next_server::context::{
        get_server_environment, get_server_module_options_context,
        get_server_resolve_options_context, ServerContextType,
    },
    next_shared::transforms::{add_next_transforms_to_pages, PageTransformType},
    page_loader::create_page_loader,
    util::{get_asset_path_from_route, pathname_for_path, regular_expression_for_path},
};

#[turbo_tasks::function]
fn get_page_client_module_options_context(
    project_path: FileSystemPathVc,
    execution_context: ExecutionContextVc,
    client_environment: EnvironmentVc,
    ty: Value<ClientContextType>,
    next_config: NextConfigVc,
) -> Result<ModuleOptionsContextVc> {
    let client_module_options_context = get_client_module_options_context(
        project_path,
        execution_context,
        client_environment,
        ty,
        next_config,
    );

    let client_module_options_context = match ty.into_value() {
        ClientContextType::Pages { pages_dir } => add_next_transforms_to_pages(
            client_module_options_context,
            pages_dir,
            Value::new(PageTransformType::Client),
        ),
        _ => client_module_options_context,
    };

    Ok(client_module_options_context)
}

#[turbo_tasks::value(serialization = "auto_for_input")]
#[derive(Debug, Copy, Clone, Hash, PartialOrd, Ord)]
pub enum PageSsrType {
    Ssr,
    SsrData,
}

#[turbo_tasks::function]
fn get_page_server_module_options_context(
    project_path: FileSystemPathVc,
    execution_context: ExecutionContextVc,
    pages_dir: FileSystemPathVc,
    ssr_ty: Value<PageSsrType>,
    next_config: NextConfigVc,
) -> ModuleOptionsContextVc {
    let server_ty = Value::new(ServerContextType::Pages { pages_dir });
    let server_module_options_context =
        get_server_module_options_context(project_path, execution_context, server_ty, next_config);

    match ssr_ty.into_value() {
        PageSsrType::Ssr => server_module_options_context,
        PageSsrType::SsrData => {
            let transform_ty = Value::new(PageTransformType::SsrData);
            add_next_transforms_to_pages(server_module_options_context, pages_dir, transform_ty)
        }
    }
}

/// Create a content source serving the `pages` or `src/pages` directory as
/// Next.js pages folder.
#[turbo_tasks::function]
pub async fn create_page_source(
    project_root: FileSystemPathVc,
    execution_context: ExecutionContextVc,
    output_path: FileSystemPathVc,
    server_root: FileSystemPathVc,
    env: ProcessEnvVc,
    browserslist_query: &str,
    next_config: NextConfigVc,
    server_addr: ServerAddrVc,
) -> Result<ContentSourceVc> {
    let project_path = wrap_with_next_js_fs(project_root);

    let pages = project_path.join("pages");
    let src_pages = project_path.join("src/pages");
    let pages_dir = if *pages.get_type().await? == FileSystemEntryType::Directory {
        pages
    } else if *src_pages.get_type().await? == FileSystemEntryType::Directory {
        src_pages
    } else {
        return Ok(NoContentSourceVc::new().into());
    };

    let ty = Value::new(ClientContextType::Pages { pages_dir });
    let server_ty = Value::new(ServerContextType::Pages { pages_dir });

    let client_environment = get_client_environment(browserslist_query);
    let client_module_options_context = get_page_client_module_options_context(
        project_path,
        execution_context,
        client_environment,
        ty,
        next_config,
    );
    let client_module_options_context = add_next_transforms_to_pages(
        client_module_options_context,
        pages_dir,
        Value::new(PageTransformType::Client),
    );
    let client_resolve_options_context =
        get_client_resolve_options_context(project_path, ty, next_config);
    let client_context: AssetContextVc = ModuleAssetContextVc::new(
        TransitionsByNameVc::cell(HashMap::new()),
        client_environment,
        client_module_options_context,
        client_resolve_options_context,
    )
    .into();

    let client_chunking_context = get_client_chunking_context(project_path, server_root, ty);

    let client_runtime_entries = get_client_runtime_entries(project_path, env, ty, next_config);

    let next_client_transition = NextClientTransition {
        is_app: false,
        client_chunking_context,
        client_module_options_context,
        client_resolve_options_context,
        client_environment,
        server_root,
        runtime_entries: client_runtime_entries,
    }
    .cell()
    .into();

    let mut transitions = HashMap::new();
    transitions.insert("next-client".to_string(), next_client_transition);
    let server_environment = get_server_environment(server_ty, env, server_addr);
    let server_resolve_options_context =
        get_server_resolve_options_context(project_path, server_ty, next_config);

    let server_module_options_context = get_page_server_module_options_context(
        project_path,
        execution_context,
        pages_dir,
        Value::new(PageSsrType::Ssr),
        next_config,
    );
    let server_transitions = TransitionsByNameVc::cell(
        [("next-client".to_string(), next_client_transition)]
            .into_iter()
            .collect(),
    );

    let server_context: AssetContextVc = ModuleAssetContextVc::new(
        server_transitions,
        server_environment,
        server_module_options_context,
        server_resolve_options_context,
    )
    .into();

    let server_data_module_options_context = get_page_server_module_options_context(
        project_path,
        execution_context,
        pages_dir,
        Value::new(PageSsrType::SsrData),
        next_config,
    );

    let server_data_context: AssetContextVc = ModuleAssetContextVc::new(
        TransitionsByNameVc::cell(HashMap::new()),
        server_environment,
        server_data_module_options_context,
        server_resolve_options_context,
    )
    .into();

    let server_runtime_entries =
        vec![
            ProcessEnvAssetVc::new(project_path, env_for_js(env, false, next_config))
                .as_ecmascript_chunk_placeable(),
        ];

    let fallback_page = get_fallback_page(
        project_path,
        execution_context,
        server_root,
        env,
        browserslist_query,
        next_config,
    );

    let page_source = create_page_source_for_directory(
        project_path,
        server_context,
        server_data_context,
        client_context,
        pages_dir,
        SpecificityVc::exact(),
        0,
        pages_dir,
        EcmascriptChunkPlaceablesVc::cell(server_runtime_entries),
        fallback_page,
        server_root,
        server_root,
        server_root.join("api"),
        output_path,
    );
    let fallback_source =
        AssetGraphContentSourceVc::new_eager(server_root, fallback_page.as_asset());

    Ok(CombinedContentSource {
        sources: vec![page_source.into(), fallback_source.into()],
    }
    .cell()
    .into())
}

/// Handles a single page file in the pages directory
#[turbo_tasks::function]
async fn create_page_source_for_file(
    context_path: FileSystemPathVc,
    server_context: AssetContextVc,
    server_data_context: AssetContextVc,
    client_context: AssetContextVc,
    pages_dir: FileSystemPathVc,
    specificity: SpecificityVc,
    page_file: FileSystemPathVc,
    runtime_entries: EcmascriptChunkPlaceablesVc,
    fallback_page: DevHtmlAssetVc,
    server_root: FileSystemPathVc,
    server_path: FileSystemPathVc,
    is_api_path: BoolVc,
    intermediate_output_path: FileSystemPathVc,
) -> Result<ContentSourceVc> {
    let source_asset = SourceAssetVc::new(page_file).into();
    let entry_asset = server_context.process(
        source_asset,
        Value::new(ReferenceType::Entry(EntryReferenceSubType::Page)),
    );
    let data_asset = server_data_context.process(
        source_asset,
        Value::new(ReferenceType::Entry(EntryReferenceSubType::Page)),
    );

    let server_chunking_context = DevChunkingContextVc::builder(
        context_path,
        intermediate_output_path,
        intermediate_output_path.join("chunks"),
        get_client_assets_path(
            server_root,
            Value::new(ClientContextType::Pages { pages_dir }),
        ),
    )
    .build();

    let data_intermediate_output_path = intermediate_output_path.join("data");

    let server_data_chunking_context = DevChunkingContextVc::builder(
        context_path,
        data_intermediate_output_path,
        data_intermediate_output_path.join("chunks"),
        get_client_assets_path(
            server_root,
            Value::new(ClientContextType::Pages { pages_dir }),
        ),
    )
    .build();

    let client_chunking_context = get_client_chunking_context(
        context_path,
        server_root,
        Value::new(ClientContextType::Pages { pages_dir }),
    );

    let pathname = pathname_for_path(server_root, server_path, true);
    let path_regex = regular_expression_for_path(pathname);

    Ok(if *is_api_path.await? {
        create_node_api_source(
            specificity,
            server_root,
            pathname,
            path_regex,
            SsrEntry {
                context: server_context,
                entry_asset,
                is_api_path,
                chunking_context: server_chunking_context,
                intermediate_output_path,
            }
            .cell()
            .into(),
            runtime_entries,
        )
    } else {
        let data_pathname = StringVc::cell(format!(
            "_next/data/development/{}",
            get_asset_path_from_route(&pathname.await?, ".json")
        ));
        let data_path_regex = regular_expression_for_path(data_pathname);

        let ssr_entry = SsrEntry {
            context: server_context,
            entry_asset,
            is_api_path,
            chunking_context: server_chunking_context,
            intermediate_output_path,
        }
        .cell()
        .into();

        let ssr_data_entry = SsrEntry {
            context: server_data_context,
            entry_asset: data_asset,
            is_api_path,
            chunking_context: server_data_chunking_context,
            intermediate_output_path: data_intermediate_output_path,
        }
        .cell()
        .into();

        CombinedContentSourceVc::new(vec![
            create_node_rendered_source(
                specificity,
                server_root,
                pathname,
                path_regex,
                ssr_entry,
                runtime_entries,
                fallback_page,
            ),
            create_node_rendered_source(
                specificity,
                server_root,
                data_pathname,
                data_path_regex,
                ssr_data_entry,
                runtime_entries,
                fallback_page,
            ),
            create_page_loader(
                server_root,
                client_context,
                client_chunking_context,
                entry_asset,
                pathname,
            ),
        ])
        .into()
    })
}

/// Handles a directory in the pages directory (or the pages directory itself).
/// Calls itself recursively for sub directories or the
/// [create_page_source_for_file] method for files.
#[turbo_tasks::function]
async fn create_page_source_for_directory(
    context_path: FileSystemPathVc,
    server_context: AssetContextVc,
    server_data_context: AssetContextVc,
    client_context: AssetContextVc,
    pages_dir: FileSystemPathVc,
    specificity: SpecificityVc,
    position: u32,
    input_dir: FileSystemPathVc,
    runtime_entries: EcmascriptChunkPlaceablesVc,
    fallback_page: DevHtmlAssetVc,
    server_root: FileSystemPathVc,
    server_path: FileSystemPathVc,
    server_api_path: FileSystemPathVc,
    intermediate_output_path: FileSystemPathVc,
) -> Result<CombinedContentSourceVc> {
    let mut sources = vec![];
    let dir_content = input_dir.read_dir().await?;
    if let DirectoryContent::Entries(entries) = &*dir_content {
        for (name, entry) in entries.iter() {
            let specificity = if name.starts_with("[[") || name.starts_with("[...") {
                specificity.with_catch_all(position)
            } else if name.starts_with('[') {
                specificity.with_dynamic_segment(position)
            } else {
                specificity
            };
            match entry {
                DirectoryEntry::File(file) => {
                    if let Some((basename, extension)) = name.rsplit_once('.') {
                        match extension {
                            // pageExtensions option from next.js
                            // defaults: https://github.com/vercel/next.js/blob/611e13f5159457fedf96d850845650616a1f75dd/packages/next/server/config-shared.ts#L499
                            "js" | "ts" | "jsx" | "tsx" | "mdx" => {
                                let (dev_server_path, intermediate_output_path, specificity) =
                                    if basename == "index" {
                                        (
                                            server_path.join("index.html"),
                                            intermediate_output_path,
                                            specificity,
                                        )
                                    } else if basename == "404" {
                                        (
                                            server_path.join("[...].html"),
                                            intermediate_output_path.join(basename),
                                            specificity.with_fallback(position),
                                        )
                                    } else {
                                        (
                                            server_path.join(basename).join("index.html"),
                                            intermediate_output_path.join(basename),
                                            specificity,
                                        )
                                    };
                                sources.push((
                                    name,
                                    create_page_source_for_file(
                                        context_path,
                                        server_context,
                                        server_data_context,
                                        client_context,
                                        pages_dir,
                                        specificity,
                                        *file,
                                        runtime_entries,
                                        fallback_page,
                                        server_root,
                                        dev_server_path,
                                        dev_server_path.is_inside(server_api_path),
                                        intermediate_output_path,
                                    ),
                                ));
                            }
                            _ => {}
                        }
                    }
                }
                DirectoryEntry::Directory(dir) => {
                    sources.push((
                        name,
                        create_page_source_for_directory(
                            context_path,
                            server_context,
                            server_data_context,
                            client_context,
                            pages_dir,
                            specificity,
                            position + 1,
                            *dir,
                            runtime_entries,
                            fallback_page,
                            server_root,
                            server_path.join(name),
                            server_api_path,
                            intermediate_output_path.join(name),
                        )
                        .into(),
                    ));
                }
                _ => {}
            }
        }
    }

    // Ensure deterministic order since read_dir is not deterministic
    sources.sort_by_key(|(k, _)| *k);

    Ok(CombinedContentSource {
        sources: sources.into_iter().map(|(_, v)| v).collect(),
    }
    .cell())
}

/// The node.js renderer for SSR of pages.
#[turbo_tasks::value]
struct SsrEntry {
    context: AssetContextVc,
    entry_asset: AssetVc,
    is_api_path: BoolVc,
    chunking_context: ChunkingContextVc,
    intermediate_output_path: FileSystemPathVc,
}

#[turbo_tasks::value_impl]
impl SsrEntryVc {
    #[turbo_tasks::function]
    async fn entry(self) -> Result<NodeRenderingEntryVc> {
        let this = self.await?;
        let virtual_asset = if *this.is_api_path.await? {
            VirtualAssetVc::new(
                this.entry_asset.path().join("server-api.tsx"),
                next_js_file("entry/server-api.tsx").into(),
            )
        } else {
            VirtualAssetVc::new(
                this.entry_asset.path().join("server-renderer.tsx"),
                next_js_file("entry/server-renderer.tsx").into(),
            )
        };

        Ok(NodeRenderingEntry {
            module: EcmascriptModuleAssetVc::new(
                virtual_asset.into(),
                this.context,
                Value::new(EcmascriptModuleAssetType::Typescript),
                EcmascriptInputTransformsVc::cell(vec![
                    EcmascriptInputTransform::TypeScript,
                    EcmascriptInputTransform::React { refresh: false },
                ]),
                this.context.environment(),
            ),
            chunking_context: this.chunking_context,
            intermediate_output_path: this.intermediate_output_path,
        }
        .cell())
    }
}

#[turbo_tasks::value_impl]
impl NodeEntry for SsrEntry {
    #[turbo_tasks::function]
    fn entry(self_vc: SsrEntryVc, _data: Value<ContentSourceData>) -> NodeRenderingEntryVc {
        // Call without being keyed by data
        self_vc.entry()
    }
}

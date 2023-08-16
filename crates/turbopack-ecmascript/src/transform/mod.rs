mod server_to_client_proxy;

use std::{path::Path, sync::Arc};

use anyhow::Result;
use next_transform_dynamic::{next_dynamic, NextDynamicMode};
use next_transform_strip_page_exports::{next_transform_strip_page_exports, ExportFilter};
use serde::{Deserialize, Serialize};
use swc_core::{
    base::SwcComments,
    common::{chain, util::take::Take, FileName, Mark, SourceMap},
    ecma::{
        ast::{Module, ModuleItem, Program},
        atoms::JsWord,
        preset_env::{self, Targets},
        transforms::{
            base::{feature::FeatureFlag, helpers::inject_helpers, resolver, Assumptions},
            react::react,
        },
        visit::{FoldWith, VisitMutWith},
    },
};
use turbo_tasks::{
    primitives::{OptionStringVc, StringVc, StringsVc},
    trace::TraceRawVcs,
};
use turbo_tasks_fs::{json::parse_json_with_source_context, FileSystemPathVc};
use turbopack_core::environment::EnvironmentVc;

use self::server_to_client_proxy::{create_proxy_module, is_client_module};

#[derive(
    Debug, Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash, Serialize, Deserialize, TraceRawVcs,
)]
pub enum NextJsPageExportFilter {
    /// Strip all data exports (getServerSideProps,
    /// getStaticProps, getStaticPaths exports.) and their unique dependencies.
    StripDataExports,
    /// Strip default export and all its unique dependencies.
    StripDefaultExport,
}

impl From<NextJsPageExportFilter> for ExportFilter {
    fn from(val: NextJsPageExportFilter) -> Self {
        match val {
            NextJsPageExportFilter::StripDataExports => ExportFilter::StripDataExports,
            NextJsPageExportFilter::StripDefaultExport => ExportFilter::StripDefaultExport,
        }
    }
}

#[turbo_tasks::value(serialization = "auto_for_input")]
#[derive(PartialOrd, Ord, Hash, Debug, Copy, Clone)]
pub enum EcmascriptInputTransform {
    ClientDirective(StringVc),
    CommonJs,
    Custom,
    Emotion,
    /// This enables a Next.js transform which will eliminate some exports
    /// from a page file, as well as any imports exclusively used by these
    /// exports.
    ///
    /// It also provides diagnostics for improper use of `getServerSideProps`.
    NextJsStripPageExports(NextJsPageExportFilter),
    /// Enables the Next.js transform for next/dynamic.
    NextJsDynamic {
        is_development: bool,
        is_server: bool,
        is_server_components: bool,
        pages_dir: Option<FileSystemPathVc>,
    },
    NextJsFont(StringsVc),
    PresetEnv(EnvironmentVc),
    React {
        #[serde(default)]
        refresh: bool,
        // swc.jsc.transform.react.importSource
        import_source: OptionStringVc,
        // swc.jsc.transform.react.runtime,
        runtime: OptionStringVc,
    },
    StyledComponents,
    StyledJsx,
    // These options are subset of swc_core::ecma::transforms::typescript::Config, but
    // it doesn't derive `Copy` so repeating values in here
    TypeScript {
        #[serde(default)]
        use_define_for_class_fields: bool,
    },
}

#[turbo_tasks::value(transparent, serialization = "auto_for_input")]
#[derive(Debug, PartialOrd, Ord, Hash, Clone)]
pub struct EcmascriptInputTransforms(Vec<EcmascriptInputTransform>);

#[turbo_tasks::value_impl]
impl EcmascriptInputTransformsVc {
    #[turbo_tasks::function]
    pub async fn extend(self, other: EcmascriptInputTransformsVc) -> Result<Self> {
        let mut transforms = self.await?.clone_value();
        transforms.extend(&*other.await?);
        Ok(EcmascriptInputTransformsVc::cell(transforms))
    }
}

pub struct TransformContext<'a> {
    pub comments: &'a SwcComments,
    pub top_level_mark: Mark,
    pub unresolved_mark: Mark,
    pub source_map: &'a Arc<SourceMap>,
    pub file_path_str: &'a str,
    pub file_name_str: &'a str,
    pub file_name_hash: u128,
}

impl EcmascriptInputTransform {
    pub async fn apply(
        &self,
        program: &mut Program,
        &TransformContext {
            comments,
            source_map,
            top_level_mark,
            unresolved_mark,
            file_path_str,
            file_name_str,
            file_name_hash,
        }: &TransformContext<'_>,
    ) -> Result<()> {
        match *self {
            EcmascriptInputTransform::React {
                refresh,
                import_source,
                runtime,
            } => {
                use swc_core::ecma::transforms::react::{Options, Runtime};
                let runtime = if let Some(runtime) = &*runtime.await? {
                    match runtime.as_str() {
                        "classic" => Runtime::Classic,
                        "automatic" => Runtime::Automatic,
                        _ => {
                            return Err(anyhow::anyhow!(
                                "Invalid value for swc.jsc.transform.react.runtime: {}",
                                runtime
                            ))
                        }
                    }
                } else {
                    Runtime::Automatic
                };

                let config = Options {
                    runtime: Some(runtime),
                    development: Some(true),
                    import_source: (&*import_source.await?).clone(),
                    refresh: if refresh {
                        Some(swc_core::ecma::transforms::react::RefreshOptions {
                            ..Default::default()
                        })
                    } else {
                        None
                    },
                    ..Default::default()
                };

                program.visit_mut_with(&mut react(
                    source_map.clone(),
                    Some(comments.clone()),
                    config,
                    top_level_mark,
                ));
            }
            EcmascriptInputTransform::CommonJs => {
                program.visit_mut_with(&mut swc_core::ecma::transforms::module::common_js(
                    unresolved_mark,
                    swc_core::ecma::transforms::module::util::Config {
                        allow_top_level_this: true,
                        import_interop: Some(
                            swc_core::ecma::transforms::module::util::ImportInterop::Swc,
                        ),
                        ..Default::default()
                    },
                    swc_core::ecma::transforms::base::feature::FeatureFlag::all(),
                    Some(comments.clone()),
                ));
            }
            EcmascriptInputTransform::Emotion => {
                let p = std::mem::replace(program, Program::Module(Module::dummy()));
                *program = p.fold_with(&mut swc_emotion::emotion(
                    Default::default(),
                    Path::new(file_name_str),
                    source_map.clone(),
                    comments.clone(),
                ))
            }
            EcmascriptInputTransform::PresetEnv(env) => {
                let versions = env.runtime_versions().await?;
                let config = swc_core::ecma::preset_env::Config {
                    targets: Some(Targets::Versions(*versions)),
                    mode: None, // Don't insert core-js polyfills
                    ..Default::default()
                };

                let module_program = unwrap_module_program(program);

                *program = module_program.fold_with(&mut chain!(
                    preset_env::preset_env(
                        top_level_mark,
                        Some(comments.clone()),
                        config,
                        Assumptions::default(),
                        &mut FeatureFlag::empty(),
                    ),
                    inject_helpers(unresolved_mark),
                ));
            }
            EcmascriptInputTransform::StyledComponents => {
                program.visit_mut_with(&mut styled_components::styled_components(
                    FileName::Anon,
                    file_name_hash,
                    parse_json_with_source_context("{}")?,
                ));
            }
            EcmascriptInputTransform::StyledJsx => {
                // Modeled after https://github.com/swc-project/plugins/blob/ae735894cdb7e6cfd776626fe2bc580d3e80fed9/packages/styled-jsx/src/lib.rs
                let real_program = std::mem::replace(program, Program::Module(Module::dummy()));
                *program = real_program.fold_with(&mut styled_jsx::visitor::styled_jsx(
                    source_map.clone(),
                    // styled_jsx don't really use that in a relevant way
                    FileName::Anon,
                ));
            }
            EcmascriptInputTransform::TypeScript {
                use_define_for_class_fields,
            } => {
                use swc_core::ecma::transforms::typescript::{strip_with_config, Config};
                let config = Config {
                    use_define_for_class_fields,
                    ..Default::default()
                };
                program.visit_mut_with(&mut strip_with_config(config, top_level_mark));
            }
            EcmascriptInputTransform::ClientDirective(transition_name) => {
                let transition_name = &*transition_name.await?;
                if is_client_module(program) {
                    *program = create_proxy_module(transition_name, &format!("./{file_name_str}"));
                    program.visit_mut_with(&mut resolver(unresolved_mark, top_level_mark, false));
                }
            }
            EcmascriptInputTransform::NextJsStripPageExports(export_type) => {
                // TODO(alexkirsz) Connect the eliminated_packages to telemetry.
                let eliminated_packages = Default::default();

                let module_program = unwrap_module_program(program);

                *program = module_program.fold_with(&mut next_transform_strip_page_exports(
                    export_type.into(),
                    eliminated_packages,
                ));
            }
            EcmascriptInputTransform::NextJsDynamic {
                is_development,
                is_server,
                is_server_components,
                pages_dir,
            } => {
                let module_program = unwrap_module_program(program);

                let pages_dir = if let Some(pages_dir) = pages_dir {
                    Some(pages_dir.await?.path.clone().into())
                } else {
                    None
                };

                *program = module_program.fold_with(&mut next_dynamic(
                    is_development,
                    is_server,
                    is_server_components,
                    NextDynamicMode::Turbo,
                    FileName::Real(file_path_str.into()),
                    pages_dir,
                ));
            }
            EcmascriptInputTransform::NextJsFont(font_loaders_vc) => {
                let mut font_loaders = vec![];
                for loader in &(*font_loaders_vc.await?) {
                    font_loaders.push(std::convert::Into::<JsWord>::into(&**loader));
                }
                let mut next_font = next_font::next_font_loaders(next_font::Config {
                    font_loaders,
                    relative_file_path_from_root: file_name_str.into(),
                });

                program.visit_mut_with(&mut next_font);
            }
            EcmascriptInputTransform::Custom => todo!(),
        }
        Ok(())
    }
}

pub fn remove_shebang(program: &mut Program) {
    match program {
        Program::Module(m) => {
            m.shebang = None;
        }
        Program::Script(s) => {
            s.shebang = None;
        }
    }
}

fn unwrap_module_program(program: &mut Program) -> Program {
    match program {
        Program::Module(module) => Program::Module(module.take()),
        Program::Script(s) => Program::Module(Module {
            span: s.span,
            body: s
                .body
                .iter()
                .map(|stmt| ModuleItem::Stmt(stmt.clone()))
                .collect(),
            shebang: s.shebang.clone(),
        }),
    }
}

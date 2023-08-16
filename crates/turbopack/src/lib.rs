#![feature(box_patterns)]
#![feature(box_syntax)]
#![feature(trivial_bounds)]
#![feature(min_specialization)]
#![feature(map_try_insert)]
#![feature(option_get_or_insert_default)]
#![feature(once_cell)]
#![feature(hash_set_entry)]
#![recursion_limit = "256"]

use std::{
    collections::{HashMap, HashSet},
    mem::swap,
};

use anyhow::Result;
use css::{CssModuleAssetVc, ModuleCssModuleAssetVc};
use ecmascript::{
    typescript::resolve::TypescriptTypesAssetReferenceVc, EcmascriptModuleAssetType,
    EcmascriptModuleAssetVc,
};
use graph::{aggregate, AggregatedGraphNodeContent, AggregatedGraphVc};
use lazy_static::lazy_static;
use module_options::{
    ModuleOptionsContextVc, ModuleOptionsVc, ModuleRuleEffect, ModuleType, ModuleTypeVc,
};
pub use resolve::resolve_options;
use turbo_tasks::{
    primitives::{BoolVc, StringVc},
    CompletionVc, Value,
};
use turbo_tasks_fs::FileSystemPathVc;
use turbopack_core::{
    asset::{Asset, AssetVc},
    compile_time_info::CompileTimeInfoVc,
    context::{AssetContext, AssetContextVc},
    ident::AssetIdentVc,
    issue::{unsupported_module::UnsupportedModuleIssue, Issue, IssueVc},
    reference::all_referenced_assets,
    reference_type::ReferenceType,
    resolve::{
        options::ResolveOptionsVc,
        origin::PlainResolveOriginVc,
        parse::{Request, RequestVc},
        pattern::Pattern,
        resolve, ResolveResultVc,
    },
};

use crate::transition::Transition;

pub mod condition;
pub mod evaluate_context;
mod graph;
pub mod module_options;
pub mod rebase;
pub mod resolve;
pub mod resolve_options_context;
pub mod transition;

pub use turbopack_css as css;
pub use turbopack_ecmascript as ecmascript;
use turbopack_json::JsonModuleAssetVc;
use turbopack_mdx::MdxModuleAssetVc;
use turbopack_static::StaticModuleAssetVc;

use self::{
    resolve_options_context::ResolveOptionsContextVc,
    transition::{TransitionVc, TransitionsByNameVc},
};

lazy_static! {
    static ref UNSUPPORTED_PACKAGES: HashSet<String> = ["@vercel/og".to_owned()].into();
    static ref UNSUPPORTED_PACKAGE_PATHS: HashSet<(String, String)> =
        [("@next/font".to_owned(), "/local".to_owned())].into();
}

#[turbo_tasks::value]
struct ModuleIssue {
    ident: AssetIdentVc,
    title: StringVc,
    description: StringVc,
}

#[turbo_tasks::value_impl]
impl Issue for ModuleIssue {
    #[turbo_tasks::function]
    fn category(&self) -> StringVc {
        StringVc::cell("other".to_string())
    }

    #[turbo_tasks::function]
    fn context(&self) -> FileSystemPathVc {
        self.ident.path()
    }

    #[turbo_tasks::function]
    fn title(&self) -> StringVc {
        self.title
    }

    #[turbo_tasks::function]
    fn description(&self) -> StringVc {
        self.description
    }
}

#[turbo_tasks::function]
async fn apply_module_type(
    source: AssetVc,
    context: ModuleAssetContextVc,
    module_type: ModuleTypeVc,
) -> Result<AssetVc> {
    Ok(match &*module_type.await? {
        ModuleType::Ecmascript(transforms) => EcmascriptModuleAssetVc::new(
            source,
            context.into(),
            Value::new(EcmascriptModuleAssetType::Ecmascript),
            *transforms,
            context.compile_time_info(),
        )
        .into(),
        ModuleType::Typescript(transforms) => EcmascriptModuleAssetVc::new(
            source,
            context.into(),
            Value::new(EcmascriptModuleAssetType::Typescript),
            *transforms,
            context.compile_time_info(),
        )
        .into(),
        ModuleType::TypescriptWithTypes(transforms) => EcmascriptModuleAssetVc::new(
            source,
            context.with_types_resolving_enabled().into(),
            Value::new(EcmascriptModuleAssetType::TypescriptWithTypes),
            *transforms,
            context.compile_time_info(),
        )
        .into(),
        ModuleType::TypescriptDeclaration(transforms) => EcmascriptModuleAssetVc::new(
            source,
            context.with_types_resolving_enabled().into(),
            Value::new(EcmascriptModuleAssetType::TypescriptDeclaration),
            *transforms,
            context.compile_time_info(),
        )
        .into(),
        ModuleType::Json => JsonModuleAssetVc::new(source).into(),
        ModuleType::Raw => source,
        ModuleType::Css(transforms) => {
            CssModuleAssetVc::new(source, context.into(), *transforms).into()
        }
        ModuleType::CssModule(transforms) => {
            ModuleCssModuleAssetVc::new(source, context.into(), *transforms).into()
        }
        ModuleType::Static => StaticModuleAssetVc::new(source, context.into()).into(),
        ModuleType::Mdx(transforms) => {
            MdxModuleAssetVc::new(source, context.into(), *transforms).into()
        }
        ModuleType::Custom(_) => todo!(),
    })
}

#[turbo_tasks::function]
async fn module(
    source: AssetVc,
    context: ModuleAssetContextVc,
    reference_type: Value<ReferenceType>,
) -> Result<AssetVc> {
    let ident = source.ident().resolve().await?;
    let options = ModuleOptionsVc::new(ident.path().parent(), context.module_options_context());

    let reference_type = reference_type.into_value();
    let mut current_source = source;
    let mut current_module_type = None;
    for rule in options.await?.rules.iter() {
        if rule
            .matches(source, &*ident.path().await?, &reference_type)
            .await?
        {
            for effect in rule.effects() {
                match effect {
                    ModuleRuleEffect::SourceTransforms(transforms) => {
                        current_source = transforms.transform(current_source);
                        if current_source.ident().resolve().await? != ident {
                            // The ident has been changed, so we need to apply new rules.
                            return Ok(module(current_source, context, Value::new(reference_type)));
                        }
                    }
                    ModuleRuleEffect::ModuleType(module) => {
                        current_module_type = Some(*module);
                    }
                    ModuleRuleEffect::AddEcmascriptTransforms(additional_transforms) => {
                        current_module_type = match current_module_type {
                            Some(ModuleType::Ecmascript(transforms)) => Some(
                                ModuleType::Ecmascript(transforms.extend(*additional_transforms)),
                            ),
                            Some(ModuleType::Typescript(transforms)) => Some(
                                ModuleType::Typescript(transforms.extend(*additional_transforms)),
                            ),
                            Some(ModuleType::TypescriptWithTypes(transforms)) => {
                                Some(ModuleType::TypescriptWithTypes(
                                    transforms.extend(*additional_transforms),
                                ))
                            }
                            Some(module_type) => {
                                ModuleIssue {
                                    ident,
                                    title: StringVc::cell("Invalid module type".to_string()),
                                    description: StringVc::cell(
                                        "The module type must be Ecmascript or Typescript to add \
                                         Ecmascript transforms"
                                            .to_string(),
                                    ),
                                }
                                .cell()
                                .as_issue()
                                .emit();
                                Some(module_type)
                            }
                            None => {
                                ModuleIssue {
                                    ident,
                                    title: StringVc::cell("Missing module type".to_string()),
                                    description: StringVc::cell(
                                        "The module type effect must be applied before adding \
                                         Ecmascript transforms"
                                            .to_string(),
                                    ),
                                }
                                .cell()
                                .as_issue()
                                .emit();
                                None
                            }
                        };
                    }
                    ModuleRuleEffect::Custom => {
                        todo!("Custom module rule effects are not yet supported");
                    }
                }
            }
        }
    }

    let module_type = current_module_type.unwrap_or(ModuleType::Raw).cell();

    Ok(apply_module_type(current_source, context, module_type))
}

#[derive(Debug)]
#[turbo_tasks::value]
pub struct ModuleAssetContext {
    transitions: TransitionsByNameVc,
    compile_time_info: CompileTimeInfoVc,
    module_options_context: ModuleOptionsContextVc,
    resolve_options_context: ResolveOptionsContextVc,
    transition: Option<TransitionVc>,
}

#[turbo_tasks::value_impl]
impl ModuleAssetContextVc {
    #[turbo_tasks::function]
    pub fn new(
        transitions: TransitionsByNameVc,
        compile_time_info: CompileTimeInfoVc,
        module_options_context: ModuleOptionsContextVc,
        resolve_options_context: ResolveOptionsContextVc,
    ) -> Self {
        Self::cell(ModuleAssetContext {
            transitions,
            compile_time_info,
            module_options_context,
            resolve_options_context,
            transition: None,
        })
    }

    #[turbo_tasks::function]
    pub fn new_transition(
        transitions: TransitionsByNameVc,
        compile_time_info: CompileTimeInfoVc,
        module_options_context: ModuleOptionsContextVc,
        resolve_options_context: ResolveOptionsContextVc,
        transition: TransitionVc,
    ) -> Self {
        Self::cell(ModuleAssetContext {
            transitions,
            compile_time_info,
            module_options_context,
            resolve_options_context,
            transition: Some(transition),
        })
    }

    #[turbo_tasks::function]
    pub async fn module_options_context(self) -> Result<ModuleOptionsContextVc> {
        Ok(self.await?.module_options_context)
    }

    #[turbo_tasks::function]
    pub async fn is_types_resolving_enabled(self) -> Result<BoolVc> {
        let context = self.await?.resolve_options_context.await?;
        Ok(BoolVc::cell(
            context.enable_types && context.enable_typescript,
        ))
    }

    #[turbo_tasks::function]
    pub async fn with_types_resolving_enabled(self) -> Result<ModuleAssetContextVc> {
        if *self.is_types_resolving_enabled().await? {
            return Ok(self);
        }
        let this = self.await?;
        let resolve_options_context = this
            .resolve_options_context
            .with_types_enabled()
            .resolve()
            .await?;
        Ok(ModuleAssetContextVc::new(
            this.transitions,
            this.compile_time_info,
            this.module_options_context,
            resolve_options_context,
        ))
    }
}

#[turbo_tasks::value_impl]
impl AssetContext for ModuleAssetContext {
    #[turbo_tasks::function]
    fn compile_time_info(&self) -> CompileTimeInfoVc {
        self.compile_time_info
    }

    #[turbo_tasks::function]
    async fn resolve_options(
        &self,
        origin_path: FileSystemPathVc,
        _reference_type: Value<ReferenceType>,
    ) -> Result<ResolveOptionsVc> {
        // TODO move `apply_commonjs/esm_resolve_options` etc. to here
        Ok(resolve_options(
            origin_path.parent().resolve().await?,
            self.resolve_options_context,
        ))
    }

    #[turbo_tasks::function]
    async fn resolve_asset(
        self_vc: ModuleAssetContextVc,
        origin_path: FileSystemPathVc,
        request: RequestVc,
        resolve_options: ResolveOptionsVc,
        reference_type: Value<ReferenceType>,
    ) -> Result<ResolveResultVc> {
        warn_on_unsupported_modules(request, origin_path).await?;

        let context_path = origin_path.parent().resolve().await?;

        let result = resolve(context_path, request, resolve_options);
        let result = self_vc.process_resolve_result(result, reference_type);

        if *self_vc.is_types_resolving_enabled().await? {
            let types_reference = TypescriptTypesAssetReferenceVc::new(
                PlainResolveOriginVc::new(self_vc.into(), origin_path).into(),
                request,
            );

            result.add_reference(types_reference.into());
        }

        Ok(result)
    }

    #[turbo_tasks::function]
    async fn process_resolve_result(
        self_vc: ModuleAssetContextVc,
        result: ResolveResultVc,
        reference_type: Value<ReferenceType>,
    ) -> Result<ResolveResultVc> {
        Ok(result
            .await?
            .map(
                |a| self_vc.process(a, reference_type.clone()).resolve(),
                |i| async move { Ok(i) },
            )
            .await?
            .into())
    }

    #[turbo_tasks::function]
    async fn process(
        self_vc: ModuleAssetContextVc,
        asset: AssetVc,
        reference_type: Value<ReferenceType>,
    ) -> Result<AssetVc> {
        let this = self_vc.await?;
        if let Some(transition) = this.transition {
            let asset = transition.process_source(asset);
            let compile_time_info = transition.process_compile_time_info(this.compile_time_info);
            let module_options_context =
                transition.process_module_options_context(this.module_options_context);
            let resolve_options_context =
                transition.process_resolve_options_context(this.resolve_options_context);
            let context = ModuleAssetContextVc::new(
                this.transitions,
                compile_time_info,
                module_options_context,
                resolve_options_context,
            );
            let m = module(asset, context, reference_type);
            Ok(transition.process_module(m, context))
        } else {
            let context = ModuleAssetContextVc::new(
                this.transitions,
                this.compile_time_info,
                this.module_options_context,
                this.resolve_options_context,
            );
            Ok(module(asset, context, reference_type))
        }
    }

    #[turbo_tasks::function]
    async fn with_transition(&self, transition: &str) -> Result<AssetContextVc> {
        Ok(
            if let Some(transition) = self.transitions.await?.get(transition) {
                ModuleAssetContextVc::new_transition(
                    self.transitions,
                    self.compile_time_info,
                    self.module_options_context,
                    self.resolve_options_context,
                    *transition,
                )
                .into()
            } else {
                // TODO report issue
                ModuleAssetContextVc::new(
                    self.transitions,
                    self.compile_time_info,
                    self.module_options_context,
                    self.resolve_options_context,
                )
                .into()
            },
        )
    }
}

#[turbo_tasks::function]
pub async fn emit_with_completion(asset: AssetVc, output_dir: FileSystemPathVc) -> CompletionVc {
    emit_assets_aggregated(asset, output_dir)
}

#[turbo_tasks::function]
async fn emit_assets_aggregated(asset: AssetVc, output_dir: FileSystemPathVc) -> CompletionVc {
    let aggregated = aggregate(asset);
    emit_aggregated_assets(aggregated, output_dir)
}

#[turbo_tasks::function]
async fn emit_aggregated_assets(
    aggregated: AggregatedGraphVc,
    output_dir: FileSystemPathVc,
) -> Result<CompletionVc> {
    Ok(match &*aggregated.content().await? {
        AggregatedGraphNodeContent::Asset(asset) => emit_asset_into_dir(*asset, output_dir),
        AggregatedGraphNodeContent::Children(children) => {
            for aggregated in children {
                emit_aggregated_assets(*aggregated, output_dir).await?;
            }
            CompletionVc::new()
        }
    })
}

#[turbo_tasks::function]
pub async fn emit_asset(asset: AssetVc) -> CompletionVc {
    asset.content().write(asset.ident().path())
}

#[turbo_tasks::function]
pub async fn emit_asset_into_dir(
    asset: AssetVc,
    output_dir: FileSystemPathVc,
) -> Result<CompletionVc> {
    let dir = &*output_dir.await?;
    Ok(if asset.ident().path().await?.is_inside(dir) {
        emit_asset(asset)
    } else {
        CompletionVc::new()
    })
}

#[turbo_tasks::value(shared)]
struct ReferencesList {
    referenced_by: HashMap<AssetVc, HashSet<AssetVc>>,
}

#[turbo_tasks::function]
async fn compute_back_references(aggregated: AggregatedGraphVc) -> Result<ReferencesListVc> {
    Ok(match &*aggregated.content().await? {
        AggregatedGraphNodeContent::Asset(asset) => {
            let mut referenced_by = HashMap::new();
            for reference in all_referenced_assets(*asset).await?.iter() {
                referenced_by.insert(*reference, [*asset].into_iter().collect());
            }
            ReferencesList { referenced_by }.into()
        }
        AggregatedGraphNodeContent::Children(children) => {
            let mut referenced_by = HashMap::<AssetVc, HashSet<AssetVc>>::new();
            let lists = children
                .iter()
                .map(|child| compute_back_references(*child))
                .collect::<Vec<_>>();
            for list in lists {
                for (key, values) in list.await?.referenced_by.iter() {
                    if let Some(set) = referenced_by.get_mut(key) {
                        for value in values {
                            set.insert(*value);
                        }
                    } else {
                        referenced_by.insert(*key, values.clone());
                    }
                }
            }
            ReferencesList { referenced_by }.into()
        }
    })
}

#[turbo_tasks::function]
async fn top_references(list: ReferencesListVc) -> Result<ReferencesListVc> {
    let list = list.await?;
    const N: usize = 5;
    let mut top = Vec::<(&AssetVc, &HashSet<AssetVc>)>::new();
    for tuple in list.referenced_by.iter() {
        let mut current = tuple;
        for item in &mut top {
            if item.1.len() < tuple.1.len() {
                swap(item, &mut current);
            }
        }
        if top.len() < N {
            top.push(current);
        }
    }
    Ok(ReferencesList {
        referenced_by: top
            .into_iter()
            .map(|(asset, set)| (*asset, set.clone()))
            .collect(),
    }
    .into())
}

async fn warn_on_unsupported_modules(
    request: RequestVc,
    origin_path: FileSystemPathVc,
) -> Result<()> {
    if let Request::Module {
        module,
        path,
        query: _,
    } = &*request.await?
    {
        // Warn if the package is known not to be supported by Turbopack at the moment.
        if UNSUPPORTED_PACKAGES.contains(module) {
            UnsupportedModuleIssue {
                context: origin_path,
                package: module.into(),
                package_path: None,
            }
            .cell()
            .as_issue()
            .emit();
        }

        if let Pattern::Constant(path) = path {
            if UNSUPPORTED_PACKAGE_PATHS.contains(&(module.to_string(), path.to_owned())) {
                UnsupportedModuleIssue {
                    context: origin_path,
                    package: module.into(),
                    package_path: Some(path.to_owned()),
                }
                .cell()
                .as_issue()
                .emit();
            }
        }
    }

    Ok(())
}

pub fn register() {
    turbo_tasks::register();
    turbo_tasks_fs::register();
    turbopack_core::register();
    turbopack_css::register();
    turbopack_ecmascript::register();
    turbopack_node::register();
    turbopack_env::register();
    turbopack_mdx::register();
    turbopack_json::register();
    turbopack_static::register();
    include!(concat!(env!("OUT_DIR"), "/register.rs"));
}

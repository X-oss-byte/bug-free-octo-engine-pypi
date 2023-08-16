use std::{collections::HashMap, fmt::Write};

use anyhow::Result;
use serde_json::Value as JsonValue;
use turbo_tasks::{Value, ValueDefault, ValueToString, Vc};
use turbo_tasks_fs::{FileContent, FileJsonContent, FileSystemPath};
use turbopack_core::{
    asset::Asset,
    context::AssetContext,
    file_source::FileSource,
    ident::AssetIdent,
    issue::{Issue, IssueExt, IssueSeverity, OptionIssueSource},
    reference::ModuleReference,
    reference_type::{ReferenceType, TypeScriptReferenceSubType},
    resolve::{
        handle_resolve_error,
        node::node_cjs_resolve_options,
        options::{
            ConditionValue, ImportMap, ImportMapping, ResolveInPackage, ResolveIntoPackage,
            ResolveModules, ResolveOptions,
        },
        origin::{ResolveOrigin, ResolveOriginExt},
        parse::Request,
        pattern::{Pattern, QueryMap},
        resolve, AliasPattern, ModuleResolveResult,
    },
    source::{OptionSource, Source},
};
#[turbo_tasks::value(shared)]
pub struct TsConfigIssue {
    pub severity: Vc<IssueSeverity>,
    pub source_ident: Vc<AssetIdent>,
    pub message: Vc<String>,
}

#[turbo_tasks::function]
async fn json_only(resolve_options: Vc<ResolveOptions>) -> Result<Vc<ResolveOptions>> {
    let mut opts = resolve_options.await?.clone_value();
    opts.extensions = vec![".json".to_string()];
    Ok(opts.cell())
}

pub async fn read_tsconfigs(
    mut data: Vc<FileContent>,
    mut tsconfig: Vc<Box<dyn Source>>,
    resolve_options: Vc<ResolveOptions>,
) -> Result<Vec<(Vc<FileJsonContent>, Vc<Box<dyn Source>>)>> {
    let mut configs = Vec::new();
    let resolve_options = json_only(resolve_options);
    loop {
        let parsed_data = data.parse_json_with_comments();
        match &*parsed_data.await? {
            FileJsonContent::Unparseable(e) => {
                let mut message = "tsconfig is not parseable: invalid JSON: ".to_string();
                if let FileContent::Content(content) = &*data.await? {
                    let text = content.content().to_str()?;
                    e.write_with_content(&mut message, text.as_ref())?;
                } else {
                    write!(message, "{}", e)?;
                }
                TsConfigIssue {
                    severity: IssueSeverity::Error.into(),
                    source_ident: tsconfig.ident(),
                    message: Vc::cell(message),
                }
                .cell()
                .emit();
            }
            FileJsonContent::NotFound => {
                TsConfigIssue {
                    severity: IssueSeverity::Error.into(),
                    source_ident: tsconfig.ident(),
                    message: Vc::cell("tsconfig not found".into()),
                }
                .cell()
                .emit();
            }
            FileJsonContent::Content(json) => {
                configs.push((parsed_data, tsconfig));
                if let Some(extends) = json["extends"].as_str() {
                    let resolved = resolve_extends(tsconfig, extends, resolve_options).await?;
                    if let Some(source) = *resolved.await? {
                        data = source.content().file_content();
                        tsconfig = source;
                        continue;
                    } else {
                        TsConfigIssue {
                            severity: IssueSeverity::Error.into(),
                            source_ident: tsconfig.ident(),
                            message: Vc::cell("extends doesn't resolve correctly".to_string()),
                        }
                        .cell()
                        .emit();
                    }
                }
            }
        }
        break;
    }
    Ok(configs)
}

/// Resolves tsconfig files according to TS's implementation:
/// https://github.com/microsoft/TypeScript/blob/611a912d/src/compiler/commandLineParser.ts#L3294-L3326
async fn resolve_extends(
    tsconfig: Vc<Box<dyn Source>>,
    extends: &str,
    resolve_options: Vc<ResolveOptions>,
) -> Result<Vc<OptionSource>> {
    let context = tsconfig.ident().path().parent();
    let request = Request::parse_string(extends.to_string());

    // TS's resolution is weird, and has special behavior for different import
    // types. There might be multiple alternatives like
    // "some/path/node_modules/xyz/abc.json" and "some/node_modules/xyz/abc.json".
    // We only want to use the first one.
    match &*request.await? {
        // TS has special behavior for "rooted" paths (absolute paths):
        // https://github.com/microsoft/TypeScript/blob/611a912d/src/compiler/commandLineParser.ts#L3303-L3313
        Request::Windows { path: Pattern::Constant(path) } |
        // Server relative is treated as absolute
        Request::ServerRelative { path: Pattern::Constant(path) } => {
            resolve_extends_rooted_or_relative(context, request, resolve_options, path).await
        }

        // TS has special behavior for (explicitly) './' and '../', but not '.' nor '..':
        // https://github.com/microsoft/TypeScript/blob/611a912d/src/compiler/commandLineParser.ts#L3303-L3313
        Request::Relative {
            path: Pattern::Constant(path),
            ..
        } if path.starts_with("./") || path.starts_with("../") => {
            resolve_extends_rooted_or_relative(context, request, resolve_options, path).await
        }

        // An empty extends is treated as "./tsconfig"
        Request::Empty => {
            let request = Request::parse_string("./tsconfig".to_string());
            Ok(resolve(context, request, resolve_options).first_source())
        }

        // All other types are treated as module imports, and potentially joined with
        // "tsconfig.json". This includes "relative" imports like '.' and '..'.
        _ => {
            let mut result = resolve(context, request, resolve_options).first_source();
            if result.await?.is_none() {
                let request = Request::parse_string(format!("{extends}/tsconfig"));
                result = resolve(context, request, resolve_options).first_source();
            }
            Ok(result)
        }
    }
}

async fn resolve_extends_rooted_or_relative(
    context: Vc<FileSystemPath>,
    request: Vc<Request>,
    resolve_options: Vc<ResolveOptions>,
    path: &str,
) -> Result<Vc<OptionSource>> {
    let mut result = resolve(context, request, resolve_options).first_source();

    // If the file doesn't end with ".json" and we can't find the file, then we have
    // to try again with it.
    // https://github.com/microsoft/TypeScript/blob/611a912d/src/compiler/commandLineParser.ts#L3305
    if !path.ends_with(".json") && result.await?.is_none() {
        let request = Request::parse_string(format!("{path}.json"));
        result = resolve(context, request, resolve_options).first_source();
    }
    Ok(result)
}

type Config = (Vc<FileJsonContent>, Vc<Box<dyn Source>>);

pub async fn read_from_tsconfigs<T>(
    configs: &[Config],
    accessor: impl Fn(&JsonValue, Vc<Box<dyn Source>>) -> Option<T>,
) -> Result<Option<T>> {
    for (config, source) in configs.iter() {
        if let FileJsonContent::Content(json) = &*config.await? {
            if let Some(result) = accessor(json, *source) {
                return Ok(Some(result));
            }
        }
    }
    Ok(None)
}

/// Resolve options specific to tsconfig.json.
#[turbo_tasks::value]
#[derive(Default)]
pub struct TsConfigResolveOptions {
    base_url: Option<Vc<FileSystemPath>>,
    import_map: Option<Vc<ImportMap>>,
}

#[turbo_tasks::value_impl]
impl ValueDefault for TsConfigResolveOptions {
    #[turbo_tasks::function]
    fn value_default() -> Vc<Self> {
        Self::default().cell()
    }
}

/// Returns the resolve options
#[turbo_tasks::function]
pub async fn tsconfig_resolve_options(
    tsconfig: Vc<FileSystemPath>,
) -> Result<Vc<TsConfigResolveOptions>> {
    let configs = read_tsconfigs(
        tsconfig.read(),
        Vc::upcast(FileSource::new(tsconfig)),
        node_cjs_resolve_options(tsconfig.root()),
    )
    .await?;

    if configs.is_empty() {
        return Ok(Default::default());
    }

    let base_url = if let Some(base_url) = read_from_tsconfigs(&configs, |json, source| {
        json["compilerOptions"]["baseUrl"].as_str().map(|base_url| {
            source
                .ident()
                .path()
                .parent()
                .try_join(base_url.to_string())
        })
    })
    .await?
    {
        *base_url.await?
    } else {
        None
    };

    let mut all_paths = HashMap::new();
    for (content, source) in configs.iter().rev() {
        if let FileJsonContent::Content(json) = &*content.await? {
            if let JsonValue::Object(paths) = &json["compilerOptions"]["paths"] {
                let mut context = source.ident().path().parent();
                if let Some(base_url) = json["compilerOptions"]["baseUrl"].as_str() {
                    if let Some(new_context) = *context.try_join(base_url.to_string()).await? {
                        context = new_context;
                    }
                };
                for (key, value) in paths.iter() {
                    if let JsonValue::Array(vec) = value {
                        let entries = vec
                            .iter()
                            .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
                            .collect();
                        all_paths.insert(
                            key.to_string(),
                            ImportMapping::primary_alternatives(entries, Some(context)),
                        );
                    } else {
                        TsConfigIssue {
                            severity: IssueSeverity::Warning.cell(),
                            source_ident: source.ident(),
                            message: Vc::cell(format!(
                                "compilerOptions.paths[{key}] doesn't contains an array as \
                                 expected\n{key}: {value:#}",
                                key = serde_json::to_string(key)?,
                                value = value
                            )),
                        }
                        .cell()
                        .emit()
                    }
                }
            }
        }
    }

    let import_map = if !all_paths.is_empty() {
        let mut import_map = ImportMap::empty();
        for (key, value) in all_paths {
            import_map.insert_alias(AliasPattern::parse(key), value.into());
        }
        Some(import_map.cell())
    } else {
        None
    };

    Ok(TsConfigResolveOptions {
        base_url,
        import_map,
    }
    .cell())
}

#[turbo_tasks::function]
pub fn tsconfig() -> Vc<Vec<String>> {
    Vc::cell(vec![
        "tsconfig.json".to_string(),
        "jsconfig.json".to_string(),
    ])
}

#[turbo_tasks::function]
pub async fn apply_tsconfig_resolve_options(
    resolve_options: Vc<ResolveOptions>,
    tsconfig_resolve_options: Vc<TsConfigResolveOptions>,
) -> Result<Vc<ResolveOptions>> {
    let tsconfig_resolve_options = tsconfig_resolve_options.await?;
    let mut resolve_options = resolve_options.await?.clone_value();
    if let Some(base_url) = tsconfig_resolve_options.base_url {
        // We want to resolve in `compilerOptions.baseUrl` first, then in other
        // locations as a fallback.
        resolve_options
            .modules
            .insert(0, ResolveModules::Path(base_url));
    }
    if let Some(tsconfig_import_map) = tsconfig_resolve_options.import_map {
        resolve_options.import_map = Some(
            resolve_options
                .import_map
                .map(|import_map| import_map.extend(tsconfig_import_map))
                .unwrap_or(tsconfig_import_map),
        );
    }
    Ok(resolve_options.cell())
}

#[turbo_tasks::function]
pub async fn type_resolve(
    origin: Vc<Box<dyn ResolveOrigin>>,
    request: Vc<Request>,
) -> Result<Vc<ModuleResolveResult>> {
    let ty = Value::new(ReferenceType::TypeScript(
        TypeScriptReferenceSubType::Undefined,
    ));
    let context_path = origin.origin_path().parent();
    let options = origin.resolve_options(ty.clone());
    let options = apply_typescript_types_options(options);
    let types_request = if let Request::Module {
        module: m,
        path: p,
        query: _,
    } = &*request.await?
    {
        let m = if let Some(stripped) = m.strip_prefix('@') {
            stripped.replace('/', "__")
        } else {
            m.clone()
        };
        Some(Request::module(
            format!("@types/{m}"),
            Value::new(p.clone()),
            QueryMap::none(),
        ))
    } else {
        None
    };
    let context_path = context_path.resolve().await?;
    let result = if let Some(types_request) = types_request {
        let result1 = resolve(context_path, request, options);
        if !*result1.is_unresolveable().await? {
            result1
        } else {
            resolve(context_path, types_request, options)
        }
    } else {
        resolve(context_path, request, options)
    };
    let result = origin
        .asset_context()
        .process_resolve_result(result, ty.clone());
    handle_resolve_error(
        result,
        ty,
        origin.origin_path(),
        request,
        options,
        OptionIssueSource::none(),
        IssueSeverity::Error.cell(),
    )
    .await
}

#[turbo_tasks::value]
pub struct TypescriptTypesAssetReference {
    pub origin: Vc<Box<dyn ResolveOrigin>>,
    pub request: Vc<Request>,
}

#[turbo_tasks::value_impl]
impl ModuleReference for TypescriptTypesAssetReference {
    #[turbo_tasks::function]
    fn resolve_reference(&self) -> Vc<ModuleResolveResult> {
        type_resolve(self.origin, self.request)
    }
}

#[turbo_tasks::value_impl]
impl ValueToString for TypescriptTypesAssetReference {
    #[turbo_tasks::function]
    async fn to_string(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(format!(
            "typescript types {}",
            self.request.to_string().await?,
        )))
    }
}

impl TypescriptTypesAssetReference {
    pub fn new(origin: Vc<Box<dyn ResolveOrigin>>, request: Vc<Request>) -> Vc<Self> {
        Self::cell(TypescriptTypesAssetReference { origin, request })
    }
}

#[turbo_tasks::function]
async fn apply_typescript_types_options(
    resolve_options: Vc<ResolveOptions>,
) -> Result<Vc<ResolveOptions>> {
    let mut resolve_options = resolve_options.await?.clone_value();
    resolve_options.extensions = vec![".tsx".to_string(), ".ts".to_string(), ".d.ts".to_string()];
    resolve_options.into_package = resolve_options
        .into_package
        .drain(..)
        .filter_map(|into| {
            if let ResolveIntoPackage::ExportsField {
                mut conditions,
                unspecified_conditions,
            } = into
            {
                conditions.insert("types".to_string(), ConditionValue::Set);
                Some(ResolveIntoPackage::ExportsField {
                    conditions,
                    unspecified_conditions,
                })
            } else {
                None
            }
        })
        .collect();
    resolve_options
        .into_package
        .push(ResolveIntoPackage::MainField("types".to_string()));
    resolve_options
        .into_package
        .push(ResolveIntoPackage::Default("index".to_string()));
    for item in resolve_options.in_package.iter_mut() {
        if let ResolveInPackage::ImportsField { conditions, .. } = item {
            conditions.insert("types".to_string(), ConditionValue::Set);
        }
    }
    Ok(resolve_options.into())
}

#[turbo_tasks::value_impl]
impl Issue for TsConfigIssue {
    #[turbo_tasks::function]
    fn severity(&self) -> Vc<IssueSeverity> {
        self.severity
    }

    #[turbo_tasks::function]
    async fn title(&self) -> Result<Vc<String>> {
        Ok(Vc::cell(
            "An issue occurred while parsing a tsconfig.json file.".to_string(),
        ))
    }

    #[turbo_tasks::function]
    fn category(&self) -> Vc<String> {
        Vc::cell("typescript".to_string())
    }

    #[turbo_tasks::function]
    fn file_path(&self) -> Vc<FileSystemPath> {
        self.source_ident.path()
    }

    #[turbo_tasks::function]
    fn description(&self) -> Vc<String> {
        self.message
    }
}

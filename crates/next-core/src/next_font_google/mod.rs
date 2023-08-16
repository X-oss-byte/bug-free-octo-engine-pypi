use anyhow::{bail, Context, Result};
use indexmap::IndexMap;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use turbo_tasks::primitives::{OptionStringVc, OptionU16Vc, StringVc, U32Vc};
use turbo_tasks_fs::{json::parse_json_with_source_context, FileContent, FileSystemPathVc};
use turbo_tasks_hash::hash_xxh3_hash64;
use turbopack_core::{
    resolve::{
        options::{
            ImportMapResult, ImportMapResultVc, ImportMapping, ImportMappingReplacement,
            ImportMappingReplacementVc, ImportMappingVc,
        },
        parse::{Request, RequestVc},
        pattern::QueryMapVc,
        ResolveResult,
    },
    virtual_asset::VirtualAssetVc,
};
use turbopack_node::execution_context::ExecutionContextVc;

use self::options::FontWeights;
use crate::{
    embed_js::next_js_file_path,
    next_font_google::{
        options::FontDataEntry,
        util::{get_font_axes, get_stylesheet_url},
    },
};

pub(crate) mod options;
pub(crate) mod request;
mod util;

pub const GOOGLE_FONTS_STYLESHEET_URL: &str = "https://fonts.googleapis.com/css2";
static FONT_DATA: Lazy<FontData> = Lazy::new(|| {
    parse_json_with_source_context(include_str!("__generated__/font-data.json")).unwrap()
});

type FontData = IndexMap<String, FontDataEntry>;

#[turbo_tasks::value(shared)]
pub struct NextFontGoogleReplacer {
    project_path: FileSystemPathVc,
}

#[turbo_tasks::value_impl]
impl NextFontGoogleReplacerVc {
    #[turbo_tasks::function]
    pub fn new(project_path: FileSystemPathVc) -> Self {
        Self::cell(NextFontGoogleReplacer { project_path })
    }
}

#[turbo_tasks::value_impl]
impl ImportMappingReplacement for NextFontGoogleReplacer {
    #[turbo_tasks::function]
    fn replace(&self, _capture: &str) -> ImportMappingVc {
        ImportMapping::Ignore.into()
    }

    #[turbo_tasks::function]
    async fn result(&self, request: RequestVc) -> Result<ImportMapResultVc> {
        let request = &*request.await?;
        let Request::Module {
            module: _,
            path: _,
            query: query_vc
        } = request else {
            return Ok(ImportMapResult::NoEntry.into());
        };

        let query = &*query_vc.await?;
        let options = font_options_from_query_map(*query_vc);
        let properties =
            get_font_css_properties(get_scoped_font_family(*query_vc), options).await?;
        let js_asset = VirtualAssetVc::new(
                next_js_file_path("internal/font/google")
                    .join(&format!("{}.js", get_request_id(*query_vc).await?)),
                FileContent::Content(
                    formatdoc!(
                        r#"
                            import cssModule from "@vercel/turbopack-next/internal/font/google/cssmodule.module.css?{}";
                            const fontData = {{
                                className: cssModule.className,
                                style: {{
                                    fontFamily: "{}",
                                    {}{}
                                }},
                            }};

                            if (cssModule.variable != null) {{
                                fontData.variable = cssModule.variable;
                            }}

                            export default fontData;
                        "#,
                        // Pass along whichever options we received to the css handler
                        qstring::QString::new(query.as_ref().unwrap().iter().collect()),
                        properties.font_family.await?,
                        properties
                            .weight
                            .await?
                            .map(|w| format!("fontWeight: {},\n", w))
                            .unwrap_or_else(|| "".to_owned()),
                        properties
                            .style
                            .await?
                            .as_ref()
                            .map(|s| format!("fontStyle: \"{}\",\n", s))
                            .unwrap_or_else(|| "".to_owned()),
                    )
                    .into(),
                )
                .into(),
            );

        Ok(ImportMapResult::Result(ResolveResult::asset(js_asset.into()).into()).into())
    }
}

#[turbo_tasks::value(shared)]
pub struct NextFontGoogleCssModuleReplacer {
    project_path: FileSystemPathVc,
    execution_context: ExecutionContextVc,
}

#[turbo_tasks::value_impl]
impl NextFontGoogleCssModuleReplacerVc {
    #[turbo_tasks::function]
    pub fn new(project_path: FileSystemPathVc, execution_context: ExecutionContextVc) -> Self {
        Self::cell(NextFontGoogleCssModuleReplacer {
            project_path,
            execution_context,
        })
    }
}

#[turbo_tasks::value_impl]
impl ImportMappingReplacement for NextFontGoogleCssModuleReplacer {
    #[turbo_tasks::function]
    fn replace(&self, _capture: &str) -> ImportMappingVc {
        ImportMapping::Ignore.into()
    }

    #[turbo_tasks::function]
    async fn result(&self, request: RequestVc) -> Result<ImportMapResultVc> {
        let request = &*request.await?;
        let Request::Module {
            module: _,
            path: _,
            query: query_vc,
        } = request else {
            return Ok(ImportMapResult::NoEntry.into());
        };
        request.request();

        let options = font_options_from_query_map(*query_vc);
        let stylesheet_url = get_stylesheet_url_from_options(options);
        let scoped_font_family = get_scoped_font_family(*query_vc);
        let css_virtual_path = next_js_file_path("internal/font/google")
            .join(&format!("/{}.module.css", get_request_id(*query_vc).await?));

        // When running Next.js integration tests, use the mock data available in
        // process.env.NEXT_FONT_GOOGLE_MOCKED_RESPONSES instead of making real
        // requests to Google Fonts.
        #[cfg(feature = "__internal_nextjs_integration_test")]
        let stylesheet_str = get_mock_stylesheet(&stylesheet_url.await?, self.execution_context)
            .await?
            .map(StringVc::cell);

        #[cfg(not(feature = "__internal_nextjs_integration_test"))]
        let stylesheet_str = {
            use turbo_tasks_fetch::fetch;
            use turbopack_core::issue::IssueSeverity;

            let stylesheet_res = fetch(
                stylesheet_url,
                OptionStringVc::cell(Some(
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, \
                     like Gecko) Chrome/104.0.0.0 Safari/537.36"
                        .to_owned(),
                )),
            )
            .await?;

            match &*stylesheet_res {
                Ok(r) => Some(r.await?.body.to_string()),
                Err(err) => {
                    // Inform the user of the failure to retreive the stylesheet, but don't
                    // propagate this error. We don't want e.g. offline connections to prevent page
                    // renders during development. During production builds, however, this error
                    // should propagate.
                    //
                    // TODO(WEB-283): Use fallback in dev in this case
                    // TODO(WEB-293): Fail production builds (not dev) in this case
                    err.to_issue(IssueSeverity::Warning.into(), css_virtual_path)
                        .as_issue()
                        .emit();

                    None
                }
            }
        };

        let stylesheet = match stylesheet_str {
            Some(s) => Some(
                update_stylesheet(s, options, scoped_font_family)
                    .await?
                    .clone_value(),
            ),
            None => None,
        };

        let properties = get_font_css_properties(scoped_font_family, options).await?;
        let font_family = properties.font_family.await?;
        let css_asset = VirtualAssetVc::new(
            css_virtual_path,
            FileContent::Content(
                formatdoc!(
                    r#"
                        {}

                        .className {{
                            font-family: {};
                            {}{}
                        }}

                        {}
                        "#,
                    stylesheet.unwrap_or_else(|| "".to_owned()),
                    font_family,
                    properties
                        .weight
                        .await?
                        .map(|w| format!("font-weight: {};\n", w))
                        .unwrap_or_else(|| "".to_owned()),
                    properties
                        .style
                        .await?
                        .as_ref()
                        .map(|s| format!("font-style: {};\n", s))
                        .unwrap_or_else(|| "".to_owned()),
                    properties
                        .variable
                        .await?
                        .as_ref()
                        .map(|v| { format!(".variable {{ {}: {}; }} ", v, *font_family) })
                        .unwrap_or_else(|| "".to_owned())
                )
                .into(),
            )
            .into(),
        );

        Ok(ImportMapResult::Result(ResolveResult::asset(css_asset.into()).into()).into())
    }
}

#[turbo_tasks::function]
async fn update_stylesheet(
    stylesheet: StringVc,
    options: NextFontGoogleOptionsVc,
    scoped_font_family: StringVc,
) -> Result<StringVc> {
    // Update font-family definitions to the scoped name
    // TODO: Do this more resiliently, e.g. transforming an swc ast
    Ok(StringVc::cell(stylesheet.await?.replace(
        &format!("font-family: '{}';", &*options.await?.font_family),
        &format!("font-family: '{}';", &*scoped_font_family.await?),
    )))
}

#[turbo_tasks::function]
async fn get_scoped_font_family(query_vc: QueryMapVc) -> Result<StringVc> {
    let options = font_options_from_query_map(query_vc).await?;
    let hash = {
        let mut hash = format!("{:x?}", *get_request_hash(query_vc).await?);
        hash.truncate(6);
        hash
    };

    Ok(StringVc::cell(format!(
        "__{}_{}",
        options.font_family.replace(' ', "_"),
        hash
    )))
}

#[turbo_tasks::function]
async fn get_request_id(query_vc: QueryMapVc) -> Result<StringVc> {
    let options = font_options_from_query_map(query_vc).await?;

    Ok(StringVc::cell(format!(
        "{}_{:x?}",
        options.font_family.to_lowercase().replace(' ', "_"),
        get_request_hash(query_vc).await?,
    )))
}

#[turbo_tasks::function]
async fn get_request_hash(query_vc: QueryMapVc) -> Result<U32Vc> {
    let query = &*query_vc.await?;
    let query = query.as_ref().context("Query map must be present")?;
    let mut to_hash = vec![];
    for (k, v) in query {
        to_hash.push(k);
        to_hash.push(v);
    }

    Ok(U32Vc::cell(
        // Truncate the has to u32. These hashes are ultimately displayed as 8-character
        // hexadecimal values.
        hash_xxh3_hash64(to_hash) as u32,
    ))
}

#[turbo_tasks::function]
async fn get_stylesheet_url_from_options(options: NextFontGoogleOptionsVc) -> Result<StringVc> {
    #[allow(unused_mut, unused_assignments)] // This is used in test environments
    let mut css_url: Option<String> = None;
    #[cfg(debug_assertions)]
    {
        use turbo_tasks_env::{CommandLineProcessEnvVc, ProcessEnv};

        let env = CommandLineProcessEnvVc::new();
        if let Some(url) = &*env.read("TURBOPACK_TEST_ONLY_MOCK_SERVER").await? {
            css_url = Some(format!("{}/css2", url));
        }
    }

    let options = options.await?;
    Ok(StringVc::cell(get_stylesheet_url(
        css_url.as_deref().unwrap_or(GOOGLE_FONTS_STYLESHEET_URL),
        &options.font_family,
        &get_font_axes(
            &FONT_DATA,
            &options.font_family,
            &options.weights,
            &options.styles,
            &options.selected_variable_axes,
        )?,
        &options.display,
    )?))
}

#[turbo_tasks::value(transparent)]
struct NextFontGoogleOptions(self::options::NextFontGoogleOptions);

#[turbo_tasks::value(transparent)]
struct FontCssProperties {
    font_family: StringVc,
    weight: OptionU16Vc,
    style: OptionStringVc,
    variable: OptionStringVc,
}

#[turbo_tasks::function]
async fn get_font_css_properties(
    scoped_font_family: StringVc,
    options: NextFontGoogleOptionsVc,
) -> Result<FontCssPropertiesVc> {
    let options = &*options.await?;
    let scoped_font_family = &*scoped_font_family.await?;

    let mut font_families = vec![scoped_font_family.clone()];
    if let Some(fallback) = &options.fallback {
        font_families.extend_from_slice(fallback);
    }

    Ok(FontCssPropertiesVc::cell(FontCssProperties {
        font_family: StringVc::cell(
            font_families
                .iter()
                .map(|f| format!("'{}'", f))
                .collect::<Vec<String>>()
                .join(", "),
        ),
        weight: OptionU16Vc::cell(match &options.weights {
            FontWeights::Variable => None,
            FontWeights::Fixed(weights) => weights.first().cloned(),
        }),
        style: OptionStringVc::cell(options.styles.first().cloned()),
        variable: OptionStringVc::cell(options.variable.clone()),
    }))
}

#[turbo_tasks::function]
async fn font_options_from_query_map(query: QueryMapVc) -> Result<NextFontGoogleOptionsVc> {
    let query_map = &*query.await?;
    // These are invariants from the next/font swc transform. Regular errors instead
    // of Issues should be okay.
    let query_map = query_map
        .as_ref()
        .context("next/font/google queries must exist")?;

    if query_map.len() != 1 {
        bail!("next/font/google queries must only have one entry");
    }

    let Some((json, _)) = query_map.iter().next() else {
            bail!("Expected one entry");
        };

    self::options::options_from_request(&parse_json_with_source_context(json)?, &FONT_DATA)
        .map(NextFontGoogleOptionsVc::cell)
}

#[cfg(feature = "__internal_nextjs_integration_test")]
async fn get_mock_stylesheet(
    url: &str,
    execution_context: ExecutionContextVc,
) -> Result<Option<String>> {
    use std::{collections::HashMap, path::Path};

    use turbo_tasks::{CompletionVc, Value};
    use turbo_tasks_env::{CommandLineProcessEnvVc, ProcessEnv};
    use turbo_tasks_fs::{
        json::parse_json_rope_with_source_context, DiskFileSystemVc, File, FileSystem,
    };
    use turbopack::evaluate_context::node_evaluate_asset_context;
    use turbopack_core::{context::AssetContext, ident::AssetIdentVc};
    use turbopack_ecmascript::{
        EcmascriptInputTransformsVc, EcmascriptModuleAssetType, EcmascriptModuleAssetVc,
    };
    use turbopack_node::{
        evaluate::{evaluate, JavaScriptValue},
        execution_context::ExecutionContext,
    };

    let env = CommandLineProcessEnvVc::new().as_process_env();
    let mocked_response_js = &*env.read("NEXT_FONT_GOOGLE_MOCKED_RESPONSES").await?;
    let mocked_response_js = mocked_response_js
        .as_ref()
        .context("Expected env NEXT_FONT_GOOGLE_MOCKED_RESPONSES")?;

    let response_path = Path::new(&mocked_response_js);
    let mock_fs = DiskFileSystemVc::new(
        "mock".to_string(),
        response_path
            .parent()
            .context("Must be valid path")?
            .to_str()
            .context("Must exist")?
            .to_string(),
    )
    .as_file_system();

    let ExecutionContext {
        env,
        project_path,
        intermediate_output_path,
    } = *execution_context.await?;
    let context = node_evaluate_asset_context(project_path, None, None);
    let loader_path = mock_fs.root().join("loader.js");
    let mocked_response_asset = EcmascriptModuleAssetVc::new(
        VirtualAssetVc::new(
            loader_path,
            File::from(format!(
                "import data from './{}'; export default function load() {{ return data; }};",
                response_path
                    .file_name()
                    .context("Must exist")?
                    .to_string_lossy(),
            ))
            .into(),
        )
        .into(),
        context,
        Value::new(EcmascriptModuleAssetType::Ecmascript),
        EcmascriptInputTransformsVc::cell(vec![]),
        context.compile_time_info(),
    )
    .into();

    let root = mock_fs.root();
    let val = evaluate(
        loader_path,
        mocked_response_asset,
        root,
        env,
        AssetIdentVc::from_path(loader_path),
        context,
        intermediate_output_path,
        None,
        vec![],
        CompletionVc::immutable(),
        /* debug */ false,
    )
    .await?;

    match &*val {
        JavaScriptValue::Value(val) => {
            let mock_map: HashMap<String, Option<String>> =
                parse_json_rope_with_source_context(val)?;
            Ok((mock_map.get(url).context("url not found")?).clone())
        }
        JavaScriptValue::Error => panic!("Unexpected error evaluating JS"),
        JavaScriptValue::Stream(_) => {
            unimplemented!("Stream not supported now");
        }
    }
}

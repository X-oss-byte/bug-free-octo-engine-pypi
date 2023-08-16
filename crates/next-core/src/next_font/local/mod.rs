use anyhow::{bail, Context, Result};
use indoc::formatdoc;
use turbo_tasks::{
    primitives::{OptionStringVc, U32Vc},
    Value,
};
use turbo_tasks_fs::{json::parse_json_with_source_context, FileContent, FileSystemPathVc};
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

use self::{
    font_fallback::get_font_fallbacks,
    options::{options_from_request, FontDescriptors, NextFontLocalOptionsVc},
    stylesheet::build_stylesheet,
    util::build_font_family_string,
};
use super::{
    font_fallback::FontFallbacksVc,
    util::{FontCssProperties, FontCssPropertiesVc},
};
use crate::next_font::{
    local::options::FontWeight,
    util::{get_request_hash, get_request_id},
};

pub mod font_fallback;
pub mod options;
pub mod request;
pub mod stylesheet;
pub mod util;

#[turbo_tasks::value(shared)]
pub(crate) struct NextFontLocalReplacer {
    project_path: FileSystemPathVc,
}

#[turbo_tasks::value_impl]
impl NextFontLocalReplacerVc {
    #[turbo_tasks::function]
    pub fn new(project_path: FileSystemPathVc) -> Self {
        Self::cell(NextFontLocalReplacer { project_path })
    }
}

#[turbo_tasks::value_impl]
impl ImportMappingReplacement for NextFontLocalReplacer {
    #[turbo_tasks::function]
    fn replace(&self, _capture: &str) -> ImportMappingVc {
        ImportMapping::Ignore.into()
    }

    #[turbo_tasks::function]
    async fn result(
        &self,
        context: FileSystemPathVc,
        request: RequestVc,
    ) -> Result<ImportMapResultVc> {
        let Request::Module {
            module: _,
            path: _,
            query: query_vc
        } = &*request.await? else {
            return Ok(ImportMapResult::NoEntry.into());
        };

        let request_hash = get_request_hash(*query_vc);
        let options_vc = font_options_from_query_map(*query_vc);
        let options = &*options_vc.await?;
        let font_fallbacks = get_font_fallbacks(context, options_vc, request_hash);
        let properties =
            &*get_font_css_properties(options_vc, font_fallbacks, request_hash).await?;
        let file_content = formatdoc!(
            r#"
                import cssModule from "@vercel/turbopack-next/internal/font/local/cssmodule.module.css?{}";
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
            qstring::QString::new(query_vc.await?.as_ref().unwrap().iter().collect()),
            properties.font_family.await?,
            properties
                .weight
                .await?
                .as_ref()
                .map(|w| format!("fontWeight: {},\n", w))
                .unwrap_or_else(|| "".to_owned()),
            properties
                .style
                .await?
                .as_ref()
                .map(|s| format!("fontStyle: \"{}\",\n", s))
                .unwrap_or_else(|| "".to_owned()),
        );
        let js_asset = VirtualAssetVc::new(
            context.join(&format!(
                "{}.js",
                get_request_id(options.variable_name.clone(), request_hash).await?
            )),
            FileContent::Content(file_content.into()).into(),
        );

        Ok(ImportMapResult::Result(ResolveResult::asset(js_asset.into()).into()).into())
    }
}

#[turbo_tasks::value(shared)]
pub struct NextFontLocalCssModuleReplacer {
    project_path: FileSystemPathVc,
}

#[turbo_tasks::value_impl]
impl NextFontLocalCssModuleReplacerVc {
    #[turbo_tasks::function]
    pub fn new(project_path: FileSystemPathVc) -> Self {
        Self::cell(NextFontLocalCssModuleReplacer { project_path })
    }
}

#[turbo_tasks::value_impl]
impl ImportMappingReplacement for NextFontLocalCssModuleReplacer {
    #[turbo_tasks::function]
    fn replace(&self, _capture: &str) -> ImportMappingVc {
        ImportMapping::Ignore.into()
    }

    #[turbo_tasks::function]
    async fn result(
        &self,
        context: FileSystemPathVc,
        request: RequestVc,
    ) -> Result<ImportMapResultVc> {
        let request = &*request.await?;
        let Request::Module {
            module: _,
            path: _,
            query: query_vc,
        } = request else {
            return Ok(ImportMapResult::NoEntry.into());
        };

        let options = font_options_from_query_map(*query_vc);
        let request_hash = get_request_hash(*query_vc);
        let css_virtual_path = context.join(&format!(
            "/{}.module.css",
            get_request_id(options.await?.variable_name.clone(), request_hash).await?
        ));
        let fallback = get_font_fallbacks(context, options, request_hash);

        let stylesheet = build_stylesheet(
            font_options_from_query_map(*query_vc),
            fallback,
            get_font_css_properties(options, fallback, request_hash),
            get_request_hash(*query_vc),
        )
        .await?;

        let css_asset = VirtualAssetVc::new(
            css_virtual_path,
            FileContent::Content(stylesheet.into()).into(),
        );

        Ok(ImportMapResult::Result(ResolveResult::asset(css_asset.into()).into()).into())
    }
}

#[turbo_tasks::function]
async fn get_font_css_properties(
    options_vc: NextFontLocalOptionsVc,
    font_fallbacks: FontFallbacksVc,
    request_hash: U32Vc,
) -> Result<FontCssPropertiesVc> {
    let options = &*options_vc.await?;

    Ok(FontCssPropertiesVc::cell(FontCssProperties {
        font_family: build_font_family_string(options_vc, font_fallbacks, request_hash),
        weight: OptionStringVc::cell(match &options.fonts {
            FontDescriptors::Many(_) => None,
            FontDescriptors::One(descriptor) => descriptor
                .weight
                .as_ref()
                // Don't include values for variable fonts. These are included in font-face
                // definitions only.
                .filter(|w| !matches!(w, FontWeight::Variable(_, _)))
                .map(|w| w.to_string()),
        }),
        style: OptionStringVc::cell(match &options.fonts {
            FontDescriptors::Many(_) => None,
            FontDescriptors::One(descriptor) => descriptor.style.clone(),
        }),
        variable: OptionStringVc::cell(options.variable.clone()),
    }))
}

#[turbo_tasks::function]
async fn font_options_from_query_map(query: QueryMapVc) -> Result<NextFontLocalOptionsVc> {
    let query_map = &*query.await?;
    // These are invariants from the next/font swc transform. Regular errors instead
    // of Issues should be okay.
    let query_map = query_map
        .as_ref()
        .context("next/font/local queries must exist")?;

    if query_map.len() != 1 {
        bail!("next/font/local queries must only have one entry");
    }

    let Some((json, _)) = query_map.iter().next() else {
            bail!("Expected one entry");
        };

    options_from_request(&parse_json_with_source_context(json)?)
        .map(|o| NextFontLocalOptionsVc::new(Value::new(o)))
}

use anyhow::{anyhow, bail, Context, Result};
use auto_hash_map::AutoMap;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use turbo_tasks::{
    primitives::{OptionStringVc, StringVc},
    TryJoinIterExt,
};
use turbo_tasks_env::{CommandLineProcessEnvVc, ProcessEnv};
use turbo_tasks_fetch::fetch;
use turbo_tasks_fs::{File, FileContent, FileSystemPathVc};
use turbo_tasks_hash::hash_xxh3_hash64;
use turbopack_core::{
    resolve::{
        options::{
            ImportMapResult, ImportMapResultVc, ImportMapping, ImportMappingReplacement,
            ImportMappingReplacementVc, ImportMappingVc,
        },
        parse::{Request, RequestVc},
        ResolveResult,
    },
    virtual_asset::VirtualAssetVc,
};

use crate::{
    embed_js::attached_next_js_package_path,
    next_font_google::{
        options::FontDataEntry,
        request::NextFontRequest,
        util::{extract_font_urls, get_font_axes, get_stylesheet_url},
    },
};

pub(crate) mod options;
pub(crate) mod request;
mod util;

pub const GOOGLE_FONTS_STYLESHEET_URL: &str = "https://fonts.googleapis.com/css2";
static FONT_DATA: Lazy<FontData> =
    Lazy::new(|| serde_json::from_str(include_str!("font-data.json")).unwrap());

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
        if let Request::Module {
            module: _,
            path: _,
            query,
        } = request
        {
            let q = &*query.await?;

            let js_asset = VirtualAssetVc::new(
                attached_next_js_package_path(self.project_path)
                    .join("internal/font/google/inter.js"),
                FileContent::Content(
                    format!(
                        r#"
                    import cssModule from "@vercel/turbopack-next/internal/font/google/cssmodule.module.css?{}";
                    export default {{
                        className: cssModule.className
                    }};
                "#,
                        // Pass along whichever options we received to the css handler
                        qstring::QString::new(q.as_ref().unwrap().iter().collect())
                    )
                    .into(),
                )
                .into(),
            );

            return Ok(ImportMapResult::Result(
                ResolveResult::Single(js_asset.into(), vec![]).into(),
            )
            .into());
        };

        Ok(ImportMapResult::NoEntry.into())
    }
}

#[turbo_tasks::value(shared)]
pub struct NextFontGoogleCssModuleReplacer {
    project_path: FileSystemPathVc,
}

#[turbo_tasks::value_impl]
impl NextFontGoogleCssModuleReplacerVc {
    #[turbo_tasks::function]
    pub fn new(project_path: FileSystemPathVc) -> Self {
        Self::cell(NextFontGoogleCssModuleReplacer { project_path })
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
        if let Request::Module {
            module: _,
            path: _,
            query,
        } = request
        {
            let query_map = &*query.await?;
            // TODO: Turn into issue
            let mut query_map = query_map
                .clone()
                .context("@next/font/google queries must exist")?;
            // TODO: Turn into issue
            assert_eq!(
                query_map.len(),
                1,
                "@next/font/google queries must only have one entry"
            );

            let Some((json, _)) = query_map.pop() else {
                // TODO: Turn into issue
                return Err(anyhow!("Expected one entry"));
            };

            let request: StringVc = StringVc::cell(json);
            // let font_data = StringVc::cell(include_str!("font-data.json").to_owned());
            let stylesheet_url = get_stylesheet_url_from_request(request);
            let stylesheet_res = fetch(
                stylesheet_url,
                OptionStringVc::cell(Some(
                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, \
                     like Gecko) Chrome/104.0.0.0 Safari/537.36"
                        .to_owned(),
                )),
            )
            .await?;

            if stylesheet_res.status >= 400 {
                bail!("Expected a successful response for Google fonts stylesheet");
            }

            let stylesheet = &*stylesheet_res.body.to_string().await?;
            let options = options_from_request(request).await?;
            let fonts = extract_font_urls(stylesheet, options.subsets.as_ref(), options.preload)?;

            let mut requests = vec![];
            for font_url in &fonts.all_urls {
                requests.push(fetch(
                    StringVc::cell(font_url.to_owned()),
                    OptionStringVc::cell(None),
                ));
            }

            let mut url_to_filename = AutoMap::new();
            let fonts_dir = self.project_path.join(".next/static/media");
            for (url, response) in fonts
                .all_urls
                .iter()
                .zip(requests.iter().try_join().await?.iter())
            {
                if response.status >= 400 {
                    bail!(
                        "Expected a successful response for font at url {}. Received status {}",
                        url,
                        response.status
                    );
                }

                let should_preload = fonts.preload_urls.contains(url);
                let url_file_extension = url
                    .rsplit('.')
                    .next()
                    .context("font url must have file extension")?;
                let filename = format!(
                    "{:x}{}.{}",
                    hash_xxh3_hash64(url),
                    if should_preload { ".p" } else { "" },
                    url_file_extension
                );
                url_to_filename.insert(url, filename.to_owned());

                let body = &*response.body.await?;
                fonts_dir
                    .join(&filename)
                    .write(FileContent::Content(File::from(body.0.clone())).into());
            }

            let mut updated_stylesheet = stylesheet.to_owned();
            for (url, filename) in url_to_filename {
                updated_stylesheet =
                    updated_stylesheet.replace(url, &format!("/_next/static/media/{}", filename));
            }

            let css_asset = VirtualAssetVc::new(
                attached_next_js_package_path(self.project_path)
                    .join("internal/font/google/cssmodule.module.css"),
                FileContent::Content(
                    format!(
                        r#"{}

                        .className {{
                            font-family: "{}";
                        }}
                        "#,
                        updated_stylesheet, options.font_family
                    )
                    .into(),
                )
                .into(),
            );

            return Ok(ImportMapResult::Result(
                ResolveResult::Single(css_asset.into(), vec![]).into(),
            )
            .into());
        };

        Ok(ImportMapResult::NoEntry.into())
    }
}

#[turbo_tasks::function]
async fn get_stylesheet_url_from_request(request_json: StringVc) -> Result<StringVc> {
    let options = options_from_request(request_json).await?;

    let url = CommandLineProcessEnvVc::new()
        .read("TURBOPACK_TEST_ONLY_GOOGLE_FONTS_STYLESHEET_URL")
        .await?;
    let url = url
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or(GOOGLE_FONTS_STYLESHEET_URL);

    Ok(StringVc::cell(get_stylesheet_url(
        url,
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

#[turbo_tasks::function]
async fn options_from_request(request: StringVc) -> Result<NextFontGoogleOptionsVc> {
    let request: NextFontRequest = serde_json::from_str(&request.await?)?;

    self::options::options_from_request(&request, &FONT_DATA).map(NextFontGoogleOptionsVc::cell)
}

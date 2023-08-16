use anyhow::{bail, Context, Result};
use auto_hash_map::AutoMap;
use indexmap::IndexMap;
use indoc::formatdoc;
use once_cell::sync::Lazy;
use turbo_tasks::{
    primitives::{OptionStringVc, StringVc},
    TryJoinIterExt,
};
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
        let Request::Module {
            module: _,
            path: _,
            query,
        } = request else {
            return Ok(ImportMapResult::NoEntry.into());
        };

        let query = &*query.await?;
        let js_asset = VirtualAssetVc::new(
                attached_next_js_package_path(self.project_path)
                    .join("internal/font/google/inter.js"),
                FileContent::Content(
                    formatdoc!(
                        r#"
                            import cssModule from "@vercel/turbopack-next/internal/font/google/cssmodule.module.css?{}";
                            export default {{
                                className: cssModule.className
                            }};
                        "#,
                        // Pass along whichever options we received to the css handler
                        qstring::QString::new(query.as_ref().unwrap().iter().collect())
                    )
                    .into(),
                )
                .into(),
            );

        Ok(ImportMapResult::Result(ResolveResult::Single(js_asset.into(), vec![]).into()).into())
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
        let Request::Module {
            module: _,
            path: _,
            query,
        } = request else {
            return Ok(ImportMapResult::NoEntry.into());
        };

        let query_map = &*query.await?;
        // These are invariants from the next/font swc transform. Regular errors instead
        // of Issues should be okay.
        let query_map = query_map
            .as_ref()
            .context("@next/font/google queries must exist")?;

        assert_eq!(
            query_map.len(),
            1,
            "@next/font/google queries must only have one entry"
        );

        let Some((json, _)) = query_map.iter().next() else {
            bail!("Expected one entry");
        };

        let request: StringVc = StringVc::cell(json.to_owned());
        let stylesheet_url = get_stylesheet_url_from_request(request);
        // TODO: Handle this failing (e.g. connection issues). This should be an Issue.
        let stylesheet_res = fetch(
            stylesheet_url,
            OptionStringVc::cell(Some(
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like \
                 Gecko) Chrome/104.0.0.0 Safari/537.36"
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

        let requests: Vec<_> = fonts
            .all_urls
            .iter()
            .map(|font_url| {
                // TODO: Handle this failing (e.g. connection issues). This should be an Issue.
                fetch(
                    StringVc::cell(font_url.to_owned()),
                    OptionStringVc::cell(None),
                )
            })
            .collect();

        let mut url_to_filename = AutoMap::new();
        let fonts_dir = self.project_path.join(".next/static/media");
        let responses = requests.iter().try_join().await?;
        for (url, response) in fonts.all_urls.iter().zip(responses.iter()) {
            if response.status >= 400 {
                bail!(
                    "Expected a successful response for font at url {}. Received status {}",
                    url,
                    response.status
                );
            }
        }

        for (url, response) in fonts.all_urls.iter().zip(responses.iter()) {
            let (_, url_file_extension) = url
                .rsplit_once('.')
                .context("font url must have file extension")?;

            let filename = format!(
                "{:x}{}.{}",
                hash_xxh3_hash64(url),
                if fonts.preload_urls.contains(url) {
                    ".p"
                } else {
                    ""
                },
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
                formatdoc!(
                    r#"
                        {}

                        .className {{
                            font-family: "{}";
                        }}
                        "#,
                    updated_stylesheet,
                    options.font_family
                )
                .into(),
            )
            .into(),
        );

        Ok(ImportMapResult::Result(ResolveResult::Single(css_asset.into(), vec![]).into()).into())
    }
}

#[turbo_tasks::function]
async fn get_stylesheet_url_from_request(request_json: StringVc) -> Result<StringVc> {
    let options = options_from_request(request_json).await?;

    Ok(StringVc::cell(get_stylesheet_url(
        GOOGLE_FONTS_STYLESHEET_URL,
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

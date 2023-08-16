use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use turbo_tasks::{
    primitives::{StringVc, StringsVc, U32Vc},
    trace::TraceRawVcs,
};
use turbo_tasks_fs::FileSystemPathVc;
use turbopack_core::issue::IssueSeverity;

use super::options::NextFontGoogleOptionsVc;
use crate::{
    next_font::{
        font_fallback::{
            AutomaticFontFallback, FontAdjustment, FontFallback, FontFallbackVc,
            DEFAULT_SANS_SERIF_FONT, DEFAULT_SERIF_FONT,
        },
        issue::NextFontIssue,
        util::{get_scoped_font_family, FontFamilyType},
    },
    util::load_next_json,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FontMetricsMapEntry {
    category: String,
    ascent: f64,
    descent: f64,
    line_gap: f64,
    units_per_em: f64,
    az_avg_width: f64,
}

#[derive(Deserialize)]
pub(super) struct FontMetricsMap(pub HashMap<String, FontMetricsMapEntry>);

#[derive(Debug, PartialEq, Serialize, Deserialize, TraceRawVcs)]
pub(super) struct Font {
    pub font_family: String,
    pub adjustment: Option<FontAdjustment>,
}

#[turbo_tasks::function]
pub(super) async fn get_font_fallback(
    context: FileSystemPathVc,
    options_vc: NextFontGoogleOptionsVc,
    request_hash: U32Vc,
) -> Result<FontFallbackVc> {
    let options = options_vc.await?;
    Ok(match &options.fallback {
        Some(fallback) => FontFallback::Manual(StringsVc::cell(fallback.clone())).cell(),
        None => {
            let metrics_json =
                load_next_json(context, "/dist/server/google-font-metrics.json").await;
            match metrics_json {
                Ok(metrics_json) => {
                    let fallback = lookup_fallback(
                        &options.font_family,
                        metrics_json,
                        options.adjust_font_fallback,
                    );

                    match fallback {
                        Ok(fallback) => FontFallback::Automatic(
                            AutomaticFontFallback {
                                scoped_font_family: get_scoped_font_family(
                                    FontFamilyType::Fallback.cell(),
                                    options.font_family.to_owned(),
                                    request_hash,
                                ),
                                local_font_family: StringVc::cell(fallback.font_family),
                                adjustment: fallback.adjustment,
                            }
                            .cell(),
                        )
                        .cell(),
                        Err(_) => {
                            NextFontIssue {
                                path: context,
                                title: StringVc::cell(format!(
                                    "Failed to find font override values for font `{}`",
                                    &options.font_family,
                                )),
                                description: StringVc::cell(
                                    "Skipping generating a fallback font.".to_owned(),
                                ),
                                severity: IssueSeverity::Warning.cell(),
                            }
                            .cell()
                            .as_issue()
                            .emit();
                            FontFallback::Error.cell()
                        }
                    }
                }
                Err(_) => FontFallback::Error.cell(),
            }
        }
    })
}

fn lookup_fallback(
    font_family: &str,
    font_metrics_map: FontMetricsMap,
    adjust: bool,
) -> Result<Font> {
    let metrics = font_metrics_map
        .0
        .get(font_family)
        .context("Font not found in metrics")?;

    let fallback = if metrics.category == "serif" {
        &DEFAULT_SERIF_FONT
    } else {
        &DEFAULT_SANS_SERIF_FONT
    };

    let metrics = if adjust {
        // Derived from
        // https://github.com/vercel/next.js/blob/a3893bf69c83fb08e88c87bf8a21d987a0448c8e/packages/next/src/server/font-utils.ts#L121
        let main_font_avg_width = metrics.az_avg_width / metrics.units_per_em;
        let fallback_font_avg_width = fallback.az_avg_width / fallback.units_per_em;
        let size_adjust = main_font_avg_width / fallback_font_avg_width;

        let ascent = metrics.ascent / (metrics.units_per_em * size_adjust);
        let descent = metrics.descent / (metrics.units_per_em * size_adjust);
        let line_gap = metrics.line_gap / (metrics.units_per_em * size_adjust);

        Some(FontAdjustment {
            ascent,
            descent,
            line_gap,
            size_adjust,
        })
    } else {
        None
    };

    Ok(Font {
        font_family: fallback.name.clone(),
        adjustment: metrics,
    })
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use turbo_tasks_fs::json::parse_json_with_source_context;

    use super::{FontAdjustment, FontMetricsMap};
    use crate::next_font::google::font_fallback::{lookup_fallback, Font};

    #[test]
    fn test_fallback_from_metrics_sans_serif() -> Result<()> {
        let font_metrics: FontMetricsMap = parse_json_with_source_context(
            r#"
            {
                "Inter": {
                    "category": "sans-serif",
                    "ascent": 2728,
                    "descent": -680,
                    "lineGap": 0,
                    "xAvgCharWidth": 1838,
                    "unitsPerEm": 2816,
                    "azAvgWidth": 1383.0697674418604
                }
            }
        "#,
        )?;

        assert_eq!(
            lookup_fallback("Inter", font_metrics, true)?,
            Font {
                font_family: "Arial".to_owned(),
                adjustment: Some(FontAdjustment {
                    ascent: 0.90002596,
                    descent: -0.22434665,
                    line_gap: 0.0,
                    size_adjust: 1.0763578
                })
            }
        );
        Ok(())
    }

    #[test]
    fn test_fallback_from_metrics_serif() -> Result<()> {
        let font_metrics: FontMetricsMap = parse_json_with_source_context(
            r#"
            {
                "Roboto Slab": {
                    "category": "serif",
                    "ascent": 2146,
                    "descent": -555,
                    "lineGap": 0,
                    "xAvgCharWidth": 1239,
                    "unitsPerEm": 2048,
                    "azAvgWidth": 1005.6279069767442
                }
            }
        "#,
        )?;

        assert_eq!(
            lookup_fallback("Roboto Slab", font_metrics, true)?,
            Font {
                font_family: "Times New Roman".to_owned(),
                adjustment: Some(FontAdjustment {
                    ascent: 0.890_269_16,
                    descent: -0.230_242_01,
                    line_gap: 0.0,
                    size_adjust: 1.177_005_4
                })
            }
        );
        Ok(())
    }
}

use anyhow::Result;
use turbo_tasks::{Value, Vc};
use turbopack_core::{
    introspect::Introspectable, source_map::GenerateSourceMap, version::VersionedContentExt,
};
use turbopack_dev_server::source::{
    wrapping_source::{ContentSourceProcessor, WrappedContentSource},
    ContentSource, ContentSourceContent, ContentSourceData, ContentSourceDataVary,
    ContentSourceResult, ContentSources, NeededData, RewriteBuilder,
};
use url::Url;

use super::{SourceMapTrace, StackFrame};
/// Responsible for performinmg source map tracging for individual error stack
/// frames. This is the API end of the client's Overlay stack-frame.ts.
#[turbo_tasks::value(shared)]
pub struct NextSourceMapTraceContentSource {
    asset_source: Vc<Box<dyn ContentSource>>,
}

#[turbo_tasks::value_impl]
impl NextSourceMapTraceContentSource {
    #[turbo_tasks::function]
    pub fn new(asset_source: Vc<Box<dyn ContentSource>>) -> Vc<NextSourceMapTraceContentSource> {
        NextSourceMapTraceContentSource { asset_source }.cell()
    }
}

#[turbo_tasks::value_impl]
impl ContentSource for NextSourceMapTraceContentSource {
    #[turbo_tasks::function]
    async fn get(
        self: Vc<Self>,
        path: String,
        data: Value<ContentSourceData>,
    ) -> Result<Vc<ContentSourceResult>> {
        let raw_query = match &data.raw_query {
            None => {
                return Ok(ContentSourceResult::need_data(Value::new(NeededData {
                    source: Vc::upcast(self),
                    path: path.to_string(),
                    vary: ContentSourceDataVary {
                        raw_query: true,
                        ..Default::default()
                    },
                })));
            }
            Some(query) => query,
        };

        let frame: StackFrame = match serde_qs::from_str(raw_query) {
            Ok(f) => f,
            _ => return Ok(ContentSourceResult::not_found()),
        };
        let (line, column) = match frame.get_pos() {
            Some((l, c)) => (l, c),
            _ => return Ok(ContentSourceResult::not_found()),
        };

        // The file is some percent encoded `http://localhost:3000/_next/foo/bar.js`
        let file = match Url::parse(&frame.file) {
            Ok(u) => u,
            _ => return Ok(ContentSourceResult::not_found()),
        };

        let id = file.query_pairs().find_map(|(k, v)| {
            if k == "id" {
                Some(v.into_owned())
            } else {
                None
            }
        });

        let wrapped = WrappedContentSource::new(
            self.await?.asset_source,
            Vc::upcast(NextSourceMapTraceContentProcessor::new(
                id,
                line,
                column,
                frame.name.map(|c| c.to_string()),
            )),
        );
        Ok(ContentSourceResult::exact(Vc::upcast(
            ContentSourceContent::Rewrite(
                RewriteBuilder::new(file.path().to_string())
                    .content_source(Vc::upcast(wrapped))
                    .build(),
            )
            .cell(),
        )))
    }

    #[turbo_tasks::function]
    fn get_children(&self) -> Vc<ContentSources> {
        Vc::cell(vec![self.asset_source])
    }
}

#[turbo_tasks::value_impl]
impl Introspectable for NextSourceMapTraceContentSource {
    #[turbo_tasks::function]
    fn ty(&self) -> Vc<String> {
        Vc::cell("next source map trace content source".to_string())
    }

    #[turbo_tasks::function]
    fn details(&self) -> Vc<String> {
        Vc::cell(
            "supports tracing an error stack frame to its original source location".to_string(),
        )
    }
}

/// Processes the eventual [ContentSourceContent] to transform it into a source
/// map trace's JSON content.
#[turbo_tasks::value]
pub struct NextSourceMapTraceContentProcessor {
    /// An optional section id to use when tracing the map. Specifying a
    /// section id trace starting at that section. Otherwise, it traces starting
    /// at the full source map.
    id: Option<String>,

    /// The generated line we are trying to trace.
    line: usize,

    /// The generated column we are trying to trace.
    column: usize,

    /// An optional name originally assigned to the stack frame, used as a
    /// default if the trace finds an unnamed source map segment.
    name: Option<String>,
}

#[turbo_tasks::value_impl]
impl NextSourceMapTraceContentProcessor {
    #[turbo_tasks::function]
    fn new(id: Option<String>, line: usize, column: usize, name: Option<String>) -> Vc<Self> {
        NextSourceMapTraceContentProcessor {
            id,
            line,
            column,
            name,
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl ContentSourceProcessor for NextSourceMapTraceContentProcessor {
    #[turbo_tasks::function]
    async fn process(&self, content: Vc<ContentSourceContent>) -> Result<Vc<ContentSourceContent>> {
        let file = match &*content.await? {
            ContentSourceContent::Static(static_content) => static_content.await?.content,
            _ => return Ok(ContentSourceContent::not_found()),
        };

        let gen = match Vc::try_resolve_sidecast::<Box<dyn GenerateSourceMap>>(file).await? {
            Some(f) => f,
            _ => return Ok(ContentSourceContent::not_found()),
        };

        let sm = if let Some(id) = &self.id {
            gen.by_section(id.clone()).await?
        } else {
            gen.generate_source_map().await?
        };
        let sm = match &*sm {
            Some(sm) => *sm,
            None => return Ok(ContentSourceContent::not_found()),
        };

        let traced = SourceMapTrace::new(sm, self.line, self.column, self.name.clone());
        Ok(ContentSourceContent::static_content(
            traced.content().versioned(),
        ))
    }
}

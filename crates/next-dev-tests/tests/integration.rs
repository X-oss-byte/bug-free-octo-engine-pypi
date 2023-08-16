#![feature(min_specialization)]
#![cfg(test)]
extern crate test_generator;

use std::{
    env,
    fmt::Write,
    future::Future,
    net::SocketAddr,
    panic::{catch_unwind, resume_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use chromiumoxide::{
    browser::{Browser, BrowserConfig},
    cdp::{
        browser_protocol::network::EventResponseReceived,
        js_protocol::runtime::{
            AddBindingParams, EventBindingCalled, EventConsoleApiCalled, EventExceptionThrown,
            PropertyPreview, RemoteObject,
        },
    },
    error::CdpError::Ws,
};
use futures::StreamExt;
use lazy_static::lazy_static;
use next_dev::{EntryRequest, NextDevServerBuilder};
use owo_colors::OwoColorize;
use serde::Deserialize;
use test_generator::test_resources;
use tokio::{
    net::TcpSocket,
    sync::mpsc::{channel, Sender},
    task::JoinSet,
};
use tungstenite::{error::ProtocolError::ResetWithoutClosingHandshake, Error::Protocol};
use turbo_tasks::{
    debug::{ValueDebug, ValueDebugStringReadRef},
    primitives::BoolVc,
    NothingVc, RawVc, ReadRef, State, TransientInstance, TransientValue, TurboTasks,
};
use turbo_tasks_fs::{DiskFileSystemVc, FileSystem};
use turbo_tasks_memory::MemoryBackend;
use turbo_tasks_testing::retry::retry_async;
use turbopack_core::issue::{CapturedIssues, IssueReporter, IssueReporterVc, PlainIssueReadRef};
use turbopack_test_utils::snapshot::snapshot_issues;

fn register() {
    next_dev::register();
    include!(concat!(env!("OUT_DIR"), "/register_test_integration.rs"));
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JestRunResult {
    test_results: Vec<JestTestResult>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JestTestResult {
    test_path: Vec<String>,
    errors: Vec<String>,
}

lazy_static! {
    // Allows for interactive manual debugging of a test case in a browser with:
    // `TURBOPACK_DEBUG_BROWSER=1 cargo test -p next-dev-tests -- test_my_pattern --nocapture`
    static ref DEBUG_BROWSER: bool = env::var("TURBOPACK_DEBUG_BROWSER").is_ok();
}

fn run_async_test<'a, T>(future: impl Future<Output = T> + Send + 'a) -> T {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let result = catch_unwind(AssertUnwindSafe(|| {
        runtime.block_on(async move {
            #[cfg(feature = "tokio_console")]
            console_subscriber::init();
            future.await
        })
    }));
    println!("Stutting down runtime...");
    runtime.shutdown_timeout(Duration::from_secs(5));
    println!("Stut down runtime");
    match result {
        Ok(result) => result,
        Err(err) => resume_unwind(err),
    }
}

#[test_resources("crates/next-dev-tests/tests/integration/*/*/*")]
fn test(resource: &str) {
    if resource.ends_with("__skipped__") || resource.ends_with("__flakey__") {
        // "Skip" directories named `__skipped__`, which include test directories to
        // skip. These tests are not considered truly skipped by `cargo test`, but they
        // are not run.
        //
        // All current `__flakey__` tests need longer timeouts, but the current
        // build of `jest-circus-browser` does not support configuring this.
        //
        // TODO(WEB-319): Update the version of `jest-circus` in `jest-circus-browser`,
        // which supports configuring this. Or explore an alternative.
        return;
    }

    let run_result = run_async_test(run_test(resource));

    assert!(
        !run_result.test_results.is_empty(),
        "Expected one or more tests to run."
    );

    let mut messages = vec![];
    for test_result in run_result.test_results {
        // It's possible to fail multiple tests across these tests,
        // so collect them and fail the respective test in Rust with
        // an aggregate message.
        if !test_result.errors.is_empty() {
            messages.push(format!(
                "\"{}\":\n{}",
                test_result.test_path[1..].join(" > "),
                test_result.errors.join("\n")
            ));
        }
    }

    if !messages.is_empty() {
        panic!(
            "Failed with error(s) in the following test(s):\n\n{}",
            messages.join("\n\n--\n")
        )
    };
}

#[test_resources("crates/next-dev-tests/tests/integration/*/*/__skipped__/*")]
#[should_panic]
fn test_skipped_fails(resource: &str) {
    let run_result = run_async_test(run_test(resource));

    // Assert that this skipped test itself has at least one browser test which
    // fails.
    assert!(
        // Skipped tests sometimes have errors (e.g. unsupported syntax) that prevent tests from
        // running at all. Allow them to have empty results.
        run_result.test_results.is_empty()
            || run_result
                .test_results
                .into_iter()
                .any(|r| !r.errors.is_empty()),
    );
}

async fn run_test(resource: &str) -> JestRunResult {
    register();
    let path = Path::new(resource)
        // test_resources matches and returns relative paths from the workspace root,
        // but pwd in cargo tests is the crate under test.
        .strip_prefix("crates/next-dev-tests")
        .unwrap();
    assert!(path.exists(), "{} does not exist", resource);

    assert!(
        path.is_dir(),
        "{} is not a directory. Integration tests must be directories.",
        path.to_str().unwrap()
    );

    let package_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = package_root
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let test_dir = workspace_root.join(resource);
    let project_dir = test_dir.join("input");
    let requested_addr = get_free_local_addr().unwrap();

    let mock_dir = path.join("__httpmock__");
    let mock_server_future = get_mock_server_future(&mock_dir);

    let (issue_tx, mut issue_rx) = channel(u16::MAX as usize);
    let issue_tx = TransientInstance::new(issue_tx);

    let tt = TurboTasks::new(MemoryBackend::default());
    let server = NextDevServerBuilder::new(
        tt.clone(),
        project_dir.to_string_lossy().to_string(),
        workspace_root.to_string_lossy().to_string(),
    )
    .entry_request(EntryRequest::Module(
        "@turbo/pack-test-harness".to_string(),
        "".to_string(),
    ))
    .entry_request(EntryRequest::Relative("index.js".to_owned()))
    .eager_compile(false)
    .hostname(requested_addr.ip())
    .port(requested_addr.port())
    .log_level(turbopack_core::issue::IssueSeverity::Warning)
    .log_detail(true)
    .issue_reporter(Box::new(move || {
        TestIssueReporterVc::new(issue_tx.clone()).into()
    }))
    .show_all(true)
    .build()
    .await
    .unwrap();

    println!(
        "{event_type} - server started at http://{address}",
        event_type = "ready".green(),
        address = server.addr
    );

    let result = tokio::select! {
        // Poll the mock_server first to add the env var
        _ = mock_server_future => panic!("Never resolves"),
        r = run_browser(server.addr) => r.expect("error while running browser"),
        _ = server.future => panic!("Never resolves"),
    };

    env::remove_var("TURBOPACK_TEST_ONLY_MOCK_SERVER");

    let task = tt.spawn_once_task(async move {
        let issues_fs = DiskFileSystemVc::new(
            "issues".to_string(),
            test_dir.join("issues").to_string_lossy().to_string(),
        )
        .as_file_system();

        let mut issues = vec![];
        while let Ok(issue) = issue_rx.try_recv() {
            issues.push(issue);
        }

        snapshot_issues(
            issues.iter().cloned(),
            issues_fs.root(),
            &workspace_root.to_string_lossy(),
        )
        .await?;

        Ok(NothingVc::new().into())
    });
    tt.wait_task_completion(task, true).await.unwrap();

    result
}

async fn create_browser(is_debugging: bool) -> Result<(Browser, JoinSet<()>)> {
    let mut config_builder = BrowserConfig::builder();
    if is_debugging {
        config_builder = config_builder
            .with_head()
            .args(vec!["--auto-open-devtools-for-tabs"]);
    }

    let (browser, mut handler) = retry_async(
        config_builder.build().map_err(|s| anyhow!(s))?,
        |c| {
            let c = c.clone();
            Browser::launch(c)
        },
        3,
        Duration::from_millis(100),
    )
    .await
    .context("Launching browser failed")?;

    // For windows it's important that the browser is dropped so that the test can
    // complete. To do that we need to cancel the spawned task below (which will
    // drop the browser). For this we are using a JoinSet which cancels all tasks
    // when dropped.
    let mut set = JoinSet::new();
    // See https://crates.io/crates/chromiumoxide
    set.spawn(async move {
        loop {
            if let Err(Ws(Protocol(ResetWithoutClosingHandshake))) = handler.next().await.unwrap() {
                // The user has most likely closed the browser. End gracefully.
                break;
            }
        }
    });

    Ok((browser, set))
}

async fn run_browser(addr: SocketAddr) -> Result<JestRunResult> {
    let is_debugging = *DEBUG_BROWSER;
    let (browser, mut handle) = create_browser(is_debugging).await?;

    // `browser.new_page()` opens a tab, navigates to the destination, and waits for
    // the page to load. chromiumoxide/Chrome DevTools Protocol has been flakey,
    // returning `ChannelSendError`s (WEB-259). Retry if necessary.
    let page = retry_async(
        (),
        |_| browser.new_page("about:blank"),
        5,
        Duration::from_millis(100),
    )
    .await
    .context("Failed to create new browser page")?;

    page.execute(AddBindingParams::new("READY")).await?;

    let mut errors = page
        .event_listener::<EventExceptionThrown>()
        .await
        .context("Unable to listen to exception events")?;
    let mut binding_events = page
        .event_listener::<EventBindingCalled>()
        .await
        .context("Unable to listen to binding events")?;
    let mut console_events = page
        .event_listener::<EventConsoleApiCalled>()
        .await
        .context("Unable to listen to console events")?;
    let mut network_response_events = page
        .event_listener::<EventResponseReceived>()
        .await
        .context("Unable to listen to response received events")?;

    page.evaluate_expression(format!("window.location='http://{addr}'"))
        .await
        .context("Unable to evaluate javascript to naviagate to target page")?;

    // Wait for the next network response event
    // This is the HTML page that we're testing
    network_response_events.next().await.context(
        "Network events channel ended unexpectedly while waiting on the network response",
    )?;

    if is_debugging {
        let _ = page.evaluate(
            r#"console.info("%cTurbopack tests:", "font-weight: bold;", "Waiting for READY to be signaled by page...");"#,
        )
        .await;
    }

    let mut errors_next = errors.next();
    let mut bindings_next = binding_events.next();
    let mut console_next = console_events.next();
    let mut network_next = network_response_events.next();

    loop {
        tokio::select! {
            event = &mut console_next => {
                if let Some(event) = event {
                    println!(
                        "console {:?}: {}",
                        event.r#type,
                        event
                            .args
                            .iter()
                            .filter_map(|a| a.value.as_ref().map(|v| format!("{:?}", v)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                } else {
                    return Err(anyhow!("Console events channel ended unexpectedly"));
                }
                console_next = console_events.next();
            }
            event = &mut errors_next => {
                if let Some(event) = event {
                    let mut message = String::new();
                    let d = &event.exception_details;
                    writeln!(message, "{}", d.text)?;
                    if let Some(RemoteObject { preview: Some(ref exception), .. }) = d.exception {
                        if let Some(PropertyPreview{ value: Some(ref exception_message), .. }) = exception.properties.iter().find(|p| p.name == "message") {
                            writeln!(message, "{}", exception_message)?;
                        }
                    }
                    if let Some(stack_trace) = &d.stack_trace {
                        for frame in &stack_trace.call_frames {
                            writeln!(message, "    at {} ({}:{}:{})", frame.function_name, frame.url, frame.line_number, frame.column_number)?;
                        }
                    }
                    let message = message.trim_end();
                    if !is_debugging {
                        return Err(anyhow!(
                            "Exception throw in page: {}",
                            message
                        ))
                    } else {
                        println!("Exception throw in page (this would fail the test case without TURBOPACK_DEBUG_BROWSER):\n{}", message);
                    }
                } else {
                    return Err(anyhow!("Error events channel ended unexpectedly"));
                }
                errors_next = errors.next();
            }
            event = &mut bindings_next => {
                if event.is_some() {
                    if is_debugging {
                        let run_tests_msg =
                            "Entering debug mode. Run `await __jest__.run()` in the browser console to run tests.";
                        println!("\n\n{}", run_tests_msg);
                        page.evaluate(format!(
                            r#"console.info("%cTurbopack tests:", "font-weight: bold;", "{}");"#,
                            run_tests_msg
                        ))
                        .await?;
                    } else {
                        let value = page.evaluate("__jest__.run()").await?.into_value()?;
                        return Ok(value);
                    }
                } else {
                    return Err(anyhow!("Binding events channel ended unexpectedly"));
                }
                bindings_next = binding_events.next();
            }
            event = &mut network_next => {
                if let Some(event) = event {
                    println!("network {} [{}]", event.response.url, event.response.status);
                } else {
                    return Err(anyhow!("Network events channel ended unexpectedly"));
                }
                network_next = network_response_events.next();
            }
            result = handle.join_next() => {
                if let Some(result) = result {
                    result?;
                } else {
                    return Err(anyhow!("Browser closed"));
                }
            }
            () = tokio::time::sleep(Duration::from_secs(60)) => {
                if !is_debugging {
                    return Err(anyhow!("Test timeout while waiting for READY"));
                }
            }
        };
    }
}

fn get_free_local_addr() -> Result<SocketAddr, std::io::Error> {
    let socket = TcpSocket::new_v4()?;
    socket.bind("127.0.0.1:0".parse().unwrap())?;
    socket.local_addr()
}

async fn get_mock_server_future(mock_dir: &Path) -> Result<(), String> {
    if mock_dir.exists() {
        let port = get_free_local_addr().unwrap().port();
        env::set_var(
            "TURBOPACK_TEST_ONLY_MOCK_SERVER",
            format!("http://127.0.0.1:{}", port),
        );

        httpmock::standalone::start_standalone_server(
            port,
            false,
            Some(mock_dir.to_path_buf()),
            false,
            0,
        )
        .await
    } else {
        std::future::pending::<Result<(), String>>().await
    }
}

#[turbo_tasks::value(shared)]
struct TestIssueReporter {
    #[turbo_tasks(trace_ignore, debug_ignore)]
    pub issue_tx: State<Sender<(PlainIssueReadRef, ValueDebugStringReadRef)>>,
}

#[turbo_tasks::value_impl]
impl TestIssueReporterVc {
    #[turbo_tasks::function]
    fn new(
        issue_tx: TransientInstance<Sender<(PlainIssueReadRef, ValueDebugStringReadRef)>>,
    ) -> Self {
        TestIssueReporter {
            issue_tx: State::new((*issue_tx).clone()),
        }
        .cell()
    }
}

#[turbo_tasks::value_impl]
impl IssueReporter for TestIssueReporter {
    #[turbo_tasks::function]
    async fn report_issues(
        &self,
        captured_issues: TransientInstance<ReadRef<CapturedIssues>>,
        _source: TransientValue<RawVc>,
    ) -> Result<BoolVc> {
        let issue_tx = self.issue_tx.get_untracked().clone();
        for issue in captured_issues.iter() {
            let plain = issue.into_plain();
            issue_tx.send((plain.await?, plain.dbg().await?)).await?;
        }

        Ok(BoolVc::cell(false))
    }
}

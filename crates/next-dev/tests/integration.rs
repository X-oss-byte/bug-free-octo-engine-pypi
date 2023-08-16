#![cfg(test)]
extern crate test_generator;

use std::{
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use chromiumoxide::{
    browser::{Browser, BrowserConfig},
    error::CdpError::Ws,
};
use futures::StreamExt;
use lazy_static::lazy_static;
use next_dev::{register, EntryRequest, NextDevServerBuilder};
use owo_colors::OwoColorize;
use serde::Deserialize;
use test_generator::test_resources;
use tokio::{net::TcpSocket, task::JoinHandle};
use tungstenite::{error::ProtocolError::ResetWithoutClosingHandshake, Error::Protocol};
use turbo_tasks::TurboTasks;
use turbo_tasks_fs::util::sys_to_unix;
use turbo_tasks_memory::MemoryBackend;
use turbo_tasks_testing::retry::retry_async;

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
    // `TURBOPACK_DEBUG_BROWSER=1 cargo test -p next-dev -- test_my_pattern --nocapture`
    static ref DEBUG_BROWSER: bool = env::var("TURBOPACK_DEBUG_BROWSER").is_ok();
}

#[test_resources("crates/next-dev/tests/integration/*/*/*")]
#[tokio::main(flavor = "current_thread")]
async fn test(resource: &str) {
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

    let run_result = run_test(resource).await;

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

#[test_resources("crates/next-dev/tests/integration/*/*/__skipped__/*")]
#[should_panic]
#[tokio::main]
async fn test_skipped_fails(resource: &str) {
    let run_result = run_test(resource).await;

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
        .strip_prefix("crates/next-dev")
        .unwrap();
    assert!(path.exists(), "{} does not exist", resource);

    assert!(
        path.is_dir(),
        "{} is not a directory. Integration tests must be directories.",
        path.to_str().unwrap()
    );

    // Count the number of dirs _under_ crates/next-dev/tests
    let test_entry = Path::new(resource).join("index.js");
    let package_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = package_root.parent().unwrap().parent().unwrap();
    let project_dir = workspace_root.join(resource);
    let requested_addr = get_free_local_addr().unwrap();

    let mock_dir = path.join("__httpmock__");
    let mock_server_future = get_mock_server_future(&mock_dir);

    let server = NextDevServerBuilder::new(
        TurboTasks::new(MemoryBackend::new()),
        sys_to_unix(&project_dir.to_string_lossy()).to_string(),
        sys_to_unix(&workspace_root.to_string_lossy()).to_string(),
    )
    .entry_request(EntryRequest::Module(
        "@turbo/pack-test-harness".to_string(),
        "".to_string(),
    ))
    .entry_request(EntryRequest::Relative(
        sys_to_unix(test_entry.strip_prefix(resource).unwrap().to_str().unwrap()).to_string(),
    ))
    .eager_compile(false)
    .hostname(requested_addr.ip())
    .port(requested_addr.port())
    .log_level(turbopack_core::issue::IssueSeverity::Warning)
    .log_detail(true)
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

    result
}

async fn create_browser(is_debugging: bool) -> Result<(Browser, JoinHandle<()>)> {
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
    // See https://crates.io/crates/chromiumoxide
    let thread_handle = tokio::task::spawn(async move {
        loop {
            if let Err(Ws(Protocol(ResetWithoutClosingHandshake))) = handler.next().await.unwrap() {
                // The user has most likely closed the browser. End gracefully.
                break;
            }
        }
    });

    Ok((browser, thread_handle))
}

async fn run_browser(addr: SocketAddr) -> Result<JestRunResult> {
    if *DEBUG_BROWSER {
        run_debug_browser(addr).await?;
    }

    run_test_browser(addr).await
}

async fn run_debug_browser(addr: SocketAddr) -> Result<()> {
    let (browser, handle) = create_browser(true).await?;
    let page = browser.new_page(format!("http://{}", addr)).await?;

    let run_tests_msg =
        "Entering debug mode. Run `await __jest__.run()` in the browser console to run tests.";
    println!("\n\n{}", run_tests_msg);
    page.evaluate(format!(
        r#"console.info("%cTurbopack tests:", "font-weight: bold;", "{}");"#,
        run_tests_msg
    ))
    .await?;

    // Wait for the user to close the browser
    handle.await?;

    Ok(())
}

async fn run_test_browser(addr: SocketAddr) -> Result<JestRunResult> {
    let (browser, _) = create_browser(false).await?;

    // `browser.new_page()` opens a tab, navigates to the destination, and waits for
    // the page to load. chromiumoxide/Chrome DevTools Protocol has been flakey,
    // returning `ChannelSendError`s (WEB-259). Retry if necessary.
    let page = retry_async(
        (),
        |_| browser.new_page(format!("http://{}", addr)),
        5,
        Duration::from_millis(100),
    )
    .await
    .context("Failed to create new browser page")?;

    let value = page.evaluate("globalThis.waitForTests?.() ?? __jest__.run()");
    Ok(value.await?.into_value()?)
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

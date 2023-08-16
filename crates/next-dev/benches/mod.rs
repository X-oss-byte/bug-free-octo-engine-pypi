#![feature(map_try_insert)]

use std::{
    collections::HashMap,
    fs::{self},
    panic::AssertUnwindSafe,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{anyhow, Context, Result};
use bundlers::get_bundlers;
use criterion::{
    criterion_group, criterion_main,
    measurement::{Measurement, WallTime},
    BenchmarkGroup, BenchmarkId, Criterion,
};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use tokio::{
    runtime::Runtime,
    time::{sleep, timeout},
};
use turbo_tasks::util::FormatDuration;
use util::{
    build_test, create_browser,
    env::{read_env, read_env_bool, read_env_list},
    module_picker::ModulePicker,
    AsyncBencherExtension, PreparedApp, BINDING_NAME,
};

use self::{bundlers::RenderType, util::resume_on_error};
use crate::{bundlers::Bundler, util::PageGuard};

mod bundlers;
mod util;

const MAX_UPDATE_TIMEOUT: Duration = Duration::from_secs(60);

fn bench_startup(c: &mut Criterion) {
    let mut g = c.benchmark_group("bench_startup");
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(60));

    bench_startup_internal(g, false);
}

fn bench_hydration(c: &mut Criterion) {
    let mut g = c.benchmark_group("bench_hydration");
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(60));

    bench_startup_internal(g, true);
}

fn bench_startup_internal(mut g: BenchmarkGroup<WallTime>, hydration: bool) {
    let runtime = Runtime::new().unwrap();
    let browser = Lazy::new(|| runtime.block_on(create_browser()));

    for bundler in get_bundlers() {
        let wait_for_hydration = match bundler.render_type() {
            RenderType::ClientSideRendered => {
                // For bundlers without server rendered html "startup" means time to hydration
                // as they only render an empty screen without hydration. Since startup and
                // hydration would be the same we skip the hydration benchmark for them.
                if hydration {
                    continue;
                } else {
                    true
                }
            }
            RenderType::ServerSidePrerendered => hydration,
            RenderType::ServerSideRenderedWithEvents => hydration,
            RenderType::ServerSideRenderedWithoutInteractivity => {
                // For bundlers without interactivity there is no hydration event to wait for
                if hydration {
                    continue;
                } else {
                    false
                }
            }
        };
        for module_count in get_module_counts() {
            let test_app = Lazy::new(|| build_test(module_count, bundler.as_ref(), false));
            let input = (bundler.as_ref(), &test_app);
            resume_on_error(AssertUnwindSafe(|| {
                g.bench_with_input(
                    BenchmarkId::new(bundler.get_name(), format!("{} modules", module_count)),
                    &input,
                    |b, &(bundler, test_app)| {
                        let test_app = &**test_app;
                        let browser = &*browser;
                        b.to_async(&runtime).try_iter_custom(|iters, m| async move {
                            let mut value = m.zero();

                            for _ in 0..iters {
                                let mut app =
                                    PreparedApp::new(bundler, test_app.path().to_path_buf())
                                        .await?;
                                let start = m.start();
                                app.start_server()?;
                                let mut guard = app.with_page(browser).await?;
                                if wait_for_hydration {
                                    guard.wait_for_hydration().await?;
                                }
                                let duration = m.end(start);
                                value = m.add(&value, &duration);

                                // Defer the dropping of the guard.
                                drop(guard);
                            }
                            Ok(value)
                        });
                    },
                );
            }));
        }
    }
    g.finish();
}

#[derive(Copy, Clone)]
enum CodeLocation {
    Effect,
    Evaluation,
}

fn bench_hmr_to_eval(c: &mut Criterion) {
    let mut g = c.benchmark_group("bench_hmr_to_eval");
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(60));

    bench_hmr_internal(g, CodeLocation::Evaluation);
}

fn bench_hmr_to_commit(c: &mut Criterion) {
    let mut g = c.benchmark_group("bench_hmr_to_commit");
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(60));

    bench_hmr_internal(g, CodeLocation::Effect);
}

fn bench_hmr_internal(mut g: BenchmarkGroup<WallTime>, location: CodeLocation) {
    // Only capture one sample for warmup
    g.warm_up_time(Duration::from_millis(1));

    let runtime = Runtime::new().unwrap();
    let browser = Lazy::new(|| runtime.block_on(create_browser()));
    let hmr_warmup = read_env("TURBOPACK_BENCH_HMR_WARMUP", 10).unwrap();

    for bundler in get_bundlers() {
        if matches!(
            bundler.render_type(),
            RenderType::ServerSideRenderedWithEvents
                | RenderType::ServerSideRenderedWithoutInteractivity
        ) && matches!(location, CodeLocation::Evaluation)
        {
            // We can't measure evaluation time for these bundlers since it's not evaluated
            // in the browser
            continue;
        }
        for module_count in get_module_counts() {
            let test_app = Lazy::new(|| build_test(module_count, bundler.as_ref(), true));
            let input = (bundler.as_ref(), &test_app);
            let module_picker =
                Lazy::new(|| Arc::new(ModulePicker::new(test_app.modules().to_vec())));

            resume_on_error(AssertUnwindSafe(|| {
                g.bench_with_input(
                    BenchmarkId::new(bundler.get_name(), format!("{} modules", module_count)),
                    &input,
                    |b, &(bundler, test_app)| {
                        let test_app = &**test_app;
                        let modules = test_app.modules();
                        let original_contents = &Mutex::new(HashMap::new());
                        let module_picker = &*module_picker;
                        let browser = &*browser;

                        b.to_async(&runtime).try_iter_async(
                            &runtime,
                            || async {
                                let mut app = PreparedApp::new_without_copy(
                                    bundler,
                                    test_app.path().to_path_buf(),
                                )
                                .await?;
                                app.start_server()?;
                                let mut guard = app.with_page(browser).await?;
                                if bundler.has_hydration_event() {
                                    guard.wait_for_hydration().await?;
                                } else {
                                    guard.page().wait_for_navigation().await?;
                                }
                                guard
                                    .page()
                                    .evaluate_expression("globalThis.HMR_IS_HAPPENING = true")
                                    .await
                                    .context(
                                        "Unable to evaluate JavaScript in the page for HMR check \
                                         flag",
                                    )?;

                                // There's a possible race condition between hydration and
                                // connection to the HMR server. We attempt to make updates with an
                                // exponential backoff until one succeeds.
                                let mut exponential_duration = Duration::from_millis(100);
                                loop {
                                    match make_change(
                                        &modules[0].0,
                                        bundler,
                                        &mut guard,
                                        location,
                                        exponential_duration,
                                        &WallTime,
                                        &mut *original_contents.lock(),
                                    )
                                    .await
                                    {
                                        Ok(_) => {
                                            break;
                                        }
                                        Err(e) => {
                                            exponential_duration *= 2;
                                            if exponential_duration > MAX_UPDATE_TIMEOUT {
                                                return Err(
                                                    e.context("failed to make warmup change")
                                                );
                                            }
                                        }
                                    }
                                }

                                // Once we know the HMR server is connected, we make a few warmup
                                // changes.
                                for _ in 0..hmr_warmup {
                                    make_change(
                                        &modules[0].0,
                                        bundler,
                                        &mut guard,
                                        location,
                                        MAX_UPDATE_TIMEOUT,
                                        &WallTime,
                                        &mut *original_contents.lock(),
                                    )
                                    .await?;
                                }

                                Ok(guard)
                            },
                            |mut guard, iters, m, verbose| async move {
                                let mut value = m.zero();
                                for iter in 0..iters {
                                    let module = module_picker.pick();
                                    let duration = make_change(
                                        &module,
                                        bundler,
                                        &mut guard,
                                        location,
                                        MAX_UPDATE_TIMEOUT,
                                        &m,
                                        &mut *original_contents.lock(),
                                    )
                                    .await?;
                                    value = m.add(&value, &duration);

                                    let i: u64 = iter + 1;
                                    if verbose && i != iters && i.count_ones() == 1 {
                                        eprint!(
                                            " [{:?} {:?}/{}]",
                                            duration,
                                            FormatDuration(value / (i as u32)),
                                            i
                                        );
                                    }
                                }

                                Ok((guard, value))
                            },
                            |guard| async move {
                                let hmr_is_happening = guard
                                    .page()
                                    .evaluate_expression("globalThis.HMR_IS_HAPPENING")
                                    .await
                                    .unwrap();
                                // Make sure that we are really measuring HMR and not accidentically
                                // full refreshing the page
                                assert!(hmr_is_happening.value().unwrap().as_bool().unwrap());

                                // Reset app to the original version
                                for (module, original) in original_contents.lock().iter() {
                                    fs::write(module, original.as_bytes()).unwrap();
                                }
                            },
                        );
                    },
                );
            }));
        }
    }
}

fn insert_code<'a>(
    path: &'a Path,
    bundler: &dyn Bundler,
    message: &str,
    location: CodeLocation,
    original_contents: &mut HashMap<&'a Path, String>,
) -> Result<impl FnOnce() -> Result<()>> {
    let original = fs::read_to_string(path)?;
    let mut contents = original.clone();
    let _ = original_contents.try_insert(path, original);

    const PRAGMA_EVAL_START: &str = "/* @turbopack-bench:eval-start */";
    const PRAGMA_EVAL_END: &str = "/* @turbopack-bench:eval-end */";

    let eval_start = contents
        .find(PRAGMA_EVAL_START)
        .ok_or_else(|| anyhow!("unable to find effect start pragma in {}", contents))?;
    let eval_end = contents
        .find(PRAGMA_EVAL_END)
        .ok_or_else(|| anyhow!("unable to find effect end pragma in {}", contents))?;

    match (location, bundler.render_type()) {
        (CodeLocation::Effect, _) => {
            contents.replace_range(
                eval_start + PRAGMA_EVAL_START.len()..eval_end,
                &format!("\nEFFECTS = [\"{message}\"];\n"),
            );
        }
        (
            CodeLocation::Evaluation,
            RenderType::ClientSideRendered | RenderType::ServerSidePrerendered,
        ) => {
            let code = format!(
                "\nglobalThis.{BINDING_NAME} && globalThis.{BINDING_NAME}(\"{message}\");\n"
            );
            contents.replace_range(eval_start + PRAGMA_EVAL_START.len()..eval_end, &code);
        }
        (
            CodeLocation::Evaluation,
            RenderType::ServerSideRenderedWithEvents
            | RenderType::ServerSideRenderedWithoutInteractivity,
        ) => {
            panic!("evaluation can't be measured for bundlers which evaluate on server side");
        }
    }

    let path = path.to_owned();
    Ok(move || Ok(fs::write(&path, contents)?))
}

static CHANGE_TIMEOUT_MESSAGE: &str = "update was not registered by bundler";

async fn make_change<'a, 'b>(
    module: &'b Path,
    bundler: &dyn Bundler,
    guard: &mut PageGuard<'a>,
    location: CodeLocation,
    timeout_duration: Duration,
    measurement: &WallTime,
    original_contents: &mut HashMap<&'b Path, String>,
) -> Result<Duration> {
    static CHANGE_COUNTER: AtomicUsize = AtomicUsize::new(0);

    let msg = format!(
        "TURBOPACK_BENCH_CHANGE_{}",
        CHANGE_COUNTER.fetch_add(1, Ordering::Relaxed)
    );

    // Keep the IO out of the measurement.
    let commit = insert_code(module, bundler, &msg, location, original_contents)?;

    let start = measurement.start();

    commit()?;

    // Wait for the change introduced above to be reflected at runtime.
    // This expects HMR or automatic reloading to occur.
    timeout(timeout_duration, guard.wait_for_binding(&msg))
        .await
        .context(CHANGE_TIMEOUT_MESSAGE)??;

    let duration = measurement.end(start);

    if cfg!(target_os = "linux") {
        // TODO(sokra) triggering HMR updates too fast can have weird effects on Linux
        tokio::time::sleep(std::cmp::max(duration, Duration::from_millis(100))).await;
    }
    Ok(duration)
}

fn bench_startup_cached(c: &mut Criterion) {
    let mut g = c.benchmark_group("bench_startup_cached");
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(60));

    bench_startup_cached_internal(g, false);
}

fn bench_hydration_cached(c: &mut Criterion) {
    let mut g = c.benchmark_group("bench_hydration_cached");
    g.sample_size(10);
    g.measurement_time(Duration::from_secs(60));

    bench_startup_cached_internal(g, true);
}

fn bench_startup_cached_internal(mut g: BenchmarkGroup<WallTime>, hydration: bool) {
    if !read_env_bool("TURBOPACK_BENCH_CACHED") {
        return;
    }

    let runtime = Runtime::new().unwrap();
    let browser = Lazy::new(|| runtime.block_on(create_browser()));

    for bundler in get_bundlers() {
        let wait_for_hydration = match bundler.render_type() {
            RenderType::ClientSideRendered => {
                // For bundlers without server rendered html "startup" means time to hydration
                // as they only render an empty screen without hydration. Since startup and
                // hydration would be the same we skip the hydration benchmark for them.
                if hydration {
                    continue;
                } else {
                    true
                }
            }
            RenderType::ServerSidePrerendered => hydration,
            RenderType::ServerSideRenderedWithEvents => hydration,
            RenderType::ServerSideRenderedWithoutInteractivity => {
                // For bundlers without interactivity there is no hydration event to wait for
                if hydration {
                    continue;
                } else {
                    false
                }
            }
        };
        for module_count in get_module_counts() {
            let test_app = Lazy::new(|| build_test(module_count, bundler.as_ref(), false));
            let input = (bundler.as_ref(), &test_app);

            resume_on_error(AssertUnwindSafe(|| {
                g.bench_with_input(
                    BenchmarkId::new(bundler.get_name(), format!("{} modules", module_count)),
                    &input,
                    |b, &(bundler, test_app)| {
                        let test_app = &**test_app;
                        let browser = &*browser;
                        b.to_async(&runtime).try_iter_custom(|iters, m| async move {
                            // Run a complete build, shut down, and test running it again
                            let mut app =
                                PreparedApp::new(bundler, test_app.path().to_path_buf()).await?;
                            app.start_server()?;
                            let mut guard = app.with_page(browser).await?;
                            if bundler.has_hydration_event() {
                                guard.wait_for_hydration().await?;
                            } else {
                                guard.page().wait_for_navigation().await?;
                            }

                            let mut app = guard.close_page().await?;

                            // Give it 4 seconds time to store the cache
                            sleep(Duration::from_secs(4)).await;

                            app.stop_server()?;

                            let mut value = m.zero();
                            for _ in 0..iters {
                                let start = m.start();
                                app.start_server()?;
                                let mut guard = app.with_page(browser).await?;
                                if wait_for_hydration {
                                    guard.wait_for_hydration().await?;
                                }
                                let duration = m.end(start);
                                value = m.add(&value, &duration);

                                app = guard.close_page().await?;
                                app.stop_server()?;
                            }

                            drop(app);
                            Ok(value)
                        });
                    },
                );
            }));
        }
    }
}

fn get_module_counts() -> Vec<usize> {
    read_env_list("TURBOPACK_BENCH_COUNTS", vec![1_000usize]).unwrap()
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = bench_startup, bench_hydration, bench_startup_cached, bench_hydration_cached, bench_hmr_to_eval, bench_hmr_to_commit
);
criterion_main!(benches);

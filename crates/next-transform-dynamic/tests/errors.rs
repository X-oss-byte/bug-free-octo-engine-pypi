use std::path::{Path, PathBuf};

use next_transform_dynamic::{next_dynamic, NextDynamicMode};
use swc_core::{
    common::FileName,
    ecma::{
        parser::{EsConfig, Syntax},
        transforms::testing::{test_fixture, FixtureTestConfig},
    },
};
use testing::fixture;

fn syntax() -> Syntax {
    Syntax::Es(EsConfig {
        jsx: true,
        ..Default::default()
    })
}

#[fixture("tests/errors/**/input.js")]
fn next_dynamic_errors(input: PathBuf) {
    next_dynamic_errors_run(&input, "output-webpack.js", NextDynamicMode::Webpack);

    next_dynamic_errors_run(&input, "output-turbo.js", NextDynamicMode::Turbo);
}

fn next_dynamic_errors_run(input: &Path, output: &str, mode: NextDynamicMode) {
    let output = input.parent().unwrap().join(output);
    test_fixture(
        syntax(),
        &|_tr| {
            next_dynamic(
                true,
                false,
                false,
                mode,
                FileName::Real(PathBuf::from("/some-project/src/some-file.js")),
                Some("/some-project/src".into()),
            )
        },
        input,
        &output,
        FixtureTestConfig {
            allow_error: true,
            ..Default::default()
        },
    );
}

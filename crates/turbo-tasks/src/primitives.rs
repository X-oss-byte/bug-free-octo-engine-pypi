use std::ops::Deref;

use auto_hash_map::AutoSet;
// This specific macro identifier is detected by turbo-tasks-build.
use turbo_tasks_macros::primitive as __turbo_tasks_internal_primitive;

use crate::{self as turbo_tasks, RawVc, Vc};

__turbo_tasks_internal_primitive!(());
__turbo_tasks_internal_primitive!(String);

#[turbo_tasks::function]
fn empty_string() -> Vc<String> {
    Vc::cell(String::new())
}

impl Vc<String> {
    #[inline(always)]
    pub fn empty() -> Vc<String> {
        empty_string()
    }
}

__turbo_tasks_internal_primitive!(Option<String>);
__turbo_tasks_internal_primitive!(Vec<String>);

#[turbo_tasks::function]
fn empty_string_vec() -> Vc<Vec<String>> {
    Vc::cell(Vec::new())
}

impl Vc<Vec<String>> {
    #[inline(always)]
    pub fn empty() -> Vc<Vec<String>> {
        empty_string_vec()
    }
}

__turbo_tasks_internal_primitive!(Option<u16>);

__turbo_tasks_internal_primitive!(bool);

__turbo_tasks_internal_primitive!(u8);
__turbo_tasks_internal_primitive!(u16);
__turbo_tasks_internal_primitive!(u32);
__turbo_tasks_internal_primitive!(u64);
__turbo_tasks_internal_primitive!(u128);
__turbo_tasks_internal_primitive!(i8);
__turbo_tasks_internal_primitive!(i16);
__turbo_tasks_internal_primitive!(i32);
__turbo_tasks_internal_primitive!(i64);
__turbo_tasks_internal_primitive!(i128);
__turbo_tasks_internal_primitive!(usize);
__turbo_tasks_internal_primitive!(isize);
__turbo_tasks_internal_primitive!(AutoSet<RawVc>);
__turbo_tasks_internal_primitive!(serde_json::Value);
__turbo_tasks_internal_primitive!(Vec<u8>);

#[turbo_tasks::value(transparent, eq = "manual")]
#[derive(Debug, Clone)]
pub struct Regex(
    #[turbo_tasks(trace_ignore)]
    #[serde(with = "serde_regex")]
    pub regex::Regex,
);

impl Deref for Regex {
    type Target = regex::Regex;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq for Regex {
    fn eq(&self, other: &Regex) -> bool {
        // Context: https://github.com/rust-lang/regex/issues/313#issuecomment-269898900
        self.0.as_str() == other.0.as_str()
    }
}
impl Eq for Regex {}

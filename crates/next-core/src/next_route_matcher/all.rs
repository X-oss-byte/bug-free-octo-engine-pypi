use serde::{Deserialize, Serialize};
use turbo_tasks::primitives::BoolVc;
use turbopack_node::route_matcher::{ParamsVc, RouteMatcher};

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct AllMatch;

impl RouteMatcher for AllMatch {
    fn matches(&self, _path: &str) -> BoolVc {
        BoolVc::cell(true)
    }

    fn params(&self, _path: &str) -> ParamsVc {
        ParamsVc::cell(Some(Default::default()))
    }
}

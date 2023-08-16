use std::marker::PhantomData;

use crate::{self as turbo_tasks, RawVc, Vc};

/// Just an empty type, but it's never equal to itself.
/// [CompletionVc] can be used as return value instead of `()`
/// to have a concrete reference that can be awaited.
/// It will invalidate the awaiting task everytime the referenced
/// task has been executed.
#[turbo_tasks::value(cell = "new")]
pub struct Completion;

// no #[turbo_tasks::value_impl] to inline new into the caller task
// this ensures it's re-created on each execution
impl Completion {
    pub fn new() -> Vc<Self> {
        Completion::cell(Completion)
    }

    /// Uses the previous completion. Can be used to cancel without triggering a
    /// new invalidation.
    pub fn unchanged() -> Vc<Self> {
        // This is the same code that CompletionVc::cell uses except that it
        // only updates the cell when it is empty (CompletionVc::cell opted-out of
        // that via `#[turbo_tasks::value(cell = "new")]`)
        let cell = turbo_tasks::macro_helpers::find_cell_by_type(*COMPLETION_VALUE_TYPE_ID);
        cell.conditional_update_shared(|old| old.is_none().then_some(Completion));
        let raw: RawVc = cell.into();
        Vc {
            node: raw,
            _t: PhantomData,
        }
    }
}

#[turbo_tasks::value(transparent)]
pub struct Completions(Vec<Vc<Completion>>);

#[turbo_tasks::value_impl]
impl Completions {
    #[turbo_tasks::function]
    pub async fn all(self: turbo_tasks::Vc<Self>) -> anyhow::Result<Vc<Completion>> {
        for c in self.await?.iter() {
            c.await?;
        }
        Ok(Completion::new())
    }
}

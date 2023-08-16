use std::{
    borrow::Cow,
    fmt::{Debug, Display},
    mem::take,
};

use anyhow::{anyhow, Error, Result};
use auto_hash_map::AutoSet;
use nohash_hasher::BuildNoHashHasher;
use turbo_tasks::{util::SharedError, RawVc, TaskId, TurboTasksBackendApi};

#[derive(Default, Debug)]
pub struct Output {
    pub(crate) content: OutputContent,
    updates: u32,
    pub(crate) dependent_tasks: AutoSet<TaskId, BuildNoHashHasher<TaskId>>,
}

#[derive(Clone, Debug)]
pub enum OutputContent {
    Empty,
    Link(RawVc),
    Error(SharedError),
    Panic(Option<Cow<'static, str>>),
}

impl Default for OutputContent {
    fn default() -> Self {
        OutputContent::Empty
    }
}

impl Display for OutputContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputContent::Empty => write!(f, "empty"),
            OutputContent::Link(raw_vc) => write!(f, "link {}", raw_vc),
            OutputContent::Error(err) => write!(f, "error {}", err),
            OutputContent::Panic(Some(message)) => write!(f, "panic {}", message),
            OutputContent::Panic(None) => write!(f, "panic"),
        }
    }
}

impl Output {
    pub fn read(&mut self, reader: TaskId) -> Result<RawVc> {
        self.dependent_tasks.insert(reader);
        self.read_untracked()
    }

    /// INVALIDATION: Be careful with this, it will not track dependencies, so
    /// using it could break cache invalidation.
    pub fn read_untracked(&mut self) -> Result<RawVc> {
        match &self.content {
            OutputContent::Empty => Err(anyhow!("Output is empty")),
            OutputContent::Error(err) => Err(anyhow::Error::new(err.clone())),
            OutputContent::Link(raw_vc) => Ok(*raw_vc),
            OutputContent::Panic(Some(message)) => Err(anyhow!("A task panicked: {message}")),
            OutputContent::Panic(None) => Err(anyhow!("A task panicked")),
        }
    }

    pub fn link(&mut self, target: RawVc, turbo_tasks: &dyn TurboTasksBackendApi) {
        debug_assert!(*self != target);
        self.assign(OutputContent::Link(target), turbo_tasks)
    }

    pub fn error(&mut self, error: Error, turbo_tasks: &dyn TurboTasksBackendApi) {
        self.content = OutputContent::Error(SharedError::new(error));
        self.updates += 1;
        // notify
        if !self.dependent_tasks.is_empty() {
            turbo_tasks.schedule_notify_tasks_set(&take(&mut self.dependent_tasks));
        }
    }

    pub fn panic(
        &mut self,
        message: Option<Cow<'static, str>>,
        turbo_tasks: &dyn TurboTasksBackendApi,
    ) {
        self.content = OutputContent::Panic(message);
        self.updates += 1;
        // notify
        if !self.dependent_tasks.is_empty() {
            turbo_tasks.schedule_notify_tasks_set(&take(&mut self.dependent_tasks));
        }
    }

    pub fn assign(&mut self, content: OutputContent, turbo_tasks: &dyn TurboTasksBackendApi) {
        self.content = content;
        self.updates += 1;
        // notify
        if !self.dependent_tasks.is_empty() {
            turbo_tasks.schedule_notify_tasks_set(&take(&mut self.dependent_tasks));
        }
    }

    pub fn dependent_tasks(&self) -> &AutoSet<TaskId, BuildNoHashHasher<TaskId>> {
        &self.dependent_tasks
    }

    pub fn gc_drop(self, turbo_tasks: &dyn TurboTasksBackendApi) {
        // notify
        if !self.dependent_tasks.is_empty() {
            turbo_tasks.schedule_notify_tasks_set(&self.dependent_tasks);
        }
    }
}

impl PartialEq<RawVc> for Output {
    fn eq(&self, rhs: &RawVc) -> bool {
        match &self.content {
            OutputContent::Link(old_target) => old_target == rhs,
            OutputContent::Empty | OutputContent::Error(_) | OutputContent::Panic(_) => false,
        }
    }
}

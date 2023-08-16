//! hash module
//!
//! This module contains the hash functions used by turborepo for certain
//! data-types. This is managed using capnproto for deterministic hashing across
//! languages and platforms.

use std::collections::HashMap;

use capnp::{
    message::{HeapAllocator, TypedBuilder},
    serialize, serialize_packed,
};
use xxhash_rust::xxh64::xxh64;

use crate::cli::EnvMode;

mod proto_capnp {
    use crate::cli::EnvMode;

    include!(concat!(env!("OUT_DIR"), "/src/hash/proto_capnp.rs"));

    impl From<EnvMode> for global_hashable::EnvMode {
        fn from(value: EnvMode) -> Self {
            match value {
                EnvMode::Infer => global_hashable::EnvMode::Infer,
                EnvMode::Loose => global_hashable::EnvMode::Loose,
                EnvMode::Strict => global_hashable::EnvMode::Strict,
            }
        }
    }

    impl From<EnvMode> for task_hashable::EnvMode {
        fn from(value: EnvMode) -> Self {
            match value {
                EnvMode::Infer => task_hashable::EnvMode::Infer,
                EnvMode::Loose => task_hashable::EnvMode::Loose,
                EnvMode::Strict => task_hashable::EnvMode::Strict,
            }
        }
    }
}

struct TaskHashable {
    // hashes
    global_hash: String,
    task_dependency_hashes: Vec<String>,
    hash_of_files: String,
    external_deps_hash: String,

    // task
    package_dir: turbopath::RelativeUnixPathBuf,
    task: String,
    outputs: TaskOutputs,
    pass_thru_args: Vec<String>,

    // env
    env: Vec<String>,
    resolved_env_vars: EnvVarPairs,
    pass_thru_env: Vec<String>,
    env_mode: EnvMode,
    dot_env: Vec<turbopath::RelativeUnixPathBuf>,
}

pub struct GlobalHashable {
    global_cache_key: String,
    global_file_hash_map: HashMap<turbopath::RelativeUnixPathBuf, String>,
    root_external_deps_hash: String,
    env: Vec<String>,
    resolved_env_vars: Vec<String>,
    pass_through_env: Vec<String>,
    env_mode: EnvMode,
    framework_inference: bool,
    dot_env: Vec<turbopath::RelativeUnixPathBuf>,
}

struct TaskOutputs {
    inclusions: Vec<String>,
    exclusions: Vec<String>,
}

impl From<TaskOutputs> for TypedBuilder<proto_capnp::task_outputs::Owned, HeapAllocator> {
    fn from(value: TaskOutputs) -> Self {
        let mut message = ::capnp::message::TypedBuilder::<
            proto_capnp::task_outputs::Owned,
            HeapAllocator,
        >::new_default();
        let mut builder = message.init_root();

        {
            let mut inclusions = builder
                .reborrow()
                .init_inclusions(value.inclusions.len() as u32);
            for (i, inclusion) in value.inclusions.iter().enumerate() {
                inclusions.set(i as u32, inclusion);
            }
        }

        {
            let mut exclusions = builder
                .reborrow()
                .init_exclusions(value.exclusions.len() as u32);
            for (i, exclusion) in value.exclusions.iter().enumerate() {
                exclusions.set(i as u32, exclusion);
            }
        }

        message
    }
}

impl TaskHashable {
    pub fn hash(self) -> u64 {
        let mut buf = Vec::new();
        let write = std::io::BufWriter::new(&mut buf);

        let reader: TypedBuilder<_, _> = self.into();
        serialize::write_message(write, &reader.into_inner()).expect("works");

        xxh64(&buf, 0)
    }
}

impl GlobalHashable {
    pub fn hash(self) -> u64 {
        let mut buf = Vec::new();
        let write = std::io::BufWriter::new(&mut buf);

        let reader: TypedBuilder<_, _> = self.into();
        serialize::write_message(write, &reader.into_inner()).expect("works");

        xxh64(&buf, 0)
    }
}

type EnvVarPairs = Vec<String>;

impl From<TaskHashable> for TypedBuilder<proto_capnp::task_hashable::Owned, HeapAllocator> {
    fn from(task_hashable: TaskHashable) -> Self {
        let mut message =
            ::capnp::message::TypedBuilder::<proto_capnp::task_hashable::Owned>::new_default();
        let mut builder = message.init_root();

        builder.set_global_hash(&task_hashable.global_hash);
        builder.set_package_dir(&task_hashable.package_dir.to_string());
        builder.set_hash_of_files(&task_hashable.hash_of_files);
        builder.set_external_deps_hash(&task_hashable.external_deps_hash);
        builder.set_task(&task_hashable.task);
        builder.set_env_mode(task_hashable.env_mode.into());

        {
            let output_builder: TypedBuilder<_, _> = task_hashable.outputs.into();
            builder
                .set_outputs(output_builder.get_root_as_reader().unwrap())
                .unwrap();
        }

        {
            let mut task_dependency_hashes_builder = builder
                .reborrow()
                .init_task_dependency_hashes(task_hashable.task_dependency_hashes.len() as u32);
            for (i, hash) in task_hashable.task_dependency_hashes.iter().enumerate() {
                task_dependency_hashes_builder.set(i as u32, hash);
            }
        }

        {
            let mut pass_thru_args_builder = builder
                .reborrow()
                .init_pass_thru_args(task_hashable.pass_thru_args.len() as u32);
            for (i, arg) in task_hashable.pass_thru_args.iter().enumerate() {
                pass_thru_args_builder.set(i as u32, arg);
            }
        }

        {
            let mut env_builder = builder.reborrow().init_env(task_hashable.env.len() as u32);
            for (i, env) in task_hashable.env.iter().enumerate() {
                env_builder.set(i as u32, env);
            }
        }

        {
            let mut pass_thru_env_builder = builder
                .reborrow()
                .init_pass_thru_env(task_hashable.pass_thru_env.len() as u32);
            for (i, env) in task_hashable.pass_thru_env.iter().enumerate() {
                pass_thru_env_builder.set(i as u32, env);
            }
        }

        {
            let mut dotenv_builder = builder
                .reborrow()
                .init_dot_env(task_hashable.dot_env.len() as u32);
            for (i, env) in task_hashable.dot_env.iter().enumerate() {
                dotenv_builder.set(i as u32, env.as_str());
            }
        }

        {
            let mut resolved_env_vars_builder = builder
                .reborrow()
                .init_resolved_env_vars(task_hashable.resolved_env_vars.len() as u32);
            for (i, env) in task_hashable.resolved_env_vars.iter().enumerate() {
                resolved_env_vars_builder.set(i as u32, env);
            }
        }

        message
    }
}

impl From<GlobalHashable> for TypedBuilder<proto_capnp::global_hashable::Owned, HeapAllocator> {
    fn from(hashable: GlobalHashable) -> Self {
        let mut message =
            ::capnp::message::TypedBuilder::<proto_capnp::global_hashable::Owned>::new_default();

        let mut global_hashable = message.init_root();

        global_hashable.set_global_cache_key(&hashable.global_cache_key);

        {
            let mut entries = global_hashable
                .reborrow()
                .init_global_file_hash_map(hashable.global_file_hash_map.len() as u32);

            // get a sorted iterator over keys and values of the hashmap
            // and set the entries in the capnp message

            let mut hashable: Vec<_> = hashable.global_file_hash_map.into_iter().collect();
            hashable.sort_by(|a, b| a.0.cmp(&b.0));

            for (i, (key, value)) in hashable.iter().enumerate() {
                let mut entry = entries.reborrow().get(i as u32);
                entry.set_key(key.as_str());
                entry.set_value(&value);
            }
        }

        global_hashable.set_root_external_deps_hash(&hashable.root_external_deps_hash);

        {
            let mut entries = global_hashable
                .reborrow()
                .init_env(hashable.env.len() as u32);
            for (i, env) in hashable.env.iter().enumerate() {
                entries.set(i as u32, env);
            }
        }

        {
            let mut resolved_env_vars = global_hashable
                .reborrow()
                .init_resolved_env_vars(hashable.resolved_env_vars.len() as u32);
            for (i, env) in hashable.resolved_env_vars.iter().enumerate() {
                resolved_env_vars.set(i as u32, env);
            }
        }

        {
            let mut pass_through_env = global_hashable
                .reborrow()
                .init_pass_through_env(hashable.pass_through_env.len() as u32);
            for (i, env) in hashable.pass_through_env.iter().enumerate() {
                pass_through_env.set(i as u32, env);
            }
        }

        global_hashable.set_env_mode(match hashable.env_mode {
            EnvMode::Infer => proto_capnp::global_hashable::EnvMode::Infer,
            EnvMode::Loose => proto_capnp::global_hashable::EnvMode::Loose,
            EnvMode::Strict => proto_capnp::global_hashable::EnvMode::Strict,
        });

        global_hashable.set_framework_inference(hashable.framework_inference);

        {
            let mut dot_env = global_hashable
                .reborrow()
                .init_dot_env(hashable.dot_env.len() as u32);
            for (i, env) in hashable.dot_env.iter().enumerate() {
                dot_env.set(i as u32, env.as_str());
            }
        }

        message.into()
    }
}

#[cfg(test)]
mod test {
    use super::{GlobalHashable, TaskHashable, TaskOutputs};
    use crate::cli::EnvMode;

    #[test]
    fn test_hash() {
        let task_hashable = TaskHashable {
            global_hash: "global_hash".to_string(),
            task_dependency_hashes: vec!["task_dependency_hash".to_string()],
            package_dir: turbopath::RelativeUnixPathBuf::new("package_dir").unwrap(),
            hash_of_files: "hash_of_files".to_string(),
            external_deps_hash: "external_deps_hash".to_string(),
            task: "task".to_string(),
            outputs: TaskOutputs {
                inclusions: vec!["inclusions".to_string()],
                exclusions: vec!["exclusions".to_string()],
            },
            pass_thru_args: vec!["pass_thru_args".to_string()],
            env: vec!["env".to_string()],
            resolved_env_vars: vec![],
            pass_thru_env: vec!["pass_thru_env".to_string()],
            env_mode: EnvMode::Infer,
            dot_env: vec![turbopath::RelativeUnixPathBuf::new("dotenv".to_string()).unwrap()],
        };

        assert_eq!(task_hashable.hash(), 0x5b222af1dea5828e);
    }

    #[test]
    fn test_global_hash() {
        let global_hash = GlobalHashable {
            global_cache_key: "global_cache_key".to_string(),
            global_file_hash_map: vec![(
                turbopath::RelativeUnixPathBuf::new("global_file_hash_map").unwrap(),
                "global_file_hash_map".to_string(),
            )]
            .into_iter()
            .collect(),
            root_external_deps_hash: "root_external_deps_hash".to_string(),
            env: vec!["env".to_string()],
            resolved_env_vars: vec![],
            pass_through_env: vec!["pass_through_env".to_string()],
            env_mode: EnvMode::Infer,
            framework_inference: true,

            dot_env: vec![turbopath::RelativeUnixPathBuf::new("dotenv".to_string()).unwrap()],
        };

        assert_eq!(global_hash.hash(), 0xafa6b9c8d52c2642);
    }
}

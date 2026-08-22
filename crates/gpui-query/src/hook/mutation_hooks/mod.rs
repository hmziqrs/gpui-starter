//! Mutation hooks and internals — `use_mutation`, `mutate`, `mutate_with_callbacks`,
//! and the internal retry loops.

mod hooks;
mod internals;

pub use hooks::{
    mutate, mutate_with_callbacks, use_mutation, use_mutation_state, use_mutation_with_options,
};

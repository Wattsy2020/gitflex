This repository contains a collection of tools for personal use, thus the tools are quite specialised for my needs and don't need to be configurable.

The repository is solely written in Rust. 
Write clean and idiomatic Rust code that is readable and maintainable by humans.
Make illegal states unrepresentable by modelling invariants in the type system (e.g. as enums / structs / traits) and enforcing them at API boundaries. 
When behavior, accepted inputs, or return types depend on a mode or state, prefer typestate, generic parameters, associated types, or mode-specific constructors and functions.
A caller should not be able to independently choose incompatible modes and operations.

Prefer functional style such as .iter() functions over for loops that transform data, or option.map() over manual if else.
However there is no need to take this too far, prefer a for loop instead of an .iter().for_each() that prints out or does some side effect.
Prefer a functional core with an imperative shell. Model domain operations as typed value transformations, 
keep side effects at module boundaries, and avoid unnecessary shared mutable state.

Write modular code to support code reuse and deduplication, design abstractions to represent frequently repeated patterns.
Split modules into files and intentionally design the API so that implementation details are private.
Use libraries where relevant, both those already in the cargo.toml and other useful libraries. For example use thiserror instead of manually implementing errors, use ratatui over solely relying on crossterm. Make other similar library choices to keep the codebase smaller and more maintainable

If you see something that could be changed that would clearly improve the codebase, change it.

Each tool is an independent Rust crate, the git root directory doesn't have a Cargo.toml, only its children project have one.
Run validation from every modified crate directory:

cargo fmt --check &&
cargo test --locked &&
cargo clippy --all-targets --locked -- -D warnings

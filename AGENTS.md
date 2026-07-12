This repository contains a collection of tools for personal use, thus the tools are quite specialised for my needs and don't need to be configurable.
Each directory is a separate rust project, the git root directory doesn't have a Cargo.toml, look at the child projects instead.

The repository is solely written in Rust. 
Write clean and idomatic Rust code that is readable and maintainable by humans.
Make illegal states unrepresentable though types and enums.
Prefer functional style such as .iter() functions over for loops, or option.map() over manual if else.

Write modular code to support code reuse and deduplication, design abstractions to represent frequently repeated patterns.
Split modules into files and intentionally design the API so that implementation details are private.
Use libraries where relevant, both those already in the cargo.toml and other useful libraries. For example use thiserror instead of manually implementing errors, use ratatui over solely relying on crossterm. Make other similar library choices to keep the codebase smaller and more maintainable

If you see something that could be changed that would clearly improve the codebase, change it.

Run these tools to validate your code: `cargo fmt --check && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`

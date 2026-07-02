# Repository Guidelines

## Project Structure & Module Organization
This repository is a single Rust crate for waveform analysis. Core code lives in `src/`, with the MCP server entry point in `src/main.rs`, the library in `src/lib.rs`, and the CLI binary in `src/bin/waveform-cli.rs`. Integration tests live under `tests/` as `*_tests.rs`. Design notes and format specs are in `docs/`. Helper tooling lives in `tools/`, and packaging scripts are in `scripts/`. `src/condition.lalrpop` is the source grammar; parser artifacts such as `parser.out` are generated and should not be edited by hand.

## Build, Test, and Development Commands
Run commands from the repository root:

- `cargo build` - debug build.
- `cargo build --release` - optimized build.
- `cargo run` - start the MCP server in stdio mode.
- `cargo run -- --http` - start the HTTP transport.
- `cargo test` - run the full test suite.
- `cargo test --test condition_tests` - run one integration test file.

Changing the LALRPOP grammar automatically re-runs code generation on the next build.

## Coding Style & Naming Conventions
Use standard Rust style: `snake_case` for functions, modules, and test files; `PascalCase` for types; short, descriptive names for analysis helpers and data models. Keep code idiomatic and prefer explicit types where they improve readability. No repo-specific formatter or linter config is checked in, so follow `cargo fmt` defaults before committing.

## Testing Guidelines
Add integration coverage in `tests/` for user-facing behavior and parser changes. Name test files `*_tests.rs` and keep fixtures small and deterministic. When modifying condition parsing, BFS logic, or protocol analysis, add or update the corresponding integration tests rather than relying only on manual runs.

## Commit & Pull Request Guidelines
Git history uses concise imperative commits, often with Conventional Commit-style scopes such as `feat(scope): ...`, `fix(scope): ...`, `docs: ...`, and `refactor: ...`. Prefer that style for new commits. Pull requests should describe the behavior change, list validation commands run, and link any related issue or design note. Include sample output when a change affects CLI or MCP responses.

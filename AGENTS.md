# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace defined in `Cargo.toml`. Shared libraries live in `crates/`, including price streams, Solana streaming, MEXC protobufs, CEX providers, and Ethereum DEX quote logic. Runnable services live in `bins/`, such as `aggregator-sol`, `amm-eth`, `arbitrade-eth`, `arbitrade`, and `arbitrade-dex-eth`. Solana Anchor program code is under `aggregator-prog/`, with Rust sources in `aggregator-prog/programs/aggregator-prog/src/` and TypeScript tests in `aggregator-prog/tests/`. Deployment assets live in `docker/`, `docker-compose*.yml`, `k8s-manifests/`, and `aws/`. Design notes are in `docs/`.

## Build, Test, and Development Commands

- `cargo build --workspace`: build all Rust crates and binaries.
- `cargo build --release -p aggregator-sol`: build one production binary; replace the package name as needed.
- `cargo run -p aggregator-sol`: run a workspace service locally.
- `cargo test --workspace`: run Rust unit and integration tests.
- `cargo fmt --all`: format Rust code.
- `cargo clippy --workspace --all-targets`: run Rust lint checks.
- `docker compose up --build`: start the Solana aggregator stack with Postgres/Hasura.
- `./start-services.sh`: build and run `amm-eth` and `arbitrade-eth` under PM2.
- `cd aggregator-prog && yarn install && anchor test`: run Anchor program tests.

## Coding Style & Naming Conventions

Use Rust 2021 conventions and `rustfmt` defaults: four-space indentation, `snake_case` modules/functions, `PascalCase` types, and `SCREAMING_SNAKE_CASE` constants. Keep service-specific code in `bins/<service>/src` and reusable logic in `crates/<crate>/src`. Prefer `anyhow` at application boundaries and typed errors with `thiserror` in libraries. In async code, avoid holding lock or map guards across `.await`.

## Testing Guidelines

Place Rust unit tests beside code in `mod tests` and integration tests under each crate's `tests/` directory. Async tests should use `#[tokio::test]`. Name tests after behavior, for example `loads_chain_config_from_toml` or `rejects_invalid_pool_state`. Run package-focused tests with `cargo test -p eth-dex-quote` before broad workspace tests when changing one crate.

## Commit & Pull Request Guidelines

Recent commits use concise Conventional Commit prefixes such as `fix:`, `perf:`, and `feat(aggregator):`. Follow that style and keep the subject imperative and specific. Pull requests should describe the service or crate changed, include test results, call out config or migration changes, and link related issues. Include screenshots only for dashboard or UI-facing changes.

## Security & Configuration Tips

Do not commit local secrets from `.env`, `.env.local`, `contracts/.env`, or Kubernetes secret files. Use the `*.template.yaml` files in `k8s-manifests/` for examples. When changing runtime configuration, document new environment variables in `.env.example` and relevant deployment manifests.

# Contributing

Short guide for buildathon contributors and reviewers.

## Development setup

1. `cp .env.example .env`
2. `cargo build --workspace`
3. `ollama serve` + model from `OLLAMA_MODEL`
4. `cargo run --bin esa-api`

Frontends: `frontend/` and `payment-simulator/` — `npm install && npm run dev`.

## Code structure

Rust workspace under `crates/`:

- `esa-core` — types, actions, audit
- `esa-state` — fabric, OCC
- `esa-agents` — four agents
- `esa-policy` — policy + verifier
- `esa-gateway` — execution + rollback
- `esa-runtime` — orchestrator
- `esa-api` — HTTP + benchmark binary

See [docs/architecture.md](docs/architecture.md).

## Running tests

```bash
make test
cargo test --workspace
./scripts/integration-test.sh
```

## Running benchmarks

```bash
make benchmark-quick
make benchmark
```

Document results in `benchmarkreport.md` and raw JSON — keep [docs/claims.md](docs/claims.md) consistent.

## Adding scenarios

1. Add entry to `benchmarks/scenarios/taxonomy.yaml`
2. Implement applicator in `crates/esa-api/src/benchmark.rs`
3. Extend harness loop in `benchmark_harness.rs`
4. Document in `benchmarks/scenarios.md`

## Pull request expectations

- No secrets in commits
- No inflated production / compliance claims in README
- Update [claims.md](docs/claims.md) when adding measurable claims
- Tests pass: `cargo test --workspace`

## Documentation

Engineering docs live in `docs/` — see [docs/README.md](docs/README.md).

# Contributing
Short guide for contributors and reviewers.

## Your first contribution

You can contribute without understanding every subsystem. Choose one small, verifiable task:

- [Check whether the setup instructions work from a clean checkout](https://github.com/sujithputta02/Esapay/issues/13)
- [Add deterministic failure-injection coverage](https://github.com/sujithputta02/Esapay/issues/14)
- [Document safe SDK timeout handling](https://github.com/sujithputta02/Esapay/issues/15)

### Suggested workflow

1. Read the issue and comment with your intended approach, especially before changing payment or safety behavior.
2. Fork the repository and create a focused branch.
3. Keep the change small; include a test or clear reproduction steps when practical.
4. Run the relevant checks and state exactly what you ran in the pull request.
5. Explain limitations and unverified behavior honestly. Do not include credentials or run tests against live payment systems.

Found something unexpected? Open an issue with your OS, setup steps, expected behavior, actual behavior, and redacted logs. Feedback about usability, architecture, security boundaries, and benchmark reproducibility is welcome.

---

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

# Changelog

Meaningful milestones only — no fabricated history.

## v1.0.5 — Titanium Auth Experience, Multi-Key Expiration & Idempotent CI/CD (2026)

- **Titanium Security & Auth Redesign**: Enterprise-grade full-screen split authentication UI with real-time password strength entropy calculation, confirmation checks, and secure session termination.
- **Granular API Key Management**: Support for multiple concurrent API keys per merchant with custom lifespan controls (30 days, 90 days, 1 year, or Never Expire) across both Test and Live environments.
- **Idempotent CI/CD Release Automation**: Upgraded GitHub Actions release pipelines with idempotent package checks (`CURRENT_VER == REMOTE_VER`) across NPM and PyPI, preventing duplicate release collisions.
- **Centered Hero Pay Simulation**: Redesigned front-and-center interactive payment simulator showcasing real-time routing decisions, P95 tail latency, and instant circuit-breaker trips.
- **Cross-Platform Package Synchronization**: Published `esapay-cli@1.0.5`, `esapay@1.0.5` (npm), and `esapay@1.0.5` (PyPI) along with native zero-dependency CLI binaries for Linux, macOS (Apple Silicon & Intel), and Windows.

## v1.0.4 — Enterprise Auth Documentation, Live Control Plane & SDK Reference (2026)

- **Comprehensive API & Auth Specification**: Documented developer endpoints for checkout simulation, gateway telemetry, circuit breakers, and state fabric inspection in `docs/api.md`.
- **Registry Documentation Overhaul**: Complete README refresh across NPM (`esapay-cli`, `esapay`) and PyPI (`esapay`) with copy-paste runnable snippets, CLI cheatsheets, and environment configuration guides (`ESA_API_KEY`, `ESA_BASE_URL`).
- **Resilient Client Connection Pooling**: Hardened connection negotiation, Bearer authorization header injection, and automated fallback when connecting to remote or containerized ESA instances.
- **Synchronized Multi-Platform Binaries**: Released compiled binaries for Linux (`x86_64`), macOS Intel (`x86_64`), macOS Apple Silicon (`aarch64`), and Windows (`x86_64`).

## v1.0.3 — Cloud Deployments, Hugging Face AI Brain & Dual-Sandbox Control Plane (2026)

- **Turnkey Cloud Deployment Blueprints**: Added `render.yaml` and `railway.json` blueprints for 1-click cloud deployments with `/healthz` alias support.
- **Hugging Face ZeroGPU AI Brain**: Integrated neural reasoning agent running on sovereign Hugging Face Spaces for continuous failure pattern recognition.
- **Dual-Sandbox Architecture**: Strict separation between Test Sandbox and Live Production Command Center workspaces with sandbox-restricted credential provisioning.
- **Automated CLI & SDK Auth**: Added automated OAuth callback listening, auto-credential resolution, and seamless environment switching.
- **Deterministic Docker Builds**: Aligned container runtime to Debian Bookworm (`rust:1-slim-bookworm`) resolving GLIBC mismatches across distribution targets.

## v1.0.2 — Resilient Standalone Mode & Command Center Direct Serve (2026)

- **Universal CLI (`esapay-cli@1.0.2`)**: Autonomous client-side Standalone Evaluation Mode when offline; graceful fallbacks for `gateways`, `health`, `workloads`, `agents`, `checkout`, and `doctor`.
- **TypeScript SDK (`esapay@1.0.2`)**: Dual-mode ESM (`import`) and CommonJS (`require`) export mappings.
- **ESA Command Center at `/`**: Backend directly serves the live Command Center on `http://localhost:8080/` via same-origin static file service (`ServeDir`).
- **Transaction-Driven Telemetry & Spikes**: Real customer payments (`POST /api/payments/checkout`) drive live StateFabric workload metrics, P95 tail latency, queue depth, and sub-second failovers.
- **Custom Server Selector**: Connect Command Center to any remote cluster, local port, or Kubernetes service.

## v1.0.1 — Public Open Source Release (2026)

- Published `esapay-cli` and `esapay` to NPM Registry.
- Published `esapay` Python SDK to PyPI.
- Added GitHub Actions publishing pipelines and automated verification.

## v1.0 — Public Launch (2026)

- Full documentation set: architecture, governance, agents, benchmarks, claims register
- B0/B1/B2 benchmark harness with 155-trial matrix
- Command Center UI + payment simulator (Razorpay Test Mode)
- Audit hash chain, replay API, effect measurement
- Optional kubectl scale side effects

## v0.5 — Rollback

- Snapshot rollback via `ROLLBACK` action and gateway pre-execution snapshots
- BENCH-11 execution-failure scenario in harness

## v0.4 — Effect verification

- `EffectMeasurement` post-execution comparison
- Effects API endpoints

## v0.3 — Gateway / OCC

- Action Gateway as sole mutation path
- `RULE_003_STALE_STATE` and commit-time version check
- Stale-state demo scenario

## v0.2 — Agent loop

- Monitor, Diagnosis (Ollama), Planning, Safety agents
- Orchestrator 5s autonomous loop

## v0.1 — Initial runtime

- In-memory `StateFabric`
- Typed `ActionType` IR
- Policy engine skeleton
- `esa-api` health + workloads

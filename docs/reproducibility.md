# Reproducibility

End-to-end path from clone to benchmark — using **commands that exist in this repository**.

## 1. Clone

```bash
git clone <repository-url>
cd ESA_paymentgateway
```

## 2. Environment

```bash
cp .env.example .env
# Edit Razorpay test keys if using live webhooks / checkout
```

**Rust:** 1.70+  
**Node:** 18+ for frontends  
**Optional:** Docker, Kind, kubectl, Ollama

## 3. Docker (optional infra)

```bash
docker compose up -d
```

Starts Postgres, Redis, NATS, Ollama, Prometheus, Grafana, `esa-api`, `frontend`.  
**Note:** API still uses **in-memory** `StateFabric` today; Compose DB URLs are not wired to persistence.

## 4. Kind / Kubernetes (optional)

```bash
kubectl apply -f k8s/deployments.yaml
```

Used for optional `kubectl scale` side effects in gateway.

## 5. Ollama

```bash
ollama pull mistral:latest   # or model in OLLAMA_MODEL
ollama serve
```

## 6. Start services (local dev)

```bash
cargo run --bin esa-api          # :8080
cd frontend && npm install && npm run dev    # :3000
cd payment-simulator && npm install && npm run dev  # :5173
```

One-shot: `./scripts/start-demo.sh`

## 7. Run demo

```bash
./scripts/run-demo-test.sh
# or
./scripts/demo.sh
```

See [demo.md](demo.md).

## 8. Run benchmark

```bash
make benchmark-quick      # ~seconds, smoke
make benchmark-smoke      # full agent cycle, 1 seed
make benchmark            # 5 seeds × 8 perf scenarios × 3 controllers + safety
```

With ablations:

```bash
cargo run --bin esa-benchmark -- --ablations
```

Outputs:

| Path | Content |
|------|---------|
| `benchmarks/raw/benchmark_results.json` | Per-trial records |
| `benchmarks/processed/aggregates.json` | Aggregates |
| `benchmarks/processed/ablations.json` | Ablation variants |
| `benchmarkreport.md` | Human summary (repo root) |
| `benchmarks/reports/benchmark_report.md` | Copy in reports folder |

## 9. Verify audit chain

```bash
make audit-verify
# or
cargo test --test tamper_detection_test
```

## 10. Run tests

```bash
make test
# or
cargo test --workspace
```

## Consistency

Numerical claims in docs should match:

- [`benchmarkreport.md`](../benchmarkreport.md)
- [`claims.md`](claims.md)
- Raw JSON in `benchmarks/raw/`

Hardware/OS in report reflect the machine where harness last ran (see report §1).

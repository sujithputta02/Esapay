# Baselines (B0, B1, B2)

Controller definitions used in the harness (`Controller` enum in `benchmark.rs`).

## B0 — Static rules (`B0_rules`)

| Aspect | Detail |
|--------|--------|
| Type | Static automation |
| Detection | 15.0s scrape interval (modeled) |
| Decision | Fixed thresholds: P95 &gt; 250ms or queue &gt; 1000 → scale +1 replica |
| Governance | No OCC, no typed IR gateway path for proposals |
| Role | **Lower bound** — simple ops playbooks |

**Last aggregate (5 seeds):** P95 ~236 ms, time above SLA ~16.5 s, total recovery ~24.6 s.

## B1 — Adaptive baseline (`B1_adaptive`)

| Aspect | Detail |
|--------|--------|
| Type | Metric-driven adaptive (HPA-style) |
| Detection | 15.0s scrape interval |
| Decision | Target latency 200ms PID ratio scaling + regional traffic shift |
| Governance | Rate limits only — no agent safety layer |
| Role | **Strong deterministic baseline** — modern autoscaler without LLM |

**Last aggregate:** P95 ~257 ms, time above SLA ~14.8 s, total recovery ~22.2 s.

## B2 — Full ESA (`B2_esa`)

| Aspect | Detail |
|--------|--------|
| Type | Governed multi-agent |
| Detection | **250ms** event stream window |
| Decision | Monitor → Diagnosis → Planning → Safety → typed proposal |
| Execution | Policy engine + OCC + Action Gateway + effect measurement + audit |
| Governance | Stale-state reject, approval gates, rollback, SHA-256 chain |
| Role | **Research prototype** — contextual proposals under deterministic control |

**Last aggregate:** P95 ~156 ms, time above SLA ~4.1 s, total recovery ~24.3 s, agent deliberation ~1.8 s.

## Fair comparison

All three controllers run on **identical scenario applicators and seeds** with the same initial `StateFabric` reset between trials.

B2 incurs **additional deliberation latency** but achieves lower tail latency and less time above SLA in the evaluated harness.

## Not compared here

- Production Razorpay traffic
- External commercial autoscaler products
- Human on-call response times

See [README.md](README.md) and [../benchmarkreport.md](../benchmarkreport.md).

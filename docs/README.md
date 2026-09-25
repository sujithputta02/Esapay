# ESA Documentation

Engineering, research, specifications, and demo assets for the ESA (Executable State Architecture) platform.

**Rule:** All claims must match [claims.md](claims.md) and [benchmarkreport.md](benchmarkreport.md).

---

## Demos, Walkthroughs & Scripts

| Document | Description |
|---|---|
| [FINAL_5MIN_DEMO_SCRIPT.md](FINAL_5MIN_DEMO_SCRIPT.md) | Official 5-minute video pitch and narrative script |
| [DEMO_WALKTHROUGH.md](DEMO_WALKTHROUGH.md) | Step-by-step interactive demo execution guide |
| [demo.md](demo.md) | Judge and operator manual for the web Command Center |

---

## Frontier Reasoning & Evaluation

| Document | Description |
|---|---|
| [AGI_REASONER_ARCHITECTURE_AND_CONSTRUCTION.md](AGI_REASONER_ARCHITECTURE_AND_CONSTRUCTION.md) | Construction of the neuro-symbolic fluid reasoner (ARC priors, DSL, MDL) & dual-cadence latency contract |
| [ESA_RBENCH_SPECIFICATION.md](ESA_RBENCH_SPECIFICATION.md) | ESA-RBench 8-stage ladder, 7 baselines + privileged reference policy, formal metrics, literature mapping |

---

## Architecture & Execution

| Document | Description |
|---|---|
| [architecture.md](architecture.md) | Layered system design & boundary of execution |
| [execution-flow.md](execution-flow.md) | End-to-end incident response lifecycle |
| [agent-model.md](agent-model.md) | Agent vs gateway authority (Agents propose, infrastructure executes) |
| [governance.md](governance.md) | Deterministic policy engine, typed IR, and gateway pipeline |
| [state-management.md](state-management.md) | Optimistic Concurrency Control (OCC), snapshots, and stale state rejection |
| [failure-recovery.md](failure-recovery.md) | Autonomous multi-gateway failover and self-healing mechanisms |
| [effect-verification.md](effect-verification.md) | Expected vs observed post-execution metrics verification |
| [audit-replay.md](audit-replay.md) | SHA-256 Merkle chain, deterministic replay, and cryptographic provenance |

---

## Agents

| Document | Description |
|---|---|
| [agents/monitor.md](agents/monitor.md) | Condition detection (latency, queue backlog, corridor errors) |
| [agents/diagnosis.md](agents/diagnosis.md) | Root-cause hypothesis (Ollama local LLM + rule fallback) |
| [agents/planning.md](agents/planning.md) | Typed `ActionProposal` synthesis |
| [agents/safety.md](agents/safety.md) | Advisory risk grading and invariant pre-checks |

---

## Benchmarks & Claims

| Document | Description |
|---|---|
| [benchmarkreport.md](benchmarkreport.md) | Full multi-seed benchmark report |
| [benchmark-results.md](benchmark-results.md) | Benchmark results summary & B0/B1/B2 comparison |
| [claims.md](claims.md) | **Formal Claims Register** & verified boundaries |
| [reproducibility.md](reproducibility.md) | Clone-to-benchmark reproduction guide |
| [../benchmarks/README.md](../benchmarks/README.md) | Benchmark harness overview |
| [../benchmarks/methodology.md](../benchmarks/methodology.md) | Multi-seed measurement methodology |
| [../benchmarks/scenarios.md](../benchmarks/scenarios.md) | BENCH-01 to BENCH-15 scenario definitions |
| [../benchmarks/baselines.md](../benchmarks/baselines.md) | Baseline controller implementations (B0 / B1 / B2) |
| [../benchmarks/ablations.md](../benchmarks/ablations.md) | Causal ablation study variants |

---

## Product Requirements & Specifications (PRD)

| Document | Format | Description |
|---|---|---|
| [ESA_paymentprdv2.md](ESA_paymentprdv2.md) | Markdown | Product requirements document (PRD v2) |
| [PRD_IMPLEMENTATION_CHECKLIST.md](PRD_IMPLEMENTATION_CHECKLIST.md) | Markdown | Implementation status checklist |
| [ESA_Payment_Angle_Detailed_PRD_with_Demo_Plan.docx](ESA_Payment_Angle_Detailed_PRD_with_Demo_Plan.docx) | Word Doc | Comprehensive PRD with detailed demo plan |
| [prd_extracted.txt](prd_extracted.txt) | Text | Extracted plain-text reference PRD |

---

## PDF Specifications & Whitepapers (`docs/pdf/`)

| File | Description |
|---|---|
| [pdf/ESA_Script.pdf](pdf/ESA_Script.pdf) | ESA pitch and demo narrative document |
| [pdf/ESA_Payment_Angle_Detailed_PRD_with_Demo_Plan.pdf](pdf/ESA_Payment_Angle_Detailed_PRD_with_Demo_Plan.pdf) | Formatted PRD specification and demo blueprint |
| [pdf/ESA_paymentprdv2.pdf](pdf/ESA_paymentprdv2.pdf) | Product requirements document (PDF Edition) |
| [pdf/esa_prd_revised.pdf](pdf/esa_prd_revised.pdf) | Revised PRD document with architecture diagrams |
| [pdf/ESA_CONSENSUS_FINAL_GAP_NOVELTY_AUDIT.pdf](pdf/ESA_CONSENSUS_FINAL_GAP_NOVELTY_AUDIT.pdf) | Architectural consensus, novelty, and gap audit |

---

## Repository Governance & Meta

- [../README.md](../README.md) — Main landing page & quick start
- [../CODE_OF_CONDUCT.md](../CODE_OF_CONDUCT.md) — Contributor Covenant v2.1
- [../CONTRIBUTING.md](../CONTRIBUTING.md) — Contribution guidelines
- [../SECURITY.md](../SECURITY.md) — Security and vulnerability disclosure policy
- [../CHANGELOG.md](../CHANGELOG.md) — Version history
- [../LICENSE](../LICENSE) — MIT License

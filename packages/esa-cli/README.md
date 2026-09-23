# esapay-cli ⚡

[![NPM Version](https://img.shields.io/npm/v/esapay-cli?color=1F51FF&label=npm)](https://www.npmjs.com/package/esapay-cli)
[![Bun Compatible](https://img.shields.io/badge/bun-compatible-FBF0DF?logo=bun&logoColor=black)](https://bun.sh)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Node.js](https://img.shields.io/badge/Node.js->=18.0.0-339933?logo=node.js&logoColor=white)](https://nodejs.org/)

Official Command Line Interface & DevTool for **ESA (Executable State Architecture)** — Autonomous Multi-Gateway Payment Resilience, Self-Healing Corridors, and Cryptographic SHA-256 Merkle Ledger.

---

## 📑 Table of Contents

- [⚡ Zero-Install Instant Execution](#-zero-install-instant-execution)
- [📦 Global Installation](#-global-installation)
- [🛠️ Complete Command Reference](#️-complete-command-reference)
  - [`health`: Control Plane Connectivity](#1-health)
  - [`status`: StateFabric Vitals & TPS](#2-status)
  - [`gateways`: Multi-Gateway Telemetry & Chaos Toggle](#3-gateways)
  - [`checkout`: Autonomous Resilient Payment](#4-checkout)
  - [`doctor`: Deep Infrastructure Diagnostics](#5-doctor)
  - [`audit`: Cryptographic Merkle Chain Verification](#6-audit)
  - [`dashboard`: Visual Web Control Center](#7-dashboard)
  - [`chaos`: Synthetic Load Spikes & Rail Degradation](#8-chaos)
  - [`logs`: Real-time Remediations & Decision Stream](#9-logs)
- [💡 Simulated Outage Walkthrough](#-simulated-outage-walkthrough)
- [🤖 Scripting & CI/CD JSON Pipelines](#-scripting--cicd-json-pipelines)
- [⚙️ Configuration & Environment Variables](#️-configuration--environment-variables)
- [🔗 Related Packages](#-related-packages)
- [📄 License](#-license)

---

## ⚡ Zero-Install Instant Execution

Run commands instantly without installing anything via `npx` or `bunx`:

```bash
# 1. Inspect live Indian payment corridors (PhonePe, Razorpay, Paytm, Cashfree)
npx esapay-cli gateways

# 2. Check cluster health & response latency
npx esapay-cli health

# 3. Execute an autonomous resilient checkout (₹500.00 via UPI)
npx esapay-cli checkout --amount 50000 --currency INR --gateway auto

# 4. Verify cryptographic SHA-256 Merkle audit chain
npx esapay-cli audit verify

# 5. Run full environment & Ollama engine diagnostics
npx esapay-cli doctor
```

*(You can also use `bunx esapay-cli <command>`)*

---

## 📦 Global Installation

Install globally to have `esapay`, `esa`, and `esapay-cli` available everywhere in your shell:

### Using NPM
```bash
npm install -g esapay-cli
```

### Using Bun
```bash
bun add -g esapay-cli
```

### Using PNPM / Yarn
```bash
pnpm add -g esapay-cli
# or
yarn global add esapay-cli
```

Once installed globally, you can invoke:
```bash
esapay health
esapay gateways
esapay checkout --amount 50000
```

---

## 🛠️ Complete Command Reference

| Command | Arguments / Flags | Description |
| :--- | :--- | :--- |
| `esapay health` | `[--url <url>] [--json]` | Verify connectivity to the ESA control plane cluster. |
| `esapay status` | `[--url <url>] [--json]` | Real-time StateFabric TPS, P95 latency, error rates, and workload replicas. |
| `esapay gateways` | `[--toggle <gateway>] [--json]` | List monitored corridors, P95 latency, and SLA success rates. Pass `--toggle <name>` to simulate bank rail outages. |
| `esapay checkout` | `--amount <paise> [--gateway auto] [--method UPI]` | Execute an autonomous routing decision with sub-second failover. |
| `esapay doctor` | `[--url <url>]` | Comprehensive system health audit (API, Ollama 24/7 engine, StateFabric, DBs). |
| `esapay audit verify` | `[--url <url>]` | Mathematically verifies cryptographic SHA-256 Merkle chain integrity. |
| `esapay audit trail` | `[--limit <N>]` | Inspect immutable decision ledger blocks. |
| `esapay dashboard` | `[--port 3000]` | Launch local web control console connected to backend. |
| `esapay chaos spike` | — | Trigger synthetic 10x traffic surge drill. |
| `esapay chaos scenario <name>` | — | Trigger complex simulated bank corridor failures. |
| `esapay logs` | `[--limit 50]` | Stream live autonomous remediation events and self-healing logs. |
| `esapay docs` | — | View official documentation and guides. |

---

### 1. `health`
Quick cluster verification:
```bash
esapay health
```
*Output:*
```
✅ ESA Control Plane is HEALTHY at http://localhost:8080
```

---

### 2. `status`
Inspect live cluster vitals and StateFabric workloads:
```bash
esapay status
```
*Output:*
```
================================================================================
  ⚡ ESA (Executable State Architecture) — Live System Vitals
================================================================================
  StateFabric TPS:    184.2
  P95 Latency:        42.1ms
  Error Rate:         0.01%
  Queue Backlog:      0 msgs
  Healthy Workloads:  4 / 4

  Active Workloads:
  ──────────────────────────────────────────────────────────────────────────
  • payment-routing-engine           [Healthy] Replicas: 3/5
  • sovereign-upi-switch             [Healthy] Replicas: 4/6
  • telemetry-collector              [Healthy] Replicas: 2/3
  • audit-merkle-committer           [Healthy] Replicas: 2/2
```

---

### 3. `gateways`
Inspect active Indian corridors or simulate an outage:
```bash
esapay gateways
```
*Output:*
```
================================================================================
  ⚡ ESA Multi-Gateway Telemetry & Corridors
================================================================================
  Gateway     Status     P95 (ms)   Success Rate  Traffic Share
  ─────────────────────────────────────────────────────────────
  phonepe     Healthy    38.4ms     99.9%         35%
  razorpay    Healthy    42.1ms     99.8%         35%
  paytm       Healthy    49.2ms     99.6%         15%
  cashfree    Healthy    52.0ms     99.5%         15%
```

To toggle an outage on Razorpay:
```bash
esapay gateways --toggle razorpay
```
*Output:*
```
⚡ Simulating outage / state change on corridor: razorpay...
✅ Gateway razorpay is now marked as Offline
```

---

### 4. `checkout`
Execute an autonomous resilient checkout:
```bash
esapay checkout --amount 50000 --currency INR --gateway auto --method UPI
```
*Output:*
```
🚀 Initiating Autonomous Checkout: ₹500.00 (50000 paise)...
✅ Checkout Decision Sealed:
  Transaction ID:      tx_live_948f93e2a0b1
  Amount:              ₹500.00 (50000 paise)
  Requested Gateway:   auto
  Routed Gateway:      phonepe
  Failover Triggered:  false
  Routing Rationale:   Optimal UPI latency (38ms) via PhonePe Indian corridor
  Checkout URL:        http://localhost:3000/pay/tx_live_948f93e2a0b1
```

---

### 5. `doctor`
Runs comprehensive diagnostic probes across all system dependencies:
```bash
esapay doctor
```
*Output:*
```
================================================================================
  ⚡ ESA System Doctor Diagnostics
================================================================================
  [PASS] ESA REST API Control Plane (HTTP 200 at http://localhost:8080)
  [PASS] Ollama 24/7 AI Deliberation Engine (llama3 running)
  [PASS] StateFabric In-Memory Event Stream (250ms cadence)
  [PASS] SHA-256 Merkle Ledger Integrity (All blocks verified)
  [PASS] Indian Banking Rail Connectors (UPI, RuPay, NetBanking ready)
```

---

### 6. `audit`
Mathematically verify the tamper-evident SHA-256 cryptographic audit chain:
```bash
esapay audit verify
```
*Output:*
```
🔐 Verifying SHA-256 Cryptographic Audit Chain...
✅ Blockchain Integrity: VERIFIED
  Blocks Validated: 1,482
  Status: All cryptographic Merkle parent hashes match perfectly.
```

---

## 💡 Simulated Outage Walkthrough

Test sub-second failover locally in 3 steps:

1. **Simulate Razorpay Outage**:
   ```bash
   esapay gateways --toggle razorpay
   ```
2. **Execute Checkout**:
   ```bash
   esapay checkout --amount 50000 --gateway auto
   ```
   *Notice `Failover Triggered: true` and traffic instantly routed to `phonepe` UPI corridor in <1.68s.*
3. **Restore Razorpay Corridor**:
   ```bash
   esapay gateways --toggle razorpay
   ```

---

## 🤖 Scripting & CI/CD JSON Pipelines

Use `--json` and pipe to `jq` for automated integration tests and GitHub Actions:

```bash
# Assert cluster health in CI
STATUS=$(esapay health --json | jq -r .status)
if [ "$STATUS" != "healthy" ]; then
  echo "Cluster unhealthy!" && exit 1
fi

# Extract routed gateway from checkout
ROUTED=$(esapay checkout --amount 10000 --json | jq -r .routed_gateway)
echo "Transaction was routed through: $ROUTED"
```

---

## ⚙️ Configuration & Environment Variables

| Variable | Default | Purpose |
| :--- | :--- | :--- |
| `ESA_API_URL` | `http://localhost:8080` | Base URL of the ESA Control Plane cluster |
| `ESA_API_KEY` | *(None)* | Bearer authorization token if cluster authentication is enabled |
| `ESA_DASHBOARD_URL` | `http://localhost:3000` | Target URL for the web control dashboard |

---

## 🔗 Related Packages

- **TypeScript SDK**: [`esapay`](https://www.npmjs.com/package/esapay) — Official Node.js and Bun SDK.
- **Python SDK**: [`esapay`](https://pypi.org/project/esapay) — Official Python SDK.
- **GitHub Repository**: [https://github.com/sujithputta02/Esapay](https://github.com/sujithputta02/Esapay)

---

## 📄 License

MIT © ESA Engineering

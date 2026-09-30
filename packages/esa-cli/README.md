# esapay-cli ⚡

[![NPM Version](https://img.shields.io/npm/v/esapay-cli?color=1F51FF&label=npm)](https://www.npmjs.com/package/esapay-cli)
[![Version](https://img.shields.io/badge/version-v1.0.4-blue.svg)](https://github.com/sujithputta02/Esapay/releases/tag/v1.0.4)
[![Bun Compatible](https://img.shields.io/badge/bun-compatible-FBF0DF?logo=bun&logoColor=black)](https://bun.sh)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Node.js](https://img.shields.io/badge/Node.js->=18.0.0-339933?logo=node.js&logoColor=white)](https://nodejs.org/)

Official Command Line Interface & DevTool for **ESA (Executable State Architecture)** — Autonomous Multi-Gateway Payment Resilience, Self-Healing Corridors, and Cryptographic SHA-256 Merkle Ledger for Sovereign Indian & Global Rails.

---

## 📑 Table of Contents

- [⚡ Zero-Install Instant Execution](#-zero-install-instant-execution)
- [📦 Global Installation](#-global-installation)
- [🔐 Authentication & API Key Management](#-authentication--api-key-management)
  - [Interactive Browser Login (`login`)](#1-interactive-browser-login)
  - [Instant Guest Sandbox (`login --guest`)](#2-instant-guest-sandbox-no-signup-needed)
  - [Direct Token Login (`login --key`)](#3-direct-api-key-login)
  - [Session Identity (`whoami`)](#4-session-identity-whoami)
  - [Logout & Revocation (`logout`)](#5-logout--revocation)
- [🛠️ Complete Command Reference](#️-complete-command-reference)
  - [`health`: Control Plane Connectivity](#1-health)
  - [`status`: StateFabric Vitals & TPS](#2-status)
  - [`gateways`: Multi-Gateway Telemetry & Outage Toggle](#3-gateways)
  - [`checkout`: Autonomous Resilient Payment](#4-checkout)
  - [`doctor`: Deep Infrastructure Diagnostics](#5-doctor)
  - [`audit`: Cryptographic Merkle Chain Verification](#6-audit)
  - [`dashboard`: Visual Web Control Center](#7-dashboard)
  - [`chaos`: Synthetic Load Spikes & Rail Degradation](#8-chaos)
  - [`logs`: Real-time Remediations & Decision Stream](#9-logs)
- [💡 Simulated Outage Walkthrough](#-simulated-outage-walkthrough)
- [🤖 Scripting & CI/CD JSON Pipelines](#-scripting--cicd-json-pipelines)
- [⚙️ Global Options & Configuration](#️-global-options--configuration)
- [🔗 Related Packages](#-related-packages)
- [📄 License](#-license)

---

## ⚡ Zero-Install Instant Execution

Run commands instantly without installing anything via `npx` or `bunx`:

```bash
# 1. Connect instantly in Sandbox test mode (zero setup)
npx esapay-cli login --guest

# 2. Inspect live Indian payment corridors (PhonePe, Razorpay, Paytm, Cashfree)
npx esapay-cli gateways

# 3. Check cluster health & response latency
npx esapay-cli health

# 4. Execute an autonomous resilient checkout (₹500.00 via UPI)
npx esapay-cli checkout --amount 50000 --currency INR --gateway auto

# 5. Verify cryptographic SHA-256 Merkle audit chain
npx esapay-cli audit verify

# 6. Run full environment & AI deliberation diagnostics
npx esapay-cli doctor
```

*(You can also use `bunx esapay-cli <command>`)*

---

## 📦 Global Installation

Install globally to have `esapay`, `esa`, and `esapay-cli` available everywhere in your terminal:

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

Once installed globally, you can invoke any alias (`esapay`, `esa`, or `esapay-cli`):
```bash
esapay health
esapay status
esapay gateways
esapay checkout --amount 50000 --gateway auto
```

---

## 🔐 Authentication & API Key Management

`esapay-cli` features a modern authentication engine (modeled after Claude Code and the Stripe CLI) with support for interactive browser login, instant guest sandbox keys, and direct token authentication.

Credentials are saved securely to `~/.esa/credentials.json` and are **automatically shared with the TypeScript and Python SDKs**!

### 1. Interactive Browser Login
Launches a local listener on `http://localhost:8765/callback` and opens the ESAPay Web Dashboard:
```bash
esapay login
```
*Once authorized in the browser, your credentials are automatically saved.*

### 2. Instant Guest Sandbox (No Signup Needed)
Instantly generate a guest test evaluation key (`esa_test_demo_...`) with pre-funded test corridors:
```bash
esapay login --guest
```
*Output:*
```
✅ Connected in Instant Guest Sandbox Mode!
  • Active API Key:  esa_test_demo_8a7bc12e4f01
  • Environment:     Sandbox (No Signup Needed)
  • Saved to:        ~/.esa/credentials.json
```

### 3. Direct API Key Login
Store your production (`esa_live_...`) or sandbox (`esa_test_...`) key directly:
```bash
esapay login --key esa_live_9f82b71c04a2d8e3
```

### 4. Session Identity (`whoami`)
Inspect your currently active merchant identity, environment, and configuration:
```bash
esapay whoami
```
*Output:*
```
================================================================================
  ⚡ ESAPay CLI — Authenticated Merchant Session
================================================================================
  Merchant User:  merchant@company.com
  Environment:    live (Production)
  Active Key:     esa_live_9f82b... [REDACTED]
  API Endpoint:   https://esapay-api.onrender.com
  Config File:    ~/.esa/credentials.json
```

### 5. Logout & Revocation
Purge locally stored credentials from your machine:
```bash
esapay logout
```

---

## 🛠️ Complete Command Reference

| Command | Arguments / Flags | Description |
| :--- | :--- | :--- |
| `esapay login` | `[--guest \| --key <key>]` | Authenticate with Supabase / Web Portal or generate instant guest test sandbox key. |
| `esapay whoami` | `[--json]` | Display active authenticated merchant session, role, and API key status. |
| `esapay logout` | — | Remove local credentials from `~/.esa/credentials.json`. |
| `esapay health` | `[--url <url>] [--json]` | Verify connectivity to the ESA control plane cluster. |
| `esapay status` | `[--url <url>] [--json]` | Real-time StateFabric TPS, P95 latency, error rates, and workload replicas. |
| `esapay gateways` | `[--toggle <corridor>] [--json]` | Monitored payment corridors (PhonePe, Razorpay, Paytm, Cashfree). Pass `--toggle <name>` to simulate bank rail outages. |
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
✅ ESA Control Plane is HEALTHY at https://esapay-api.onrender.com
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
  StateFabric TPS:    4120.0
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

To toggle a synthetic outage on Razorpay:
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
  Checkout URL:        https://esapay.io/pay/tx_live_948f93e2a0b1
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
  [PASS] ESA REST API Control Plane (HTTP 200 at https://esapay-api.onrender.com)
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
🔐 Verifying SHA-256 Merkle Audit Chain...
✅ Audit Chain Integrity: VALID (All 142 block hashes match root 9e8a...3f01)
```

---

## 💡 Simulated Outage Walkthrough

Test ESA's self-healing circuit breaker in under 60 seconds:

```bash
# Step 1: Check baseline corridor health
esapay gateways

# Step 2: Trigger synthetic degradation on PhonePe
esapay gateways --toggle phonepe

# Step 3: Execute a checkout — watch it autonomously route to Razorpay
esapay checkout --amount 100000 --gateway auto

# Step 4: Verify the decision rationale & cryptographic audit record
esapay audit trail --limit 1

# Step 5: Restore PhonePe corridor
esapay gateways --toggle phonepe
```

---

## 🤖 Scripting & CI/CD JSON Pipelines

Every command accepts `--json` for direct ingestion into `jq`, automated health monitors, or CI/CD pipelines:

```bash
# Extract current StateFabric TPS
npx esapay-cli status --json | jq '.vitals[-1].total_tps'

# Assert audit chain validity in GitHub Actions
npx esapay-cli audit verify --json | jq -e '.valid == true'

# Pipe healthy gateway list into monitoring
npx esapay-cli gateways --json | jq '.[] | select(.status == "Healthy") | .name'
```

---

## ⚙️ Global Options & Configuration

| Flag | Env Variable | Default | Description |
| :--- | :--- | :--- | :--- |
| `-u, --url <URL>` | `ESA_API_URL` | `https://esapay-api.onrender.com` | Target ESA cluster control plane URL. |
| `-k, --key <KEY>` | `ESA_API_KEY` | `~/.esa/credentials.json` | API Key (`esa_live_...` or `esa_test_...`). |
| `--json` | — | `false` | Return structured JSON output for automation. |
| `-V, --version` | — | — | Print CLI version (`1.0.4`). |
| `-h, --help` | — | — | Display comprehensive help menu. |

---

## 🔗 Related Packages

- [**`esapay` (TypeScript / Node / Bun SDK)**](https://www.npmjs.com/package/esapay) — Official SDK with strict TypeScript types, automatic credential inheritance, and sub-second checkout failover.
- [**`esapay` (Python SDK on PyPI)**](https://pypi.org/project/esapay/) — Zero-dependency Python 3.8+ package for FastAPI, Django, Flask, and AWS Lambda.
- [**GitHub Repository**](https://github.com/sujithputta02/Esapay) — Source code, benchmark specifications, and local development instructions.

---

## 📄 License

MIT © [Sujith Putta](https://github.com/sujithputta02)

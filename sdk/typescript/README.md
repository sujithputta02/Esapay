# esapay ⚡

[![NPM Version](https://img.shields.io/npm/v/esapay?color=1F51FF&label=npm)](https://www.npmjs.com/package/esapay)
[![Bun Compatible](https://img.shields.io/badge/bun-compatible-FBF0DF?logo=bun&logoColor=black)](https://bun.sh)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![TypeScript: Strict](https://img.shields.io/badge/TypeScript-Strict%20Ready-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Node.js](https://img.shields.io/badge/Node.js->=18.0.0-339933?logo=node.js&logoColor=white)](https://nodejs.org/)

The official TypeScript, Node.js, and Bun SDK for **ESA (Executable State Architecture)** — Autonomous Multi-Gateway Payment Resilience and Self-Healing Financial Infrastructure for Sovereign Indian Rails.

---

## 📑 Table of Contents

- [Overview](#-overview)
- [Why ESA?](#-why-esa)
- [Installation](#-installation)
- [Quickstart (TypeScript & Bun)](#-quickstart-typescript--bun)
- [Sovereign Indian Rails Support](#-sovereign-indian-rails-support)
- [Complete API Reference](#-complete-api-reference)
  - [Initialization (`new EsaGateway`)](#1-initialization)
  - [Universal Checkout (`esa.checkout`)](#2-universal-checkout)
  - [Gateway Telemetry & Health (`esa.gateways`)](#3-gateway-telemetry--health)
  - [Cryptographic Audit Ledger (`esa.audit`)](#4-cryptographic-audit-ledger)
  - [Live Telemetry Stream (`esa.vitals`)](#5-live-telemetry-stream)
  - [Chaos Engineering (`esa.chaos`)](#6-chaos-engineering)
- [Production Framework Recipes](#-production-framework-recipes)
  - [Next.js App Router (Route Handler)](#nextjs-app-router-route-handler)
  - [Express / Fastify Backend](#express--fastify-backend)
  - [Bun Native HTTP Server](#bun-native-http-server)
- [Webhook Verification & Security](#-webhook-verification--security)
- [Error Handling](#-error-handling)
- [TypeScript Definitions](#-typescript-definitions)
- [Related Packages](#-related-packages)
- [License](#-license)

---

## ⚡ Overview

In modern fintech, upstream payment gateway downtime, bank corridor degradation, and latency spikes cost businesses millions in abandoned checkouts. **ESA** turns payment infrastructure into an **autonomous, self-healing system**.

With `esapay`, your application executes intelligent payment routing across top Indian payment aggregators (**PhonePe**, **Razorpay**, **Paytm**, and **Cashfree**). If any gateway or bank rail suffers high latency (>150ms) or elevated error rates, ESA detects the failure in **250ms** and routes subsequent traffic to healthy corridors in **<1.68s** with **zero lost transactions**.

```
                           ┌──────────────────────────┐
                           │   Your Application       │
                           │   (Node / Bun / Next.js) │
                           └─────────────┬────────────┘
                                         │ esa.checkout({ amount: 50000, method: 'UPI' })
                                         ▼
                     ┌───────────────────────────────────────┐
                     │         ESA Self-Healing Engine       │
                     │  • 250ms Real-Time Event Stream       │
                     │  • Deterministic Safety Guardrails    │
                     │  • Sub-Second Failover Circuit Breaker│
                     │  • SHA-256 Tamper-Proof Audit Chain   │
                     └───────────────────┬───────────────────┘
                                         │
               ┌────────────────┬────────┴───────┬────────────────┐
               ▼                ▼                ▼                ▼
        ┌──────────────┐ ┌──────────────┐ ┌──────────────┐ ┌──────────────┐
        │   PhonePe    │ │   Razorpay   │ │    Paytm     │ │   Cashfree   │
        │  (UPI / QR)  │ │ (RuPay / CC) │ │ (NetBanking) │ │  (UPI / QR)  │
        └──────────────┘ └──────────────┘ └──────────────┘ └──────────────┘
```

---

## 🚀 Why ESA?

1. **Sub-Second Autonomous Failover**: Continuous telemetry evaluation routes transactions away from degraded rails before customers notice.
2. **Sovereign Indian Rails First**: Native optimizations for UPI Intent, Dynamic UPI QR, RuPay Zero-MDR, and direct NetBanking across SBI, HDFC, ICICI, and Axis Bank.
3. **Deterministic Safety Gate**: LLM-driven optimization agents propose topology changes, but zero unverified or hallucinated actions mutate financial state.
4. **Immutable SHA-256 Merkle Ledger**: Every routing decision, latency snapshot, and failover trigger is cryptographically sealed into an append-only verifiable chain.
5. **Zero Vendor Lock-In**: Unified API abstraction across PhonePe, Razorpay, Paytm, and Cashfree.

---

## 📦 Installation

Install `esapay` using your preferred runtime or package manager:

### Using NPM
```bash
npm install esapay
```

### Using Bun
```bash
bun add esapay
```

### Using PNPM
```bash
pnpm add esapay
```

### Using Yarn
```bash
yarn add esapay
```

---

## ⚡ Quickstart (TypeScript & Bun)

```typescript
import { EsaGateway } from 'esapay';

// 1. Initialize client pointed at your ESA cluster
const esa = new EsaGateway({
  apiUrl: process.env.ESA_API_URL || 'http://localhost:8080',
  apiKey: process.env.ESA_API_KEY,
  timeoutMs: 8000,
});

async function main() {
  // 2. Health check
  const health = await esa.health();
  console.log('ESA Cluster Status:', health.status);

  // 3. Inspect active Indian payment corridors & live latency
  const corridors = await esa.gateways.list();
  console.table(corridors.map(c => ({
    Gateway: c.name,
    Status: c.status,
    P95: `${c.p95_latency_ms.toFixed(1)}ms`,
    SuccessRate: `${(c.success_rate * 100).toFixed(1)}%`,
    TrafficShare: `${c.active_traffic_pct}%`,
  })));

  // 4. Execute an autonomous resilient checkout (₹500.00 via UPI)
  const order = await esa.checkout({
    amount: 50000,       // In paise (₹500.00)
    currency: 'INR',
    gateway: 'auto',     // Autonomous dynamic routing
    method: 'UPI',       // UPI | CARD | NETBANKING
  });

  console.log('✅ Transaction Created:');
  console.log('  Transaction ID:', order.transaction_id);
  console.log('  Routed Gateway:', order.routed_gateway);
  console.log('  Failover Triggered:', order.failover_triggered);
  console.log('  Routing Reason:', order.routing_reason);
  console.log('  Checkout URL:', order.checkout_url);

  // 5. Mathematically verify the SHA-256 cryptographic audit ledger
  const audit = await esa.audit.verifyChain();
  console.log(`🔐 Audit Chain Integrity: ${audit.valid ? 'VERIFIED' : 'TAMPERED'}`);
}

main().catch(console.error);
```

---

## 🇮🇳 Sovereign Indian Rails Support

| Rail / Method | Supported Corridors | Features & Optimization |
| :--- | :--- | :--- |
| **UPI Intent & Collect** | PhonePe, Razorpay, Paytm, Cashfree | Direct deep-linking to BHIM, Google Pay, PhonePe, Paytm apps. Sub-200ms intent generation. |
| **Dynamic UPI QR** | PhonePe, Paytm, Cashfree | NPCI-compliant dynamic QR payloads with instant server-sent push confirmation. |
| **RuPay Zero-MDR Cards** | Razorpay, Cashfree, Paytm | Priority routing for RuPay credit and debit cards to maximize merchant cost savings. |
| **NetBanking (T1 Banks)** | Razorpay, Paytm, Cashfree | Automatic fallback between SBI, HDFC, ICICI, and Axis Bank direct gateways. |

---

## 🛠️ Complete API Reference

### 1. Initialization

```typescript
import { EsaGateway, ESAClient } from 'esapay';

const esa = new EsaGateway({
  apiUrl: 'http://localhost:8080', // ESA Control Plane URL (defaults to http://localhost:8080)
  wsUrl: 'ws://localhost:8080',    // Telemetry WebSocket URL (auto-inferred if omitted)
  apiKey: 'secret_live_...',       // Optional Bearer token for protected clusters
  timeoutMs: 10000,                // HTTP request timeout in milliseconds (default: 10000)
});
```

*(Note: `ESAClient` is an exact alias of `EsaGateway` for backwards compatibility).*

---

### 2. Universal Checkout

#### `esa.checkout(params)` / `esa.payments.checkout(params)`
Initiates an autonomous payment session. If the requested gateway is degraded or in `'auto'` mode, the router dynamically selects the optimal Indian corridor.

```typescript
const decision = await esa.checkout({
  amount: 149900,                    // Required: Amount in paise (149900 = ₹1,499.00)
  currency: 'INR',                   // Optional: Defaults to 'INR'
  gateway: 'auto',                   // 'auto' | 'phonepe' | 'razorpay' | 'paytm' | 'cashfree'
  method: 'UPI',                     // 'UPI' | 'CARD' | 'NETBANKING'
});
```

**Parameters:**
- `amount` *(number, required)*: Transaction amount in smallest currency unit (paise for INR).
- `currency` *(string, optional)*: Currency code. Defaults to `'INR'`.
- `gateway` *(string, optional)*: Target gateway name or `'auto'` for autonomous multi-gateway routing.
- `method` *(string, optional)*: Payment method: `'UPI'`, `'CARD'`, `'NETBANKING'`.

**Returns `Promise<CheckoutDecision>`:**
```typescript
interface CheckoutDecision {
  transaction_id: string;        // Unique ESA transaction identifier
  amount: number;                // Amount in paise
  currency: string;              // 'INR'
  requested_gateway: string;     // 'auto' or gateway requested
  routed_gateway: string;        // Actual gateway selected (e.g. 'phonepe')
  failover_triggered: boolean;   // true if an outage/latency caused failover
  routing_reason: string;        // Human/AI rationale for routing selection
  checkout_url: string;          // Customer-facing checkout or intent URL
  timestamp?: string;            // ISO-8601 timestamp
}
```

---

### 3. Gateway Telemetry & Health

#### `esa.gateways.list()`
Fetches real-time telemetry, P95 latencies, success rates, and traffic allocation across all monitored gateways.

```typescript
const corridors = await esa.gateways.list();
```

**Returns `Promise<GatewayHealth[]>`:**
```typescript
interface GatewayHealth {
  gateway: string;            // Gateway slug ('phonepe', 'razorpay', 'paytm', 'cashfree')
  name: string;               // Display name ('PhonePe UPI Gateway')
  status: string;             // 'Healthy' | 'Degraded' | 'Offline'
  p95_latency_ms: number;     // P95 round-trip latency in ms
  success_rate: number;       // Ratio from 0.0 to 1.0 (e.g. 0.998)
  active_traffic_pct: number; // Current traffic allocation (0 to 100)
  is_healthy: boolean;        // Boolean status flag
}
```

#### `esa.gateways.toggle(gatewayName)`
Simulates a bank rail outage or restores a gateway for chaos and failover testing.

```typescript
// Toggle Razorpay offline to test automated PhonePe failover
const result = await esa.gateways.toggle('razorpay');
console.log(result.message); // "Gateway razorpay is now marked as Offline"
```

---

### 4. Cryptographic Audit Ledger

#### `esa.audit.verifyChain()`
Mathematically validates the cryptographic SHA-256 Merkle chain of all payment routing events and topology shifts.

```typescript
const verification = await esa.audit.verifyChain();
console.log(verification);
// { valid: true, block_count: 1420, message: "Chain mathematically verified" }
```

#### `esa.audit.getTrail(limit?)`
Retrieves recent immutable ledger blocks.

```typescript
const blocks = await esa.audit.getTrail(25);
for (const b of blocks) {
  console.log(`Block #${b.index}: ${b.action_type} | Hash: ${b.current_hash.slice(0, 16)}...`);
}
```

---

### 5. Live Telemetry Stream

#### `esa.vitals.subscribe(callback)`
Establishes a real-time WebSocket connection to the ESA telemetry pipeline to receive live cluster vitals every 250ms.

```typescript
const unsubscribe = esa.vitals.subscribe((vitals) => {
  console.log(`TPS: ${vitals.total_tps} | P95: ${vitals.avg_p95_ms}ms | Errors: ${vitals.avg_error_rate}`);
});

// To disconnect later:
// unsubscribe();
```

---

### 6. Chaos Engineering

#### `esa.chaos.triggerSpike()`
Simulates an unexpected 10x transaction traffic spike on Indian UPI rails to evaluate dynamic scaling.

```typescript
await esa.chaos.triggerSpike();
```

#### `esa.chaos.triggerScenario(scenarioName)`
Executes synthetic disaster scenarios such as bank NPCI switch timeouts.

```typescript
await esa.chaos.triggerScenario('npci_switch_degradation');
```

---

## 🌐 Production Framework Recipes

### Next.js App Router (Route Handler)

```typescript
// app/api/checkout/route.ts
import { NextResponse } from 'next/server';
import { EsaGateway } from 'esapay';

const esa = new EsaGateway({
  apiUrl: process.env.ESA_API_URL || 'http://localhost:8080',
  apiKey: process.env.ESA_API_KEY,
});

export async function POST(req: Request) {
  try {
    const { amount, method } = await req.json();

    const decision = await esa.checkout({
      amount,
      currency: 'INR',
      gateway: 'auto',
      method: method || 'UPI',
    });

    return NextResponse.json({
      success: true,
      transactionId: decision.transaction_id,
      routedGateway: decision.routed_gateway,
      checkoutUrl: decision.checkout_url,
      failoverActive: decision.failover_triggered,
    });
  } catch (error: any) {
    return NextResponse.json({ success: false, error: error.message }, { status: 500 });
  }
}
```

---

### Express / Fastify Backend

```typescript
import express from 'express';
import { EsaGateway } from 'esapay';

const app = express();
app.use(express.json());

const esa = new EsaGateway({
  apiUrl: process.env.ESA_API_URL || 'http://localhost:8080',
});

app.post('/api/pay', async (req, res) => {
  try {
    const decision = await esa.checkout({
      amount: req.body.amount, // in paise
      currency: 'INR',
      gateway: 'auto',
      method: 'UPI',
    });

    res.json(decision);
  } catch (err: any) {
    res.status(502).json({ error: 'Payment routing failed', details: err.message });
  }
});

app.listen(4000, () => console.log('Payment service listening on :4000'));
```

---

### Bun Native HTTP Server

```typescript
import { EsaGateway } from 'esapay';

const esa = new EsaGateway({ apiUrl: 'http://localhost:8080' });

Bun.serve({
  port: 3001,
  async fetch(req) {
    const url = new URL(req.url);

    if (url.pathname === '/checkout' && req.method === 'POST') {
      const body = await req.json();
      const decision = await esa.checkout({
        amount: body.amount,
        currency: 'INR',
        gateway: 'auto',
        method: 'UPI',
      });
      return Response.json(decision);
    }

    if (url.pathname === '/health') {
      return Response.json(await esa.health());
    }

    return new Response('Not Found', { status: 404 });
  },
});
```

---

## 🔒 Webhook Verification & Security

ESA signs all routing events, payment notifications, and failover webhooks using standard HMAC SHA-256 signatures with timestamp anti-replay verification:

```typescript
import crypto from 'node:crypto';

export function verifyEsaWebhook(
  rawBody: string,
  signatureHeader: string,
  webhookSecret: string
): boolean {
  const [timestampPart, signaturePart] = signatureHeader.split(',');
  const timestamp = timestampPart.replace('t=', '');
  const signature = signaturePart.replace('v1=', '');

  // Protect against replay attacks (5 minute threshold)
  const age = Math.abs(Date.now() - parseInt(timestamp, 10));
  if (age > 5 * 60 * 1000) return false;

  const payloadToSign = `${timestamp}.${rawBody}`;
  const computed = crypto
    .createHmac('sha256', webhookSecret)
    .update(payloadToSign)
    .digest('hex');

  return crypto.timingSafeEqual(Buffer.from(signature), Buffer.from(computed));
}
```

---

## 🛡️ Error Handling

The SDK throws descriptive errors with status codes and upstream gateway diagnostic context:

```typescript
import { EsaGateway } from 'esapay';

const esa = new EsaGateway({ apiUrl: 'http://localhost:8080' });

try {
  await esa.checkout({ amount: 100, currency: 'INR' });
} catch (error: any) {
  console.error('Failed to route payment:', error.message);
  // Example: ESA API Error [503]: All upstream Indian gateways degraded and circuit breakers tripped
}
```

---

## 📦 Related Packages

- **CLI Tool**: [`esapay-cli`](https://www.npmjs.com/package/esapay-cli) — Terminal dashboard, health checks, and chaos failover testing (`npx esapay-cli`).
- **Python SDK**: [`esapay`](https://pypi.org/project/esapay) — Official Python client with standard library zero-dependency engine.
- **GitHub Repository**: [https://github.com/sujithputta02/Esapay](https://github.com/sujithputta02/Esapay)

---

## 📄 License

MIT © ESA Engineering

# esapay ⚡

[![PyPI Version](https://img.shields.io/pypi/v/esapay?color=1F51FF&label=pypi)](https://pypi.org/project/esapay/)
[![Python Version](https://img.shields.io/badge/python-3.8+-blue.svg?logo=python&logoColor=white)](https://www.python.org/)
[![Dependencies](https://img.shields.io/badge/dependencies-zero%20(stdlib%20only)-success.svg)](https://docs.python.org/3/library/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)

Official Python SDK for **ESA (Executable State Architecture)** — Autonomous Multi-Gateway Payment Resilience and Self-Healing Financial Infrastructure for Sovereign Indian Rails.

---

## 📑 Table of Contents

- [Overview](#-overview)
- [Why ESA for Python?](#-why-esa-for-python)
- [Installation](#-installation)
- [Quickstart](#-quickstart)
- [Sovereign Indian Rails Support](#-sovereign-indian-rails-support)
- [Complete API Reference](#-complete-api-reference)
  - [Initialization (`EsaGateway`)](#1-initialization)
  - [Autonomous Checkout (`esa.checkout`)](#2-autonomous-checkout)
  - [Gateway Telemetry & Corridors (`esa.gateways`)](#3-gateway-telemetry--corridors)
  - [Cryptographic Merkle Audit Ledger (`esa.audit`)](#4-cryptographic-merkle-audit-ledger)
  - [StateFabric Workload Entities (`esa.workloads`)](#5-statefabric-workload-entities)
- [Production Web Framework Recipes](#-production-web-framework-recipes)
  - [FastAPI Route Handler](#fastapi-route-handler)
  - [Flask Microservice](#flask-microservice)
  - [Django View Handler](#django-view-handler)
- [Webhook Verification & HMAC Security](#-webhook-verification--hmac-security)
- [Error Handling](#-error-handling)
- [Type Definitions & Dataclasses](#-type-definitions--dataclasses)
- [Related Packages](#-related-packages)
- [License](#-license)

---

## ⚡ Overview

In fintech and high-volume e-commerce, unexpected bank corridor degradation, UPI switch timeouts, and payment gateway downtime cause severe transaction abandonment. **ESA** turns payment infrastructure into an **autonomous, self-healing system**.

With `esapay`, your Python services execute intelligent routing across top Indian payment aggregators (**PhonePe**, **Razorpay**, **Paytm**, and **Cashfree**). If any gateway or bank rail suffers high latency (>150ms) or elevated error rates, ESA detects the failure in **250ms** and routes subsequent traffic to healthy corridors in **<1.68s** with **zero dropped transactions**.

```
                           ┌──────────────────────────┐
                           │      Python Backend      │
                           │ (FastAPI / Flask / Django)│
                           └─────────────┬────────────┘
                                         │ esa.checkout(amount=50000, method="UPI")
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

## 🚀 Why ESA for Python?

1. **Zero External Dependencies**: Engineered strictly on Python's native standard library (`urllib`, `dataclasses`, `json`). Zero binary compilation, zero security supply-chain bloat. Instant execution in **AWS Lambda**, **Google Cloud Functions**, **Cloud Run**, and serverless environments.
2. **Sub-Second Autonomous Failover**: Sub-second circuit breaking continuously protects your business from upstream gateway downtime.
3. **Sovereign Indian Rails Optimization**: Native handling for UPI Intent, Dynamic UPI QR, RuPay Zero-MDR, and direct NetBanking fallback (SBI, HDFC, ICICI, Axis).
4. **Cryptographic SHA-256 Audit Proofs**: Every routing decision, failover reason, and corridor switch is recorded in an immutable Merkle ledger verified directly from Python.

---

## 📦 Installation

Install `esapay` using pip, uv, or poetry:

### Using pip
```bash
pip install esapay
```

### Using uv
```bash
uv add esapay
```

### Using Poetry
```bash
poetry add esapay
```

*(Works on Python 3.8, 3.9, 3.10, 3.11, 3.12, 3.13 and PyPy)*

---

## ⚡ Quickstart

```python
from esapay import EsaGateway

# 1. Initialize client pointed at your ESA cluster
esa = EsaGateway(
    api_url="http://localhost:8080",
    # api_key="secret_live_..."  # Optional bearer token
    timeout=10.0,
)

def main():
    # 2. Check cluster connectivity
    health = esa.health()
    print(f"ESA Cluster Status: {health.get('status', 'healthy')}")

    # 3. Inspect active Indian payment corridors & P95 latency
    corridors = esa.gateways.list()
    print("\nLive Payment Corridors:")
    for c in corridors:
        print(f"  • {c.name:25} [{c.status:8}] P95: {c.p95_latency_ms:.1f}ms | Success: {c.success_rate * 100:.1f}%")

    # 4. Execute an autonomous resilient checkout (₹500.00 via UPI)
    decision = esa.checkout(
        amount=50000,       # In paise (₹500.00)
        currency="INR",
        gateway="auto",     # Autonomous dynamic routing
        method="UPI",       # UPI | CARD | NETBANKING
    )

    print("\n✅ Autonomous Checkout Decision Sealed:")
    print(f"  Transaction ID:      {decision.transaction_id}")
    print(f"  Routed Corridor:     {decision.routed_gateway}")
    print(f"  Failover Triggered:  {decision.failover_triggered}")
    print(f"  Routing Rationale:   {decision.routing_reason}")
    print(f"  Checkout Session:    {decision.checkout_url}")

    # 5. Mathematically verify the SHA-256 cryptographic audit chain
    audit = esa.audit.verify_chain()
    print(f"\n🔐 Audit Chain Verified: {audit.get('valid', True)}")

if __name__ == "__main__":
    main()
```

---

## 🇮🇳 Sovereign Indian Rails Support

| Rail / Method | Supported Corridors | Optimization Highlights |
| :--- | :--- | :--- |
| **UPI Intent & Dynamic QR** | PhonePe, Razorpay, Paytm, Cashfree | Instant deep-link intent generation for Google Pay, PhonePe, Paytm, and BHIM apps. Sub-200ms latency. |
| **RuPay Zero-MDR Cards** | Razorpay, Cashfree, Paytm | Priority routing for RuPay credit and debit cards to maximize merchant cost savings under NPCI guidelines. |
| **NetBanking (Tier-1 Banks)** | Razorpay, Paytm, Cashfree | Direct core-banking switch fallback across SBI, HDFC, ICICI, and Axis Bank. |

---

## 🛠️ Complete API Reference

### 1. Initialization

```python
from esapay import EsaGateway

esa = EsaGateway(
    api_url="http://localhost:8080",      # ESA cluster endpoint
    api_key=None,                          # Optional Bearer token
    timeout=10.0,                          # Request timeout in seconds
    auto_failover=True,                    # Enable dynamic routing
    corridors=["phonepe", "razorpay", "paytm", "cashfree"]
)
```

*(Note: `from esapay import ESAClient` is also provided as an exact alias).*

---

### 2. Autonomous Checkout

#### `esa.checkout(amount, currency="INR", gateway="auto", method="UPI")`
Executes an intelligent checkout decision. If a specific gateway is specified but degraded, or if `'auto'` is selected, ESA autonomously routes to the fastest healthy Indian corridor.

```python
decision = esa.checkout(
    amount=149900,         # Required: In paise (149900 = ₹1,499.00)
    currency="INR",        # Optional: Defaults to "INR"
    gateway="auto",        # "auto" | "phonepe" | "razorpay" | "paytm" | "cashfree"
    method="UPI",          # "UPI" | "CARD" | "NETBANKING"
)
```

**Returns `CheckoutDecision`:**
- `transaction_id` *(str)*: Unique ESA transaction identifier.
- `amount` *(int)*: Amount in paise.
- `currency` *(str)*: Currency code (e.g. `"INR"`).
- `requested_gateway` *(str)*: Gateway requested by caller.
- `routed_gateway` *(str)*: Final gateway selected by the resilient router.
- `failover_triggered` *(bool)*: `True` if failover was triggered due to corridor degradation.
- `routing_reason` *(str)*: Human/AI explanation of the routing rationale.
- `checkout_url` *(str)*: Customer payment session URL.
- `timestamp` *(Optional[str])*: ISO-8601 timestamp.

---

### 3. Gateway Telemetry & Corridors

#### `esa.gateways.list() -> List[GatewayHealth]`
Retrieves live SLA health, P95 latencies, success rates, and traffic allocation across all monitored gateways.

```python
corridors = esa.gateways.list()
for gw in corridors:
    print(f"{gw.name}: {gw.status}, P95={gw.p95_latency_ms}ms, healthy={gw.is_healthy}")
```

#### `esa.gateways.toggle(gateway: str) -> dict`
Simulates a bank rail outage or restores a gateway for chaos and failover drills.

```python
# Toggle Razorpay offline to test automated PhonePe failover
result = esa.gateways.toggle("razorpay")
print(result) # {'gateway': 'razorpay', 'is_healthy': False, 'message': '...'}
```

---

### 4. Cryptographic Merkle Audit Ledger

#### `esa.audit.verify_chain() -> dict`
Mathematically validates the cryptographic SHA-256 Merkle chain of all payment routing events and topology shifts.

```python
result = esa.audit.verify_chain()
print(f"Audit Status: {result.get('valid')}")
```

#### `esa.audit.get_blocks() -> List[AuditBlock]`
Retrieves recent immutable ledger blocks.

```python
blocks = esa.audit.get_blocks()
for block in blocks:
    print(f"Block #{block.index}: {block.action_type} | Hash: {block.current_hash[:16]}...")
```

---

### 5. StateFabric Workload Entities

#### `esa.workloads.list() -> List[WorkloadEntity]`
Inspect registered workloads and execution replicas in the Executable StateFabric.

```python
workloads = esa.workloads.list()
for w in workloads:
    print(f"{w.name} ({w.id}): Replicas={w.replicas}, P95={w.p95_latency_ms}ms")
```

---

## 🌐 Production Web Framework Recipes

### FastAPI Route Handler

```python
# main.py
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from esapay import EsaGateway, EsaClientError

app = FastAPI(title="Fintech Payment Service")
esa = EsaGateway(api_url="http://localhost:8080")

class CheckoutRequest(BaseModel):
    amount: int  # in paise
    method: str = "UPI"
    currency: str = "INR"

@app.post("/api/checkout")
async def create_checkout(req: CheckoutRequest):
    try:
        decision = esa.checkout(
            amount=req.amount,
            currency=req.currency,
            gateway="auto",
            method=req.method,
        )
        return {
            "success": True,
            "transaction_id": decision.transaction_id,
            "routed_gateway": decision.routed_gateway,
            "checkout_url": decision.checkout_url,
            "failover_active": decision.failover_triggered,
        }
    except EsaClientError as e:
        raise HTTPException(status_code=502, detail=str(e))
```

---

### Flask Microservice

```python
from flask import Flask, request, jsonify
from esapay import EsaGateway, EsaClientError

app = Flask(__name__)
esa = EsaGateway(api_url="http://localhost:8080")

@app.route("/pay", methods=["POST"])
def pay():
    data = request.get_json() or {}
    amount = data.get("amount", 50000)

    try:
        decision = esa.checkout(
            amount=amount,
            currency="INR",
            gateway="auto",
            method=data.get("method", "UPI")
        )
        return jsonify({
            "transaction_id": decision.transaction_id,
            "routed_gateway": decision.routed_gateway,
            "checkout_url": decision.checkout_url
        })
    except EsaClientError as err:
        return jsonify({"error": str(err)}), 502

if __name__ == "__main__":
    app.run(port=5000)
```

---

### Django View Handler

```python
# views.py
import json
from django.http import JsonResponse
from django.views import View
from django.conf import settings
from esapay import EsaGateway

esa = EsaGateway(api_url=getattr(settings, "ESA_API_URL", "http://localhost:8080"))

class CheckoutView(View):
    def post(self, request, *args, **kwargs):
        payload = json.loads(request.body)
        decision = esa.checkout(
            amount=payload.get("amount"),
            currency="INR",
            gateway="auto",
            method=payload.get("method", "UPI"),
        )
        return JsonResponse({
            "transaction_id": decision.transaction_id,
            "routed_gateway": decision.routed_gateway,
            "checkout_url": decision.checkout_url,
        })
```

---

## 🔒 Webhook Verification & HMAC Security

Verify ESA webhook signatures using Python's native `hmac` and `hashlib` modules:

```python
import hmac
import hashlib
import time

def verify_esa_webhook(raw_payload: bytes, signature_header: str, secret: str) -> bool:
    """
    Verifies the HMAC SHA-256 signature of an incoming ESA webhook notification.
    Format: 't=1690000000,v1=abcdef...'
    """
    parts = dict(item.split("=") for item in signature_header.split(","))
    timestamp = parts.get("t")
    signature = parts.get("v1")

    if not timestamp or not signature:
        return False

    # Prevent replay attacks (>5 minute clock skew)
    if abs(time.time() - int(timestamp)) > 300:
        return False

    signed_payload = f"{timestamp}.".encode("utf-8") + raw_payload
    expected = hmac.new(
        secret.encode("utf-8"),
        signed_payload,
        hashlib.sha256
    ).hexdigest()

    return hmac.compare_digest(expected, signature)
```

---

## 🛡️ Error Handling

The client raises `EsaClientError` with HTTP status codes and upstream diagnostic context:

```python
from esapay import EsaGateway, EsaClientError

esa = EsaGateway(api_url="http://localhost:8080")

try:
    esa.checkout(amount=1000)
except EsaClientError as e:
    print(f"Payment routing failed: {e}")
    if e.status_code == 503:
        print("All upstream gateways currently degraded.")
```

---

## 📦 Related Packages

- **TypeScript SDK**: [`esapay`](https://www.npmjs.com/package/esapay) — Official Node.js and Bun SDK.
- **CLI Tool**: [`esapay-cli`](https://www.npmjs.com/package/esapay-cli) — Terminal dashboard & diagnostics (`npx esapay-cli`).
- **GitHub Repository**: [https://github.com/sujithputta02/Esapay](https://github.com/sujithputta02/Esapay)

---

## 📄 License

MIT © ESA Engineering

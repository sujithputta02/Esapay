# ESA (Executable State Architecture) — Ultimate 5-Minute Video Pitch & Demo Script
**Speaker:** Sujith Putta  
**Event:** Razorpay AI Buildathon  
**Track:** Track 05 — Open Track: *"Build what you believe should exist"*  
**5-Minute Demo Video:** [https://youtu.be/77qjP2yK7Og](https://youtu.be/77qjP2yK7Og)  
**Target Duration:** Exactly 5:00 Minutes (No Q&A — 100% Focused Live Pitch & Demo)  

> 💡 **Reading Tip for Sujith**: All exact numbers, percentages, and metrics are highlighted in prominent bracket boxes like **`[ ₹30,46,625.80 INR ]`** and **`[ 39.2% ]`** so you can read them with 100% confidence!

---

## 🧭 Master 5-Minute Screen & Speech Roadmap

```text
┌─────────────────┬─────────────────┬─────────────────┬─────────────────┬─────────────────┐
│ MINUTE 1 (0:00) │ MINUTE 2 (1:00) │ MINUTE 3 (2:15) │ MINUTE 4 (3:30) │ MINUTE 5 (4:20) │
├─────────────────┼─────────────────┼─────────────────┼─────────────────┼─────────────────┤
│ 🖥️ SCREEN 1     │ 🖥️ SCREEN 2     │ 🖥️ SCREEN 3     │ 🖥️ SCREEN 4     │ 🖥️ SCREEN 5     │
│ Payment UI      │ Command Center  │ Agents View     │ Dashboard RCA & │ Benchmarks View │
│ Simulator       │ Dashboard Spike │ Reasoning Cards │ Audit Ledger    │ 155-Run Evidence│
│ (Port 5173)     │ (Port 3000)     │ (:3000/agents)  │ (:3000/dashboard│ (:3000/benchmark│
└─────────────────┴─────────────────┴─────────────────┴─────────────────┴─────────────────┘
```

---

## 🎬 Word-for-Word Spoken Script with Metric Bracket Boxes

---

### ⏱️ MINUTE 1 (00:00 – 01:00): Introduction, UI Metric Boxes & Authentic Checkout

#### 📍 Screen: Payment Simulator (`http://localhost:5173`)
*(Mouse: Point to the Top Wallet Balance, Donut Chart, and click on the 3D Cards)*

#### 🗣️ Read This Aloud:
> *"Hello Razorpay AI Buildathon! I'm **Sujith Putta**. For **Track 05 — Open Track**, I built what I believe fundamentally needs to exist in mission-critical financial systems: **ESA (Executable State Architecture)** — an autonomous, self-healing runtime for payment infrastructure.*
>
> *Every year during Diwali flash sales or IPL finals, payment gateways face sudden regional surges, bank downtime, and queue buildup. Traditional cloud autoscalers take **`[ 3 to 5 Minutes ]`** to react, resulting in thousands of dropped checkouts and crores in lost merchant GMV.*
>
> *We asked: **How can we let AI reason about payment infrastructure without giving AI unrestricted authority to break production?***
>
> *Look at our Payment Simulator here on the left: at the top is our **Merchant Settlement Wallet** tracking **`[ ₹30,46,625.80 INR ]`**, our **Gross Captured Income** of **`[ +₹1,28,568 INR ]`** with a **`[ +15.7% ]`** weekly growth, and a dynamic **Payment Rails Split Donut** processing **`[ 4,033 Live TPS ]`** across UPI **`[ 45% ]`**, Cards **`[ 30% ]`**, and NetBanking **`[ 15% ]`**.*
>
> *Below, we have stacked authentic **Razorpay Domestic Indian Test Cards** — HDFC Visa Debit **`[ 4100 2800 0000 1007 ]`**, SBI RuPay Platinum **`[ 6527 6589 0000 1005 ]`**, and ICICI Mastercard Business **`[ 5555 5100 0008 1006 ]`**.*
>
> *When I click **`[Copy]`** on our HDFC Regalia Card and click **`PAY ₹500`** **`[ 50,000 Paise ]`**, it launches the official Razorpay Checkout modal. I complete the payment, and it instantly captures into our live webhook transaction log.*
>
> *Now, let's see what happens when the infrastructure faces a catastrophic surge."*

---

### ⏱️ MINUTE 2 (01:00 – 02:15): High-Consequence Spike & Real-Time Self-Healing

#### 📍 Screen: Switch to Command Center Dashboard (`http://localhost:3000/dashboard`)
*(Action: Click `🔥 TRIGGER 3X FLASH-SALE SPIKE` on Simulator, then point to the Area Graph and Bank Rails)*

#### 🗣️ Read This Aloud:
> *"Let’s inject a sudden **`[ 3.5x Flash Sale Surge ]`** on the India-South UPI rail.*
>
> *(Click **`🔥 TRIGGER 3X FLASH-SALE SPIKE`**)*
>
> *Watch the Command Center: In just **`[ 250 Milliseconds ]`**, our streaming telemetry detects the incident — giving us a **`[ 60x Detection Advantage ]`** over traditional **`[ 15-Second ]`** metric scraping.*
>
> *Throughput surges past **`[ 8,000 req/min ]`**, and our **Unified Multi-Metric Area Graph** dynamically shifts from **Lime Green** to **Alert Red** as P95 tail latency breaches our **`[ 250ms SLA Boundary ]`**.*
>
> *Look at our **Upstream Bank Rails Monitor**: HDFC UPI latency spikes to **`[ 88ms ]`** with a **`[ 91.2% Success Rate ]`**. The intelligent router immediately shifts **`[ 25% ]`** of traffic share to SBI and ICICI.*
>
> *Within **`[ 3.4 Seconds — our Autonomous AI MTTR ]`**, the control loop authorizes an atomic scale-out mutation. The Kubernetes deployment scales from **`[ 2 Replicas → 3 Replicas ]`**, queue backlog of **`[ 1,450 Requests ]`** drops to **`[ Zero ]`**, P95 latency recovers to **`[ 156ms ]`**, and the vitals graph turns healthy green again — **protecting `[ 100% of Merchant GMV ]` and `[ ₹48.2 Lakhs ]` without a single dropped transaction**."*

---

### ⏱️ MINUTE 3 (02:15 – 3:30): Explainable 4-Agent Ollama Reasoning & Reasoning Cards

#### 📍 Screen: Agents View (`http://localhost:3000/agents`)
*(Action: Click `Agents` on navbar $\to$ scroll through reasoning stream $\to$ toggle `[Raw Prompt & JSON]`)*

#### 🗣️ Read This Aloud:
> *"How does the AI actually reason about infrastructure? Instead of a monolithic black-box prompt, ESA runs a governed **4-Agent Collaborative Reasoning Loop** powered by local Ollama LLMs:*
>
> 1. *The **Monitor Agent** filters streaming telemetry windows in **`[ ~15 Milliseconds ]`**.*
> 2. *The **Diagnosis Agent** isolates the failure mechanism using local LLM inference — here diagnosing a `CAPACITY_ISSUE` with **`[ 90% AI Confidence ]`** in **`[ ~1.4 Seconds ]`**.*
> 3. *The **Planning Agent** formulates Pareto-optimal action candidates in **`[ ~220 Milliseconds ]`**, balancing cloud compute cost against latency recovery.*
> 4. *The **Safety Agent** evaluates policy boundaries and issues risk advisory in **`[ ~115 Milliseconds ]`**.*
>
> *Look at our **Structured Visual Breakdown Card**: Every decision is made explainable with concrete anomaly metrics — Error Rate at **`[ 5.38% ]`** and P95 at **`[ 345ms ]`**, the plain-English LLM hypothesis, isolated root cause tags, and the approved **`[ CREATE_REPLICA ]`** mutation with the **`[ ✓ Safety Gate Invariant #4 Passed ]`** badge.*
>
> *If an infrastructure engineer or auditor needs raw transparency, toggling **`[Raw Prompt & JSON]`** displays the exact LLM prompt and JSON schema with 1-click clipboard export."*

---

### ⏱️ MINUTE 4 (03:30 – 04:20): Deterministic Action Gateway, OCC CAS & 1-Click RCA

#### 📍 Screen: Return to Dashboard (`:3000/dashboard`) $\to$ Click `[AI Incident RCA]`
*(Action: Walk through the RCA timeline, SHA-256 HMAC badge, and click `[Copy Post-Mortem Report]`)*

#### 🗣️ Read This Aloud:
> *"Here is the defining architectural pillar of ESA: **The AI never has direct execution authority on infrastructure.***
>
> *All proposals generated by agents must pass through our **Deterministic Action Gateway** in Rust in **`[ ~15 Milliseconds ]`**:*
>
> 1. *First, **Atomic Optimistic Concurrency Control (OCC CAS)**: If an agent generates a plan based on State Version 0, but the cluster state has advanced to Version 2, the gateway **atomically rejects the stale proposal** (`PolicyVerdict::StaleState`), completely eliminating race conditions.*
> 2. *Second, **Compensating Rollback**: If downstream settlement times out or execution fails, the gateway automatically restores the pre-incident cluster snapshot in **`[ Under 2 Seconds ]`**.*
> 3. *Third, **SHA-256 HMAC Tamper-Evident Audit Ledger**: Every decision is cryptographically chained into an immutable ledger that can be deterministically replayed without re-calling the LLM.*
>
> *Clicking **`[AI Incident RCA]`** generates an instant, executive post-mortem with chronological traces and cryptographic signatures ready for compliance review."*

---

### ⏱️ MINUTE 5 (04:20 – 05:00): 155-Run Benchmark Proof & The Winning Pitch

#### 📍 Screen: Benchmarks View (`http://localhost:3000/benchmarks`)
*(Action: Point to the Multi-Phase Benchmark Table and the 650 Safety Trials Stat Card)*

#### 🗣️ Read This Aloud:
> *"We didn’t just build a visual demo; we validated ESA across **`[ 155 Multi-Seed Benchmark Runs ]`** and **`[ 650 Adversarial Trials ]`** on a live Kubernetes cluster:*
>
> *Compared to static rules and adaptive baselines:*
> - *P95 tail latency dropped from **`[ 257ms → 156ms ]`** — a **`[ 39.2% Tail Latency Advantage ]`**.*
> - *Time violating SLA dropped from **`[ 16.5s → 4.1s ]`** — a **`[ 72.3% SLA Breach Reduction ]`**.*
> - *Queue stabilization time dropped from **`[ 9.6s → 2.3s ]`**.*
> - *And across 650 adversarial attempts (stale race tokens, out-of-bounds replicas, unauthorized region migrations, and LLM timeouts), ESA achieved **`[ 0 / 650 Unsafe Mutations ]`** — a **`[ 0.00% Safety Violation Rate ]`**.*
>
> *In summary: ESA proves that autonomous AI can safely operate mission-critical payment infrastructure when backed by deterministic policy gates, atomic OCC validation, and cryptographic auditability.*
>
> *This is what should exist. Thank you!"*

---

## 📊 Quick Reference Box: Key Metrics at a Glance

| Metric Name | Exact Value in Script | Where to Point on Screen |
| :--- | :--- | :--- |
| **Merchant Settlement Wallet** | `[ ₹30,46,625.80 INR ]` | Top Banner on Simulator (`:5173`) |
| **Gross Income Volume** | `[ +₹1,28,568 INR ]` (+15.7%) | Left Column Card on Simulator (`:5173`) |
| **Total Cluster Throughput** | `[ 4,033 Live TPS ]` | Donut Center on Simulator (`:5173`) |
| **Payment Rails Load Split** | `[ UPI 45%, Cards 30%, NetBanking 15% ]` | Donut Ring on Simulator (`:5173`) |
| **Streaming Detection Speed** | `[ 250 Milliseconds ]` (vs 15s) | Telemetry Event Badge on Dashboard (`:3000`) |
| **Spike Peak Ingress** | `[ 8,000+ req/min ]` | Unified Area Graph Peak on Dashboard (`:3000`) |
| **Autonomous AI MTTR** | `[ 3.4 Seconds ]` | Protected GMV Ribbon on Dashboard (`:3000`) |
| **Protected Merchant Volume** | `[ ₹48.2 Lakhs ]` (100% Retention) | Protected GMV Ribbon on Dashboard (`:3000`) |
| **Tail Latency Reduction** | `[ 257ms → 156ms ]` (**`[ 39.2% Advantage ]`**) | Benchmark Matrix on Benchmarks (`:3000/benchmarks`) |
| **SLA Violation Reduction** | `[ 16.5s → 4.1s ]` (**`[ 72.3% Reduction ]`**) | Benchmark Matrix on Benchmarks (`:3000/benchmarks`) |
| **Queue Drain Speed** | `[ 9.6s → 2.3s ]` | Benchmark Matrix on Benchmarks (`:3000/benchmarks`) |
| **Safety Invariant Violations**| `[ 0 / 650 (0.00% Violation Rate) ]` | Safety Stat Card on Benchmarks (`:3000/benchmarks`) |
| **AI Diagnosis Confidence** | `[ 90% Confidence ]` | AI Confidence Meter on Agents (`:3000/agents`) |

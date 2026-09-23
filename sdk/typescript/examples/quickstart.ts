/**
 * ESA (Executable State Architecture) — SDK Quickstart Example
 *
 * Demonstrates:
 * 1. Connecting to the live StateFabric
 * 2. Fetching real-time vitals and workload entities
 * 3. Checking 24/7 Ollama agent metrics and costs
 * 4. Cryptographically verifying the SHA-256 audit chain
 * 5. Triggering autonomous self-healing via chaos drill
 */

import { ESAClient } from '../src/index.js';

async function main() {
  const esa = new ESAClient({ apiUrl: 'http://localhost:8080' });

  console.log('⚡ Connected to ESA (Executable State Architecture)');

  // 1. Health & Vitals
  const health = await esa.health();
  console.log('Health status:', health.status);

  const vitals = await esa.vitals.getHistory();
  const latest = vitals[vitals.length - 1];
  if (latest) {
    console.log(`📊 StateFabric Vitals: TPS=${latest.total_tps.toFixed(1)}, P95=${latest.avg_p95_ms.toFixed(1)}ms, ErrorRate=${(latest.avg_error_rate * 100).toFixed(2)}%`);
  }

  // 2. Workload Entities
  const workloads = await esa.workloads.list();
  console.log(`📦 Active Workloads: ${workloads.length}`);
  for (const w of workloads) {
    console.log(`  - ${w.workload_id}: [${w.state}] Replicas=${w.replication.current_replicas}/${w.replication.max_replicas}`);
  }

  // 3. Dual-Tier Agents & 24/7 Ollama Costs
  const agents = await esa.agents.getStatus();
  console.log(`🤖 Agents online: ${agents.agents.length}`);

  const costs = await esa.agents.getCosts();
  console.log(`💰 AI Inference: ${costs.total_requests} requests, ${costs.total_output_tokens} tokens, $${costs.total_cost_usd.toFixed(6)}`);

  // 4. Multi-Gateway Autonomous Corridors (Razorpay, Stripe, PhonePe, Cashfree, Paytm, Adyen)
  const gateways = await esa.gateways.list();
  console.log(`💳 Active Gateways: ${gateways.length}`);
  for (const g of gateways) {
    console.log(`  - ${g.name}: [${g.is_healthy ? 'HEALTHY' : 'DEGRADED'}] P95=${g.p95_latency_ms.toFixed(1)}ms SLA=${(g.success_rate * 100).toFixed(1)}%`);
  }

  // 5. Universal Payment Checkout with Autonomous Failover
  const checkout = await esa.payments.checkout({
    amount: 50000,
    currency: 'INR',
    gateway: 'auto',
    method: 'UPI',
  });
  console.log(`⚡ Checkout Decision: ${checkout.transaction_id}`);
  console.log(`   Routed: ${checkout.routed_gateway} (Failover: ${checkout.failover_triggered ? 'YES' : 'NO'})`);
  console.log(`   Rationale: ${checkout.routing_reason}`);

  // 6. Audit Chain Verification
  const audit = await esa.audit.verifyChain();
  console.log(`🛡️ Audit Chain Valid: ${audit.is_valid} (${audit.total_blocks} blocks verified)`);

  // 7. Subscribe to real-time telemetry (WebSocket)
  const unsubscribe = esa.vitals.subscribe((update) => {
    console.log(`[WS Telemetry] TPS: ${update.total_tps}, P95: ${update.avg_p95_ms}ms`);
  });

  // Keep alive for 3 seconds to receive events, then cleanup
  setTimeout(() => {
    unsubscribe();
    console.log('Done!');
  }, 3000);
}

main().catch(console.error);

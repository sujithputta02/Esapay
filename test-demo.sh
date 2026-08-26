#!/bin/bash

echo "🎯 ESA Payment Gateway - Live Demo Test"
echo "========================================"
echo ""

echo "1️⃣  Checking current workload status..."
curl -s http://localhost:8080/api/workloads/payment-processor-prod | jq '{
  workload_id: .workload_id,
  state: .state,
  metrics: {
    rps: .metrics.rate_per_min,
    error_rate: .metrics.error_rate,
    p95_latency: .metrics.p95_latency_ms
  }
}'
echo ""

echo "2️⃣  Triggering traffic spike (10x multiplier)..."
curl -s -X POST http://localhost:8080/api/demo/trigger-spike \
  -H "Content-Type: application/json" \
  -d '{"workload_id": "payment-processor-prod", "multiplier": 10.0}' | jq .
echo ""

echo "3️⃣  Checking updated workload (after spike)..."
sleep 2
curl -s http://localhost:8080/api/workloads/payment-processor-prod | jq '{
  workload_id: .workload_id,
  state: .state,
  metrics: {
    rps: .metrics.rate_per_min,
    error_rate: .metrics.error_rate,
    p95_latency: .metrics.p95_latency_ms
  }
}'
echo ""

echo "4️⃣  Waiting for agents to detect and respond (15 seconds)..."
sleep 15

echo "5️⃣  Checking agent statuses..."
curl -s http://localhost:8080/api/agents/status | jq '.agents[] | {agent: .name, status: .status, task: .current_task}'
echo ""

echo "6️⃣  Checking recent autonomous actions..."
curl -s http://localhost:8080/api/actions/recent | jq '.actions[] | {action: .action_type, workload: .workload_id, status: .status, outcome: .outcome}'
echo ""

echo "7️⃣  Checking Ollama AI token usage..."
curl -s http://localhost:8080/api/metrics/tokens | jq .
echo ""

echo "✅ Demo Complete!"
echo ""
echo "🎯 What just happened:"
echo "  - Workload metrics spiked 10x"
echo "  - Monitor Agent detected anomalies"
echo "  - Diagnosis Agent used Ollama AI to analyze"
echo "  - Planning Agent proposed CREATE_REPLICA action"
echo "  - Safety Agent reviewed and approved"
echo "  - Action Gateway executed the action"
echo "  - All autonomous, no human intervention!"
echo ""
echo "📊 Check backend logs for detailed agent reasoning"

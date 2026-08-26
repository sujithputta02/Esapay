#!/bin/bash
set -e

echo "🌱 Seeding ESA Demo Data"
echo "========================"

API_URL="http://localhost:8080"

# Wait for API to be ready
echo "⏳ Waiting for API..."
for i in {1..30}; do
    if curl -s -f "$API_URL/health" > /dev/null 2>&1; then
        echo "✅ API is ready"
        break
    fi
    if [ $i -eq 30 ]; then
        echo "❌ API did not start in time"
        exit 1
    fi
    sleep 1
done

# Create sample workloads
echo "📦 Creating sample workloads..."

curl -X POST "$API_URL/api/workloads" \
  -H "Content-Type: application/json" \
  -d '{
    "workload_id": "w_payment_processor_001",
    "shard_id": "shard_south_001",
    "state": "HEALTHY",
    "region": "IN-SOUTH",
    "metrics": {
      "rate_per_min": 1000.0,
      "p50_latency_ms": 50.0,
      "p95_latency_ms": 120.0,
      "p99_latency_ms": 200.0,
      "error_rate": 0.01,
      "queue_depth": 10,
      "timestamp": "'$(date -u +"%Y-%m-%dT%H:%M:%SZ")'"
    },
    "replication": {
      "min_replicas": 2,
      "max_replicas": 5,
      "current_replicas": 2,
      "consistency_mode": "STRONG"
    },
    "locality": {
      "preferred_region": "IN-SOUTH",
      "fallback_regions": ["IN-WEST"]
    },
    "lifecycle": "ACTIVE",
    "version": 1,
    "updated_at": "'$(date -u +"%Y-%m-%dT%H:%M:%SZ")'"
  }' || echo "⚠️  Failed to create workload (may already exist)"

echo ""
echo "✅ Demo data seeded!"
echo ""
echo "🎯 Ready to demo!"
echo "   Visit: http://localhost:3000"
echo "   Click: 'Trigger Spike' button"

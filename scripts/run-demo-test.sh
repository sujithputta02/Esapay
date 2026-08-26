#!/usr/bin/env bash
# Smoke test for ESA demo path (PRD AC-21 / §24.4)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
API_URL="${API_URL:-http://localhost:8080}"

echo "== ESA demo smoke test =="
echo "API: $API_URL"

curl -sf "$API_URL/health" | head -c 200
echo ""

curl -sf -X POST "$API_URL/api/demo/seed" | head -c 300
echo ""

curl -sf -X POST "$API_URL/api/demo/scenario/burst-spike" \
  -H "Content-Type: application/json" \
  -d '{"intensity": 1.5}' | head -c 300
echo ""

curl -sf -X POST "$API_URL/api/benchmark/run" \
  -H "Content-Type: application/json" \
  -d '{"scenario":"burst","multiplier":3.0,"seed":42}' | head -c 500
echo ""

curl -sf -X POST "$API_URL/api/demo/scenario/rollback-demo" \
  -H "Content-Type: application/json" \
  -d '{}' | head -c 400
echo ""

echo "== Smoke test complete =="

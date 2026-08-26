#!/bin/bash
set -e

echo "🧪 Running ESA Test Suite"
echo "========================="

# Backend tests
echo ""
echo "🦀 Running Rust tests..."
cargo test --all --verbose

# Frontend tests (if bun test is configured)
echo ""
echo "⚛️  Running Frontend tests..."
cd frontend
if [ -f "bun.test.ts" ] || [ -d "src/__tests__" ]; then
    bun test
else
    echo "ℹ️  No frontend tests configured yet"
fi
cd ..

echo ""
echo "✅ All tests passed!"

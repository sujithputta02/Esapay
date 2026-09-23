import sys
import os
sys.path.insert(0, os.path.abspath(os.path.join(os.path.dirname(__file__), "..")))

from esa import EsaGateway

def main():
    print("Testing ESA Python SDK against live server (http://localhost:8080)...")
    esa = EsaGateway(api_url="http://localhost:8080")

    # 1. Health check
    health = esa.health()
    print("1. Health Check Response:", health)
    assert health.get("status") == "healthy", f"Expected healthy status, got {health}"

    # 2. Gateways list
    gateways = esa.gateways.list()
    print(f"2. Fetched {len(gateways)} Gateways:")
    for gw in gateways:
        print(f"   - {gw.name}: {gw.status} ({gw.p95_latency_ms}ms, {gw.success_rate*100:.1f}%)")
    assert len(gateways) > 0, "Gateways list should not be empty"

    # 3. Autonomous Checkout test
    print("3. Executing Autonomous Checkout (₹500.00 INR)...")
    decision = esa.checkout(amount=50000, currency="INR", gateway="auto", method="UPI")
    print(f"   ✓ Transaction ID:     {decision.transaction_id}")
    print(f"   ✓ Routed Gateway:     {decision.routed_gateway}")
    print(f"   ✓ Failover Triggered: {decision.failover_triggered}")
    print(f"   ✓ Rationale:          {decision.routing_reason}")
    print(f"   ✓ Session Endpoint:   {decision.checkout_url}")
    assert decision.transaction_id.startswith("tx_esa_"), "Invalid tx id"

    print("\n🎉 ALL PYTHON SDK INTEGRATION TESTS PASSED 100%!")

if __name__ == "__main__":
    main()

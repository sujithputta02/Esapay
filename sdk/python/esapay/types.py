from dataclasses import dataclass, field
from typing import Optional, Dict, Any, List
from datetime import datetime

@dataclass
class GatewayHealth:
    gateway: str
    name: str
    status: str
    p95_latency_ms: float
    success_rate: float
    active_traffic_pct: float
    is_healthy: bool

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "GatewayHealth":
        return cls(
            gateway=data.get("gateway", ""),
            name=data.get("name", ""),
            status=data.get("status", "Healthy"),
            p95_latency_ms=float(data.get("p95_latency_ms", 0.0)),
            success_rate=float(data.get("success_rate", 1.0)),
            active_traffic_pct=float(data.get("active_traffic_pct", 0.0)),
            is_healthy=bool(data.get("is_healthy", True)),
        )

@dataclass
class CheckoutDecision:
    transaction_id: str
    amount: int
    currency: str
    requested_gateway: str
    routed_gateway: str
    failover_triggered: bool
    routing_reason: str
    checkout_url: str
    timestamp: Optional[str] = None

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "CheckoutDecision":
        return cls(
            transaction_id=data.get("transaction_id", ""),
            amount=int(data.get("amount", 0)),
            currency=data.get("currency", "INR"),
            requested_gateway=data.get("requested_gateway", "auto"),
            routed_gateway=data.get("routed_gateway", ""),
            failover_triggered=bool(data.get("failover_triggered", False)),
            routing_reason=data.get("routing_reason", ""),
            checkout_url=data.get("checkout_url", ""),
            timestamp=data.get("timestamp"),
        )

@dataclass
class WorkloadEntity:
    id: str
    name: str
    namespace: str
    replicas: int
    p95_latency_ms: float
    error_rate: float
    cpu_utilization: float
    state_token: str
    status: str = "Healthy"
    metadata: Dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "WorkloadEntity":
        return cls(
            id=data.get("id", ""),
            name=data.get("name", ""),
            namespace=data.get("namespace", "default"),
            replicas=int(data.get("replicas", 1)),
            p95_latency_ms=float(data.get("p95_latency_ms", 0.0)),
            error_rate=float(data.get("error_rate", 0.0)),
            cpu_utilization=float(data.get("cpu_utilization", 0.0)),
            state_token=data.get("state_token", ""),
            status=data.get("status", "Healthy"),
            metadata=data.get("metadata", {}),
        )

@dataclass
class AuditBlock:
    index: int
    timestamp: str
    decision_id: str
    action_type: str
    previous_hash: str
    current_hash: str
    is_valid: bool = True

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "AuditBlock":
        return cls(
            index=int(data.get("index", 0)),
            timestamp=data.get("timestamp", ""),
            decision_id=data.get("decision_id", ""),
            action_type=data.get("action_type", ""),
            previous_hash=data.get("previous_hash", ""),
            current_hash=data.get("current_hash", ""),
            is_valid=bool(data.get("is_valid", True)),
        )

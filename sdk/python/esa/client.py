import json
import urllib.request
import urllib.error
import urllib.parse
from typing import Optional, Dict, Any, List, Union
from .types import GatewayHealth, CheckoutDecision, WorkloadEntity, AuditBlock

class EsaClientError(Exception):
    """Base exception for ESA Client errors."""
    def __init__(self, message: str, status_code: Optional[int] = None):
        super().__init__(message)
        self.status_code = status_code

class GatewaySubsystem:
    def __init__(self, client: "EsaGateway"):
        self._client = client

    def list(self) -> List[GatewayHealth]:
        """List all monitored gateway corridors with latency and SLA health."""
        raw = self._client._request("/api/gateways")
        if isinstance(raw, list):
            return [GatewayHealth.from_dict(item) for item in raw]
        return []

    def toggle(self, gateway: str) -> Dict[str, Any]:
        """Simulate an outage or restore health for a specific payment corridor."""
        path = f"/api/gateways/{urllib.parse.quote(gateway)}/toggle"
        return self._client._request(path, method="POST")

class WorkloadSubsystem:
    def __init__(self, client: "EsaGateway"):
        self._client = client

    def list(self) -> List[WorkloadEntity]:
        """List workloads registered in the Executable StateFabric."""
        raw = self._client._request("/api/workloads")
        if isinstance(raw, list):
            return [WorkloadEntity.from_dict(item) for item in raw]
        return []

    def get(self, workload_id: str) -> WorkloadEntity:
        """Fetch a specific workload entity by ID."""
        path = f"/api/workloads/{urllib.parse.quote(workload_id)}"
        raw = self._client._request(path)
        return WorkloadEntity.from_dict(raw)

class AuditSubsystem:
    def __init__(self, client: "EsaGateway"):
        self._client = client

    def get_blocks(self) -> List[AuditBlock]:
        """Retrieve the immutable SHA-256 decision audit chain."""
        raw = self._client._request("/api/audit/blocks")
        if isinstance(raw, list):
            return [AuditBlock.from_dict(item) for item in raw]
        return []

    def verify_chain(self) -> Dict[str, Any]:
        """Verify mathematical integrity of the SHA-256 audit ledger."""
        return self._client._request("/api/audit/verify")

class EsaGateway:
    """
    ESA (Executable State Architecture) Official Python SDK
    Autonomous Multi-Gateway Resilience & Self-Healing Financial Infrastructure
    """
    def __init__(
        self,
        api_url: str = "http://localhost:8080",
        api_key: Optional[str] = None,
        timeout: float = 10.0,
        auto_failover: bool = True,
        corridors: Optional[List[str]] = None,
    ):
        self.api_url = api_url.rstrip("/")
        self.api_key = api_key
        self.timeout = timeout
        self.auto_failover = auto_failover
        self.corridors = corridors or ["razorpay", "phonepe", "paytm", "cashfree"]

        # Subsystems
        self.gateways = GatewaySubsystem(self)
        self.workloads = WorkloadSubsystem(self)
        self.audit = AuditSubsystem(self)

    def _request(
        self,
        path: str,
        method: str = "GET",
        data: Optional[Dict[str, Any]] = None,
    ) -> Any:
        url = f"{self.api_url}{path}"
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json",
            "User-Agent": "esa-python-sdk/1.0.0",
        }
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"

        body = json.dumps(data).encode("utf-8") if data is not None else None
        req = urllib.request.Request(url, data=body, headers=headers, method=method)

        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as response:
                content = response.read().decode("utf-8")
                if not content:
                    return {}
                return json.loads(content)
        except urllib.error.HTTPError as e:
            err_body = e.read().decode("utf-8") if e.fp else ""
            raise EsaClientError(
                f"ESA API Error [{e.code}]: {err_body or e.reason}",
                status_code=e.code
            ) from e
        except urllib.error.URLError as e:
            raise EsaClientError(f"Failed to connect to ESA Control Plane at {self.api_url}: {e.reason}") from e

    def health(self) -> Dict[str, Any]:
        """Check the health status of the ESA control plane cluster."""
        return self._request("/health")

    def checkout(
        self,
        amount: int,
        currency: str = "INR",
        gateway: str = "auto",
        method: str = "UPI",
    ) -> CheckoutDecision:
        """
        Execute an autonomous payment routing decision.
        If a bank rail or gateway is degraded, ESA safely executes sub-second
        failover to an optimal healthy rail.
        """
        payload = {
            "amount": amount,
            "currency": currency,
            "gateway": gateway,
            "method": method,
        }
        raw = self._request("/api/payments/checkout", method="POST", data=payload)
        return CheckoutDecision.from_dict(raw)

# Alias for backwards compatibility
ESAClient = EsaGateway

import os
import json
import urllib.request
import urllib.error
import urllib.parse
from typing import Optional, Dict, Any, List, Union
from .types import (
    GatewayHealth,
    CheckoutDecision,
    PaymentTransactionStatus,
    WorkloadEntity,
    AuditBlock,
)

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
        api_url: Optional[str] = None,
        api_key: Optional[str] = None,
        timeout: float = 10.0,
        auto_failover: bool = True,
        corridors: Optional[List[str]] = None,
    ):
        creds = {}
        cred_path = os.path.expanduser("~/.esa/credentials.json")
        if os.path.exists(cred_path):
            try:
                with open(cred_path, "r", encoding="utf-8") as f:
                    creds = json.load(f)
            except Exception:
                pass

        if not api_url:
            api_url = os.environ.get("ESA_API_URL") or creds.get("api_url") or "https://esapay-api.onrender.com"
        self.api_url = api_url.rstrip("/")

        if not api_key:
            api_key = (
                os.environ.get("ESA_API_KEY")
                or os.environ.get("ESA_LIVE_API_KEY")
                or os.environ.get("ESA_TEST_API_KEY")
                or creds.get("api_key")
            )
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
        extra_headers: Optional[Dict[str, str]] = None,
    ) -> Any:
        url = f"{self.api_url}{path}"
        headers = {
            "Content-Type": "application/json",
            "Accept": "application/json",
            "User-Agent": "esa-python-sdk/1.0.0",
        }
        if self.api_key:
            headers["Authorization"] = f"Bearer {self.api_key}"
        if extra_headers:
            headers.update(extra_headers)

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
        idempotency_key: Optional[str] = None,
    ) -> CheckoutDecision:
        """
        Execute an autonomous payment routing decision.
        If a bank rail or gateway is degraded, ESA safely executes sub-second
        failover to an optimal healthy rail.
        Pass `idempotency_key` so retries after a client timeout safely return
        the original routing decision without creating a duplicate attempt.
        """
        payload: Dict[str, Any] = {
            "amount": amount,
            "currency": currency,
            "gateway": gateway,
            "method": method,
        }
        extra_headers = None
        if idempotency_key:
            payload["idempotency_key"] = idempotency_key
            extra_headers = {"Idempotency-Key": idempotency_key}
        raw = self._request(
            "/api/payments/checkout",
            method="POST",
            data=payload,
            extra_headers=extra_headers,
        )
        return CheckoutDecision.from_dict(raw)

    def get_payment_status(self, transaction_id: str) -> PaymentTransactionStatus:
        """Look up the durable state of a payment attempt by transaction_id."""
        path = f"/api/payments/transactions/{urllib.parse.quote(transaction_id, safe='')}"
        raw = self._request(path)
        return PaymentTransactionStatus.from_dict(raw)

# Alias for backwards compatibility
ESAClient = EsaGateway

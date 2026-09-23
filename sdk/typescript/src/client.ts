import type {
  ESAClientConfig,
  WorkloadEntity,
  VitalsSnapshot,
  AgentStatusResponse,
  AICostMetrics,
  AuditBlock,
  AuditVerificationResult,
  PaymentOrderParams,
  PaymentOrderResponse,
  GatewayHealth,
  CheckoutParams,
  CheckoutDecision,
} from './types.js';

export class ESAClient {
  private readonly apiUrl: string;
  private readonly wsUrl: string;
  private readonly apiKey?: string;
  private readonly timeoutMs: number;

  constructor(config: ESAClientConfig = {}) {
    this.apiUrl = (config.apiUrl || 'http://localhost:8080').replace(/\/$/, '');
    this.wsUrl = (config.wsUrl || this.apiUrl.replace(/^http/, 'ws')).replace(/\/$/, '');
    this.apiKey = config.apiKey;
    this.timeoutMs = config.timeoutMs || 10000;
  }

  private async request<T>(path: string, options: RequestInit = {}): Promise<T> {
    const url = `${this.apiUrl}${path}`;
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      ...(this.apiKey ? { Authorization: `Bearer ${this.apiKey}` } : {}),
      ...(options.headers as Record<string, string>),
    };

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);

    try {
      const response = await fetch(url, {
        ...options,
        headers,
        signal: controller.signal,
      });

      if (!response.ok) {
        const errorText = await response.text().catch(() => '');
        throw new Error(`ESA API Error [${response.status}]: ${errorText || response.statusText}`);
      }

      return (await response.json()) as T;
    } finally {
      clearTimeout(timer);
    }
  }

  /**
   * Health check for the ESA control plane
   */
  async health(): Promise<{ status: string; timestamp?: string }> {
    return this.request<{ status: string; timestamp?: string }>('/health');
  }

  async getHealth(): Promise<{ status: string; timestamp?: string }> {
    return this.health();
  }

  /**
   * Workload Management in the Executable StateFabric
   */
  readonly workloads = {
    list: async (): Promise<WorkloadEntity[]> => {
      return this.request<WorkloadEntity[]>('/api/workloads');
    },

    get: async (id: string): Promise<WorkloadEntity> => {
      return this.request<WorkloadEntity>(`/api/workloads/${encodeURIComponent(id)}`);
    },

    create: async (workload: Partial<WorkloadEntity>): Promise<WorkloadEntity> => {
      return this.request<WorkloadEntity>('/api/workloads', {
        method: 'POST',
        body: JSON.stringify(workload),
      });
    },
  };

  /**
   * System Vitals & Real-Time Telemetry
   */
  readonly vitals = {
    getHistory: async (): Promise<VitalsSnapshot[]> => {
      return this.request<VitalsSnapshot[]>('/api/vitals/history');
    },

    /**
     * Subscribe to real-time vitals updates via WebSocket.
     * Returns an unsubscribe function.
     */
    subscribe: (callback: (vitals: VitalsSnapshot) => void): (() => void) => {
      const wsUrl = `${this.wsUrl}/ws/telemetry`;
      const WebSocketCtor = (globalThis as any).WebSocket;

      if (!WebSocketCtor) {
        console.warn('[ESA SDK] WebSocket is not natively supported in this runtime.');
        return () => {};
      }

      const ws = new WebSocketCtor(wsUrl);

      ws.onmessage = (event: any) => {
        try {
          const data = JSON.parse(typeof event.data === 'string' ? event.data : event.data.toString());
          if (data.type === 'VitalsUpdate' || data.total_tps !== undefined) {
            callback(data);
          }
        } catch {
          // ignore malformed frame
        }
      };

      return () => {
        try {
          ws.close();
        } catch {}
      };
    },
  };

  /**
   * Dual-Tier AI Agents & Inference Cost Tracking
   */
  readonly agents = {
    getStatus: async (): Promise<AgentStatusResponse> => {
      return this.request<AgentStatusResponse>('/api/agents/status');
    },

    getCosts: async (): Promise<AICostMetrics> => {
      return this.request<AICostMetrics>('/api/costs/ai');
    },
  };

  /**
   * Cryptographic SHA-256 Audit Trail & Formal Verification
   */
  readonly audit = {
    verifyChain: async (): Promise<AuditVerificationResult> => {
      return this.request<AuditVerificationResult>('/api/audit/verify-chain');
    },

    getTrail: async (limit: number = 20): Promise<AuditBlock[]> => {
      return this.request<AuditBlock[]>(`/api/audit/trail?limit=${limit}`);
    },

    replay: async (decisionId: string): Promise<{ success: boolean; message: string }> => {
      return this.request<{ success: boolean; message: string }>(
        `/api/audit/replay/${encodeURIComponent(decisionId)}`,
        { method: 'POST' }
      );
    },
  };

  /**
   * Chaos Engineering & Demo Drills
   */
  readonly chaos = {
    triggerSpike: async (): Promise<{ status: string; message: string }> => {
      return this.request<{ status: string; message: string }>('/api/demo/trigger-spike', {
        method: 'POST',
      });
    },

    triggerScenario: async (scenario: string): Promise<{ status: string; scenario: string }> => {
      return this.request<{ status: string; scenario: string }>(
        `/api/demo/scenario/${encodeURIComponent(scenario)}`,
        { method: 'POST' }
      );
    },

    seed: async (): Promise<{ status: string; count: number }> => {
      return this.request<{ status: string; count: number }>('/api/demo/seed', {
        method: 'POST',
      });
    },
  };

  /**
   * Multi-Gateway Autonomous Corridors (Razorpay, Stripe, PhonePe, Cashfree, Paytm, Adyen)
   */
  readonly gateways = {
    /**
     * List all monitored gateways, their live SLA health, P95 latency, and traffic allocation
     */
    list: async (): Promise<GatewayHealth[]> => {
      return this.request<GatewayHealth[]>('/api/gateways');
    },

    /**
     * Simulate an outage or toggle healthy status for a specific gateway
     */
    toggle: async (gateway: string): Promise<{ gateway: string; is_healthy: boolean; message: string }> => {
      return this.request<{ gateway: string; is_healthy: boolean; message: string }>(
        `/api/gateways/${encodeURIComponent(gateway)}/toggle`,
        { method: 'POST' }
      );
    },
  };

  /**
   * Self-Healing Universal Payments & Multi-Gateway Routing
   */
  readonly payments = {
    /**
     * Universal checkout with automated AI failover across Razorpay, Stripe, PhonePe, Cashfree, etc.
     */
    checkout: async (params: CheckoutParams): Promise<CheckoutDecision> => {
      return this.request<CheckoutDecision>('/api/payments/checkout', {
        method: 'POST',
        body: JSON.stringify(params),
      });
    },

    createOrder: async (params: PaymentOrderParams): Promise<PaymentOrderResponse> => {
      return this.request<PaymentOrderResponse>('/api/razorpay/orders', {
        method: 'POST',
        body: JSON.stringify(params),
      });
    },

    confirm: async (params: {
      razorpay_payment_id: string;
      razorpay_order_id: string;
      razorpay_signature: string;
    }): Promise<{ success: boolean; order_id: string }> => {
      return this.request<{ success: boolean; order_id: string }>('/api/razorpay/confirm', {
        method: 'POST',
        body: JSON.stringify(params),
      });
    },
  };

  /**
   * Generates the direct dashboard URL configured for this server.
   * Allows any user to open the ESA web dashboard pointed at their own backend.
   */
  getDashboardUrl(options?: { dashboardBaseUrl?: string }): string {
    const base = options?.dashboardBaseUrl || 'http://localhost:3000';
    return `${base.replace(/\/$/, '')}/?server=${encodeURIComponent(this.apiUrl)}`;
  }

  /**
   * Opens the ESA web dashboard in the default browser pointed directly at this server.
   * Works in Node.js, Bun, and desktop environments.
   */
  async openDashboard(options?: { dashboardBaseUrl?: string }): Promise<string> {
    const targetUrl = this.getDashboardUrl(options);
    console.log(`🌐 Launching ESA Dashboard: ${targetUrl}`);

    const proc = (globalThis as any).process;
    if (proc && proc.platform) {
      try {
        const cp = await (Function('return import("node:child_process")')() as Promise<any>);
        const cmd =
          proc.platform === 'darwin'
            ? 'open'
            : proc.platform === 'win32'
            ? 'start'
            : 'xdg-open';
        cp.exec(`${cmd} "${targetUrl}"`);
      } catch (e) {
        console.warn(`Could not auto-open browser: ${e}`);
      }
    }
    return targetUrl;
  }

  /**
   * Top-level checkout convenience method
   */
  async checkout(params: CheckoutParams): Promise<CheckoutDecision> {
    return this.payments.checkout(params);
  }
}

export const EsaGateway = ESAClient;
export type EsaGateway = ESAClient;

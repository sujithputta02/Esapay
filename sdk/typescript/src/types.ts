/**
 * ESA (Executable State Architecture) — Core TypeScript Types
 */

export type WorkloadState = 'Healthy' | 'Degraded' | 'Critical' | 'Rebalancing';

export type Region = 'IndiaSouth' | 'IndiaWest' | 'IndiaNorth' | 'Global';

export type ConsistencyMode = 'Strong' | 'Eventual' | 'Linearizable';

export interface WorkloadMetrics {
  rate_per_min: number;
  p50_latency_ms: number;
  p95_latency_ms: number;
  p99_latency_ms: number;
  error_rate: number;
  queue_depth: number;
  timestamp: string;
}

export interface ReplicationPolicy {
  min_replicas: number;
  max_replicas: number;
  current_replicas: number;
  consistency_mode: ConsistencyMode;
}

export interface LocalityPreference {
  preferred_region: Region;
  fallback_regions: Region[];
}

export interface WorkloadEntity {
  workload_id: string;
  shard_id: string;
  state: WorkloadState;
  region: Region;
  metrics: WorkloadMetrics;
  replication: ReplicationPolicy;
  locality: LocalityPreference;
  lifecycle: 'Active' | 'Draining' | 'Decommissioned';
  version: number;
  updated_at: string;
}

export interface VitalsSnapshot {
  timestamp: string;
  total_tps: number;
  avg_p95_ms: number;
  avg_error_rate: number;
  total_queue: number;
  healthy_count: number;
  degraded_count: number;
}

export interface AgentInfo {
  name: string;
  status: 'Active' | 'Ready' | 'Standby' | 'Degraded';
  model: string;
  avg_latency_ms: number;
}

export interface AgentStatusResponse {
  agents: AgentInfo[];
  fluid_reasoner: {
    available: boolean;
    endpoint: string;
  };
  ollama: {
    available: boolean;
    model: string;
    live_24_7: boolean;
  };
}

export interface AICostMetrics {
  total_requests: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cost_usd: number;
  cache_hit_rate: number;
}

export interface AuditBlock {
  block_index: number;
  decision_id: string;
  timestamp: string;
  action_type: string;
  proposed_by: string;
  risk_level: 'Low' | 'Medium' | 'High' | 'Critical';
  previous_hash: string;
  block_hash: string;
  effects_verified: boolean;
}

export interface AuditVerificationResult {
  is_valid: boolean;
  total_blocks: number;
  latest_hash: string;
  tamper_detected: boolean;
  errors?: string[];
}

export interface PaymentOrderParams {
  amount: number;
  currency: string;
  receipt?: string;
  notes?: Record<string, string>;
}

export interface PaymentOrderResponse {
  id: string;
  amount: number;
  currency: string;
  receipt?: string;
  status: string;
  created_at: number;
}

export type PaymentGatewayType =
  | 'auto'
  | 'razorpay'
  | 'stripe'
  | 'phonepe'
  | 'cashfree'
  | 'paytm'
  | 'adyen';

export interface GatewayHealth {
  gateway: string;
  name: string;
  status: string;
  p95_latency_ms: number;
  success_rate: number;
  active_traffic_pct: number;
  is_healthy: boolean;
}

export interface CheckoutParams {
  amount: number;
  currency?: string;
  gateway?: PaymentGatewayType;
  method?: string;
  customer_id?: string;
}

export interface CheckoutDecision {
  transaction_id: string;
  amount: number;
  currency: string;
  requested_gateway: string;
  routed_gateway: string;
  failover_triggered: boolean;
  routing_reason: string;
  checkout_url: string;
  timestamp: string;
}

export interface ESAClientConfig {
  /**
   * Base URL of the ESA Control Plane API (default: http://localhost:8080)
   */
  apiUrl?: string;

  /**
   * WebSocket URL for real-time telemetry streaming (default: ws://localhost:8080)
   */
  wsUrl?: string;

  /**
   * Optional API Key for authenticated endpoints
   */
  apiKey?: string;

  /**
   * Request timeout in milliseconds (default: 10000ms)
   */
  timeoutMs?: number;
}

// ESA Frontend Types

export interface WorkloadEntity {
  workload_id: string;
  shard_id: string;
  state: WorkloadState;
  region: Region;
  metrics: WorkloadMetrics;
  replication: ReplicationPolicy;
  locality: LocalityPreference;
  lifecycle: LifecycleState;
  version: number;
  updated_at: string;
}

export type WorkloadState = 'HEALTHY' | 'DEGRADED' | 'OVERLOADED' | 'RECOVERING';

export type Region = 'IN-SOUTH' | 'IN-WEST' | 'IN-NORTH' | 'US-EAST' | 'EU-WEST';

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
  consistency_mode: 'STRONG' | 'EVENTUAL' | 'CAUSAL';
}

export interface LocalityPreference {
  preferred_region: Region;
  fallback_regions: Region[];
}

export type LifecycleState = 'CREATE' | 'WARM' | 'ACTIVE' | 'DRAINING' | 'RETIRED';

export interface Condition {
  condition_type: ConditionType;
  workload_id: string;
  severity: Severity;
  description: string;
  metrics: any;
}

export type ConditionType = 'HIGH_LATENCY' | 'QUEUE_BACKLOG' | 'HIGH_ERROR_RATE' | 'WORKLOAD_DEGRADED' | 'NODE_FAILURE';

export type Severity = 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL';

export interface Diagnosis {
  hypothesis: string;
  root_cause: RootCause;
  confidence: number;
  evidence_refs: string[];
  recommended_action: string | null;
}

export type RootCause = 'HOT_PARTITION' | 'CAPACITY_ISSUE' | 'NODE_DEGRADATION' | 'TRAFFIC_SPIKE' | 'OTHER' | 'NONE';

export interface ActionProposal {
  proposal_id: string;
  action: ActionType;
  proposed_by: AgentId;
  proposed_at: string;
  evidence_refs: string[];
  priority: ActionPriority;
}

export type ActionType =
  | { action: 'CREATE_REPLICA'; workload_id: string; target_region: Region; reason: string; confidence: number; risk: RiskLevel; state_version: number }
  | { action: 'SHIFT_ROUTE'; workload_id: string; from_region: Region; to_region: Region; traffic_percentage: number; reason: string }
  | { action: 'MIGRATE_PARTITION'; workload_id: string; shard_id: string; target_region: Region }
  | { action: 'THROTTLE_WORKLOAD'; workload_id: string; throttle_percentage: number }
  | { action: 'ROLLBACK'; original_action_id: string; reason: string };

export type AgentId = 'monitor' | 'diagnosis' | 'planning' | 'safety';

export type ActionPriority = 'LOW' | 'NORMAL' | 'HIGH' | 'URGENT';

export type RiskLevel = 'LOW' | 'MEDIUM' | 'HIGH' | 'CRITICAL';

export interface ActionExecution {
  execution_id: string;
  proposal_id: string;
  action: ActionType;
  executed_at: string;
  completed_at: string | null;
  outcome: ActionOutcome | null;
  before_metrics: any;
  after_metrics: any | null;
  error_message: string | null;
}

export type ActionOutcome = 'SUCCESS' | 'FAILED' | 'ROLLED_BACK' | 'PARTIAL';

export interface PolicyResult {
  decision_id: string;
  proposal_id: string;
  verdict: PolicyVerdict;
  rule_ids: string[];
  risk_score: number;
  modifications: string[];
  explanation: string;
}

export type PolicyVerdict = 
  | { verdict: 'ALLOWED' }
  | { verdict: 'DENIED'; reason: string }
  | { verdict: 'MODIFIED'; modifications: string[] }
  | { verdict: 'REQUIRES_APPROVAL'; reason: string };

export interface TokenStats {
  total_requests: number;
  total_input_tokens: number;
  total_output_tokens: number;
}

export interface TelemetryMessage {
  type: 'workload_update' | 'agent_activity' | 'condition_detected' | 'action_proposed' | 'action_executed' | 'policy_decision' | 'vitals_update';
  [key: string]: any;
}

export interface VitalsSnapshot {
  timestamp: string;
  total_tps: number;
  avg_p95_ms: number;
  avg_error_rate: number;
  total_queue: number;
  healthy_count: number;
  degraded_count: number;
  total_workloads?: number;
}

export interface AgentStatus {
  agent_id: AgentId;
  status: 'IDLE' | 'OBSERVING' | 'REASONING' | 'ACTING' | 'COMPLETE';
  current_task: string | null;
  confidence: number | null;
  latest_observation: string | null;
  latest_decision: string | null;
}

export interface PaymentAttemptSummary {
  transaction_id: string;
  merchant_id: string;
  provider: string;
  provider_order_id: string | null;
  payment_reference: string | null;
  amount_minor: number;
  currency: string;
  state: string;
  created_at: string;
  updated_at: string;
}

export interface PaymentIntelligenceSummaryResponse {
  merchant_id: string;
  total_attempts: number;
  state_counts: Record<string, number>;
  provider_counts: Record<string, number>;
  recent_attempts: PaymentAttemptSummary[];
  inference_provider: string;
  model_id: string;
  read_only: boolean;
}

export interface PaymentInvestigationReport {
  investigation_id: string;
  payment_reference: string;
  authoritative_payment_state: string;
  observed_provider_state: string | null;
  issue_summary: string;
  probable_root_cause: string;
  confidence: number;
  supporting_evidence_refs: string[];
  alternative_explanations: string[];
  uncertainty_notes: string[];
  recommended_next_investigation: string[];
  memory_refs_used: string[];
  provider_used: string;
  model_id: string;
  deterministic_fallback_used: boolean;
  evaluation_id: string | null;
}

export interface PaymentInvestigationResponse {
  context: {
    investigation_id: string;
    authoritative_state: string;
    attempt_state: string;
    allowed_evidence_refs: string[];
    missing_data: string[];
    refund_summary: {
      captured_amount_minor: number;
      processed_refunds_minor: number;
      pending_reservations_minor: number;
      remaining_refundable_minor: number;
    };
  };
  report: PaymentInvestigationReport;
  read_only: boolean;
}

export interface PaymentMemoryRecord {
  memory_id: string;
  memory_type: 'working' | 'episodic' | 'semantic' | 'procedural';
  scope: 'system' | 'merchant';
  merchant_id: string | null;
  provider: string | null;
  incident_category: string;
  lifecycle_state: string | null;
  title: string;
  summary: string;
  resolution: string | null;
  source_refs: string[];
  verification_status: 'unverified' | 'verified' | 'rejected';
  version: number;
  created_at: string;
  updated_at: string;
  expires_at: string | null;
}

export interface PaymentMemoryRecentResponse {
  merchant_id: string;
  count: number;
  records: PaymentMemoryRecord[];
  read_only: boolean;
}

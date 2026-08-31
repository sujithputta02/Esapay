import { useQuery } from '@tanstack/react-query';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { CheckCircle, XCircle, AlertCircle, Shield, AlertTriangle } from 'lucide-react';

interface Verdict {
  verdict_id: string;
  decision_id: string;
  timestamp: string;
  verdict: string;
  action_type: string;
  workload_id: string;
  rules_passed?: string[];
  rules_failed?: string[];
  explanation: string;
}

interface VerdictStats {
  total_decisions: number;
  allow_count: number;
  deny_count: number;
  stale_state_count: number;
  requires_approval_count: number;
  allow_rate: number;
  deny_rate: number;
  stale_rate: number;
  approval_required_rate: number;
}

const getVerdictIcon = (verdict: string) => {
  switch (verdict.toUpperCase()) {
    case 'ALLOW':
      return <CheckCircle className="w-5 h-5 text-accent" />;
    case 'DENY':
      return <XCircle className="w-5 h-5 text-error" />;
    case 'STALE_STATE':
      return <AlertCircle className="w-5 h-5 text-warning" />;
    case 'REQUIRES_APPROVAL':
      return <AlertTriangle className="w-5 h-5 text-warning" />;
    default:
      return <Shield className="w-5 h-5 text-accent" />;
  }
};

export function PolicyView() {
  const { data: verdictsData } = useQuery({
    queryKey: ['verdicts'],
    queryFn: async () => {
      const response = await apiClient.getRecentVerdicts();
      return response as { verdicts: Verdict[]; total: number };
    },
    refetchInterval: 2000,
  });

  const { data: statsData } = useQuery({
    queryKey: ['verdict-stats'],
    queryFn: async () => {
      const response = await apiClient.getVerdictStats();
      return response as { stats: VerdictStats };
    },
    refetchInterval: 2000,
  });

  const verdicts = verdictsData?.verdicts || [];
  const stats = statsData?.stats || {
    total_decisions: 0,
    allow_count: 0,
    deny_count: 0,
    stale_state_count: 0,
    requires_approval_count: 0,
    allow_rate: 0,
    deny_rate: 0,
    stale_rate: 0,
    approval_required_rate: 0,
  };

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-[28px] font-bold text-white tracking-tight">Policy Engine & Verifier Verdicts</h1>
        <p className="text-[15px] text-[#B8B8B8] mt-1">
          Deterministic safety bounds, state-version invariant checks, and Kubernetes replica constraint verifications.
        </p>
      </div>

      {/* Verdict Statistics (rounded-[22px] bg-[#333333]) */}
      <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-4">
        <div className="bg-[#333333] rounded-[22px] p-5 min-h-[110px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[14px] font-medium text-[#B8B8B8]">Total Decisions</span>
          <span className="text-[26px] font-extrabold text-white tracking-tight">{stats.total_decisions}</span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-5 min-h-[110px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[14px] font-medium text-accent">ALLOW Rate</span>
          <div>
            <span className="text-[26px] font-extrabold text-accent tracking-tight">{stats.allow_count}</span>
            <span className="text-xs text-[#777777] ml-1.5">({(stats.allow_rate * 100).toFixed(0)}%)</span>
          </div>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-5 min-h-[110px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[14px] font-medium text-error">DENY</span>
          <div>
            <span className="text-[26px] font-extrabold text-error tracking-tight">{stats.deny_count}</span>
            <span className="text-xs text-[#777777] ml-1.5">({(stats.deny_rate * 100).toFixed(0)}%)</span>
          </div>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-5 min-h-[110px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[14px] font-medium text-warning">STALE STATE</span>
          <div>
            <span className="text-[26px] font-extrabold text-warning tracking-tight">{stats.stale_state_count}</span>
            <span className="text-xs text-[#777777] ml-1.5">({(stats.stale_rate * 100).toFixed(0)}%)</span>
          </div>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-5 min-h-[110px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[14px] font-medium text-[#B8B8B8]">APPROVAL</span>
          <div>
            <span className="text-[26px] font-extrabold text-white tracking-tight">{stats.requires_approval_count}</span>
            <span className="text-xs text-[#777777] ml-1.5">({(stats.approval_required_rate * 100).toFixed(0)}%)</span>
          </div>
        </div>
      </div>

      {/* Verdict Breakdown Progress & Rules */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-8">
        {/* Distribution Progress */}
        <div className="bg-[#272727] rounded-[32px] p-7 sm:p-8 border border-white/[0.04] space-y-5">
          <h3 className="text-[20px] font-bold text-white">Verdict Distribution</h3>
          <div className="space-y-4">
            {[
              { label: 'ALLOW (Passed Safety)', count: stats.allow_count, rate: stats.allow_rate, color: 'bg-accent' },
              { label: 'DENY (Policy Violation)', count: stats.deny_count, rate: stats.deny_rate, color: 'bg-error' },
              { label: 'STALE_STATE (Concurrency Invariant)', count: stats.stale_state_count, rate: stats.stale_rate, color: 'bg-warning' },
              { label: 'REQUIRES_APPROVAL', count: stats.requires_approval_count, rate: stats.approval_required_rate, color: 'bg-[#777777]' },
            ].map((item, idx) => (
              <div key={idx} className="space-y-1.5">
                <div className="flex items-center justify-between text-xs">
                  <span className="font-semibold text-white">{item.label}</span>
                  <span className="text-[#B8B8B8] font-mono">
                    {item.count} ({(item.rate * 100).toFixed(1)}%)
                  </span>
                </div>
                <div className="h-2 bg-[#1D1E1C] rounded-full overflow-hidden">
                  <div className={`h-full ${item.color} transition-all`} style={{ width: `${Math.max(item.rate * 100, item.count > 0 ? 5 : 0)}%` }} />
                </div>
              </div>
            ))}
          </div>
        </div>

        {/* Policy Rules Reference */}
        <div className="bg-[#272727] rounded-[32px] p-7 sm:p-8 border border-white/[0.04] space-y-4">
          <h3 className="text-[20px] font-bold text-white">Deterministic Invariants</h3>
          <div className="space-y-3">
            <div className="p-3.5 bg-[#333333] rounded-[16px] border border-white/[0.03]">
              <p className="font-bold text-accent text-xs uppercase tracking-wide">RULE_001_REPLICA_BOUNDS</p>
              <p className="text-xs text-[#B8B8B8] mt-1">Guarantees Kubernetes pod replicas stay within min 2 and max 30 constraint bounds</p>
            </div>
            <div className="p-3.5 bg-[#333333] rounded-[16px] border border-white/[0.03]">
              <p className="font-bold text-accent text-xs uppercase tracking-wide">RULE_002_STALE_STATE_INVARIANT</p>
              <p className="text-xs text-[#B8B8B8] mt-1">Atomically rejects mutation if state version incremented during LLM inference loop</p>
            </div>
            <div className="p-3.5 bg-[#333333] rounded-[16px] border border-white/[0.03]">
              <p className="font-bold text-accent text-xs uppercase tracking-wide">RULE_003_CIRCUIT_BREAKER</p>
              <p className="text-xs text-[#B8B8B8] mt-1">Automatically shifts traffic to fallback regions if P95 latency exceeds 250ms SLA</p>
            </div>
          </div>
        </div>
      </div>

      {/* Recent Verdicts */}
      <div className="bg-[#272727] rounded-[32px] p-7 sm:p-9 border border-white/[0.04] space-y-6">
        <div className="flex items-center justify-between">
          <h3 className="text-[20px] font-bold text-white">Recent Policy Verdicts</h3>
          <span className="text-xs font-mono text-[#777777]">{verdicts.length} Evaluated</span>
        </div>

        {verdicts.length > 0 ? (
          <div className="space-y-4">
            {verdicts.map((verdict) => (
              <div
                key={verdict.verdict_id}
                className="p-6 rounded-[22px] bg-[#333333] border border-white/[0.03] space-y-3"
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    {getVerdictIcon(verdict.verdict)}
                    <div>
                      <p className="font-mono text-sm font-bold text-white">
                        {verdict.decision_id.substring(0, 16)}...
                      </p>
                      <p className="text-xs text-[#777777] mt-0.5">
                        {new Date(verdict.timestamp).toLocaleTimeString()}
                      </p>
                    </div>
                  </div>
                  <Badge variant={verdict.verdict === 'ALLOW' ? 'success' : 'warning'}>
                    {verdict.verdict}
                  </Badge>
                </div>

                <div className="p-4 bg-[#1D1E1C] rounded-[16px] border border-white/[0.04] text-xs space-y-2">
                  <p className="text-[#B8B8B8]">
                    <strong className="text-white">Action:</strong> {verdict.action_type} on{' '}
                    <span className="font-mono text-accent">{verdict.workload_id}</span>
                  </p>
                  <p className="text-white font-medium">{verdict.explanation}</p>
                </div>

                {verdict.rules_passed && verdict.rules_passed.length > 0 && (
                  <div className="flex flex-wrap items-center gap-2 text-xs pt-1">
                    <span className="text-accent font-semibold">Rules Passed:</span>
                    {verdict.rules_passed.map((rule, idx) => (
                      <Badge key={idx} variant="accent">
                        {rule}
                      </Badge>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        ) : (
          <div className="text-center py-16 text-[#777777] space-y-2">
            <Shield className="w-12 h-12 text-[#777777] mx-auto mb-2 opacity-20" />
            <p className="text-[15px] font-medium text-white">No policy evaluations logged yet</p>
            <p className="text-xs text-[#777777]">
              Verdicts will stream in live as actions are verified against policy constraints
            </p>
          </div>
        )}
      </div>
    </div>
  );
}


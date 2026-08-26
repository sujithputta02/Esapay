import { useQuery } from '@tanstack/react-query';
import { Card, CardHeader, CardTitle, CardBody } from '@/components/ui/Card';
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
      return <CheckCircle className="w-5 h-5 text-green-500" />;
    case 'DENY':
      return <XCircle className="w-5 h-5 text-red-500" />;
    case 'STALE_STATE':
      return <AlertCircle className="w-5 h-5 text-yellow-500" />;
    case 'REQUIRES_APPROVAL':
      return <AlertTriangle className="w-5 h-5 text-orange-500" />;
    default:
      return <Shield className="w-5 h-5 text-blue-500" />;
  }
};

const getVerdictColor = (verdict: string) => {
  switch (verdict.toUpperCase()) {
    case 'ALLOW':
      return 'success';
    case 'DENY':
      return 'error';
    case 'STALE_STATE':
      return 'warning';
    case 'REQUIRES_APPROVAL':
      return 'warning';
    default:
      return 'default';
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
    <div className="p-8 space-y-8">
      <div>
        <h1 className="text-h1 font-bold text-text-primary">Policy Verdicts</h1>
        <p className="text-body text-text-secondary mt-2">
          Real-time policy evaluation verdicts (ALLOW / DENY / STALE_STATE / REQUIRES_APPROVAL)
        </p>
      </div>

      {/* Verdict Statistics */}
      <div className="grid grid-cols-1 md:grid-cols-5 gap-4">
        <Card>
          <CardBody className="text-center">
            <p className="text-text-secondary text-small">Total Decisions</p>
            <p className="text-h2 font-bold text-text-primary mt-2">{stats.total_decisions}</p>
          </CardBody>
        </Card>

        <Card>
          <CardBody className="text-center">
            <p className="text-green-600 font-semibold text-small">ALLOW</p>
            <p className="text-h3 font-bold text-green-600 mt-2">{stats.allow_count}</p>
            <p className="text-micro text-text-secondary mt-1">{(stats.allow_rate * 100).toFixed(0)}%</p>
          </CardBody>
        </Card>

        <Card>
          <CardBody className="text-center">
            <p className="text-red-600 font-semibold text-small">DENY</p>
            <p className="text-h3 font-bold text-red-600 mt-2">{stats.deny_count}</p>
            <p className="text-micro text-text-secondary mt-1">{(stats.deny_rate * 100).toFixed(0)}%</p>
          </CardBody>
        </Card>

        <Card>
          <CardBody className="text-center">
            <p className="text-yellow-600 font-semibold text-small">STALE_STATE</p>
            <p className="text-h3 font-bold text-yellow-600 mt-2">{stats.stale_state_count}</p>
            <p className="text-micro text-text-secondary mt-1">{(stats.stale_rate * 100).toFixed(0)}%</p>
          </CardBody>
        </Card>

        <Card>
          <CardBody className="text-center">
            <p className="text-orange-600 font-semibold text-small">APPROVAL</p>
            <p className="text-h3 font-bold text-orange-600 mt-2">{stats.requires_approval_count}</p>
            <p className="text-micro text-text-secondary mt-1">{(stats.approval_required_rate * 100).toFixed(0)}%</p>
          </CardBody>
        </Card>
      </div>

      {/* Verdict Breakdown Chart */}
      <Card>
        <CardHeader>
          <CardTitle>Verdict Distribution</CardTitle>
        </CardHeader>
        <CardBody>
          <div className="space-y-4">
            {[
              {
                label: 'ALLOW',
                count: stats.allow_count,
                rate: stats.allow_rate,
                color: 'bg-green-500',
              },
              {
                label: 'DENY',
                count: stats.deny_count,
                rate: stats.deny_rate,
                color: 'bg-red-500',
              },
              {
                label: 'STALE_STATE',
                count: stats.stale_state_count,
                rate: stats.stale_rate,
                color: 'bg-yellow-500',
              },
              {
                label: 'REQUIRES_APPROVAL',
                count: stats.requires_approval_count,
                rate: stats.approval_required_rate,
                color: 'bg-orange-500',
              },
            ].map((item, idx) => (
              <div key={idx}>
                <div className="flex items-center justify-between mb-2">
                  <span className="font-semibold text-text-primary">{item.label}</span>
                  <span className="text-small text-text-secondary">
                    {item.count} ({(item.rate * 100).toFixed(1)}%)
                  </span>
                </div>
                <div className="h-2 bg-background rounded-full overflow-hidden">
                  <div className={`h-full ${item.color} transition-all`} style={{ width: `${item.rate * 100}%` }} />
                </div>
              </div>
            ))}
          </div>
        </CardBody>
      </Card>

      {/* Recent Verdicts */}
      <Card>
        <CardHeader>
          <CardTitle>Recent Verdicts</CardTitle>
        </CardHeader>
        <CardBody>
          {verdicts.length > 0 ? (
            <div className="space-y-4">
              {verdicts.map((verdict) => (
                <div
                  key={verdict.verdict_id}
                  className="p-5 rounded-lg bg-background-elevated border-l-4 border-accent hover:shadow-md transition-shadow"
                >
                  <div className="flex items-start justify-between mb-3">
                    <div className="flex items-center gap-3">
                      {getVerdictIcon(verdict.verdict)}
                      <div>
                        <p className="font-mono text-small font-semibold text-text-primary">
                          {verdict.decision_id.substring(0, 12)}...
                        </p>
                        <p className="text-micro text-text-secondary mt-1">
                          {new Date(verdict.timestamp).toLocaleTimeString()}
                        </p>
                      </div>
                    </div>
                    <Badge variant={getVerdictColor(verdict.verdict)} className="font-bold">
                      {verdict.verdict}
                    </Badge>
                  </div>

                  <div className="space-y-2 mb-3">
                    <p className="text-small text-text-secondary">
                      <strong>Action:</strong> {verdict.action_type} on{' '}
                      <span className="font-mono">{verdict.workload_id}</span>
                    </p>
                    <p className="text-small text-text-primary">{verdict.explanation}</p>
                  </div>

                  {/* Rules */}
                  {verdict.rules_passed && verdict.rules_passed.length > 0 && (
                    <div className="mb-2">
                      <p className="text-xs font-semibold text-green-700 mb-1">✅ Rules Passed:</p>
                      <div className="flex flex-wrap gap-2">
                        {verdict.rules_passed.map((rule, idx) => (
                          <Badge key={idx} variant="success">
                            {rule}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}

                  {verdict.rules_failed && verdict.rules_failed.length > 0 && (
                    <div>
                      <p className="text-xs font-semibold text-red-700 mb-1">❌ Rules Failed:</p>
                      <div className="flex flex-wrap gap-2">
                        {verdict.rules_failed.map((rule, idx) => (
                          <Badge key={idx} variant="error">
                            {rule}
                          </Badge>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              ))}
            </div>
          ) : (
            <div className="text-center py-12">
              <Shield className="w-12 h-12 text-text-muted mx-auto mb-4 opacity-20" />
              <p className="text-text-secondary">No verdicts yet</p>
              <p className="text-micro text-text-muted mt-2">
                Policy verdicts will appear as actions are evaluated and executed
              </p>
            </div>
          )}
        </CardBody>
      </Card>

      {/* Policy Rules Reference */}
      <Card>
        <CardHeader>
          <CardTitle>Policy Rules Reference</CardTitle>
        </CardHeader>
        <CardBody className="space-y-3">
          <div className="p-3 bg-blue-50 rounded border-l-2 border-blue-400">
            <p className="font-semibold text-blue-900">RULE_001_UNKNOWN_ACTION</p>
            <p className="text-sm text-blue-800 mt-1">Denies actions not in the whitelisted action types</p>
          </div>
          <div className="p-3 bg-blue-50 rounded border-l-2 border-blue-400">
            <p className="font-semibold text-blue-900">RULE_002_REPLICA_BOUNDS</p>
            <p className="text-sm text-blue-800 mt-1">Ensures replica count stays within min/max constraints</p>
          </div>
          <div className="p-3 bg-blue-50 rounded border-l-2 border-blue-400">
            <p className="font-semibold text-blue-900">RULE_003_STALE_STATE</p>
            <p className="text-sm text-blue-800 mt-1">Rejects actions with mismatched state version (requires replan)</p>
          </div>
        </CardBody>
      </Card>
    </div>
  );
}

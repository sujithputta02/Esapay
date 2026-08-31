import { useQuery } from '@tanstack/react-query';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { Clock, CheckCircle, AlertCircle, Activity, ShieldCheck } from 'lucide-react';

interface Action {
  action_id: string;
  action_type: string;
  workload_id: string;
  status: string;
  timestamp: string;
  outcome: string;
}

export function AuditView() {
  const { data: actionsData } = useQuery({
    queryKey: ['actions'],
    queryFn: async () => {
      const response = await apiClient.getRecentActions();
      return response as { actions: Action[] };
    },
    refetchInterval: 2000,
  });

  // Deduplicate actions by action_id to prevent duplicate keys
  const actions = actionsData?.actions || [];
  const uniqueActions = actions.reduce((acc: Action[], current) => {
    const exists = acc.find((item) => item.action_id === current.action_id);
    if (!exists) {
      acc.push(current);
    }
    return acc;
  }, []);

  const getStatusIcon = (status: string) => {
    switch (status.toLowerCase()) {
      case 'completed':
        return <CheckCircle className="w-5 h-5 text-accent" />;
      case 'failed':
        return <AlertCircle className="w-5 h-5 text-error" />;
      case 'in_progress':
        return <Activity className="w-5 h-5 text-accent animate-pulse" />;
      default:
        return <Clock className="w-5 h-5 text-text-secondary" />;
    }
  };

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-[28px] font-bold text-white tracking-tight">Audit Trail & Lineage</h1>
        <p className="text-[15px] text-[#B8B8B8] mt-1">
          Complete decision lineage, policy verification proofs, and Kubernetes runtime mutations.
        </p>
      </div>

      <div className="bg-[#272727] rounded-[32px] p-7 sm:p-9 border border-white/[0.04] space-y-6">
        <div className="flex items-center justify-between">
          <h3 className="text-[20px] font-bold text-white">Action Execution Timeline</h3>
          <span className="text-xs font-mono text-[#777777]">{uniqueActions.length} Recorded Events</span>
        </div>

        {uniqueActions.length > 0 ? (
          <div className="space-y-4">
            {uniqueActions.map((action, index) => (
              <div
                key={`${action.action_id}-${index}`}
                className="p-6 rounded-[22px] bg-[#333333] border border-white/[0.03] space-y-3"
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    {getStatusIcon(action.status)}
                    <div>
                      <h4 className="font-mono text-sm font-bold text-white">
                        {action.action_id}
                      </h4>
                      <p className="text-xs text-[#777777] mt-0.5">
                        {new Date(action.timestamp).toLocaleString()}
                      </p>
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <Badge variant={action.status === 'completed' ? 'success' : 'default'}>
                      {action.status.toUpperCase()}
                    </Badge>
                    <Badge variant="charcoal">{action.action_type}</Badge>
                  </div>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-xs bg-[#1D1E1C] p-4 rounded-[16px] border border-white/[0.04]">
                  <div>
                    <span className="text-[#777777]">Target Workload:</span>
                    <span className="text-white ml-2 font-mono font-semibold">{action.workload_id}</span>
                  </div>
                  <div>
                    <span className="text-[#777777]">Execution Outcome:</span>
                    <span className="text-accent ml-2 font-medium">{action.outcome}</span>
                  </div>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="text-center py-16 text-[#777777] space-y-2">
            <ShieldCheck className="w-12 h-12 text-[#777777] mx-auto mb-2 opacity-20" />
            <p className="text-[15px] font-medium text-white">No actions executed yet</p>
            <p className="text-xs text-[#777777]">
              Trigger a traffic spike or send test payments to see autonomous Kubernetes scaling actions
            </p>
          </div>
        )}
      </div>
    </div>
  );
}


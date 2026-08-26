import { useQuery } from '@tanstack/react-query';
import { Card, CardHeader, CardTitle, CardBody } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { Clock, CheckCircle, AlertCircle, Activity } from 'lucide-react';

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
    const exists = acc.find(item => item.action_id === current.action_id);
    if (!exists) {
      acc.push(current);
    }
    return acc;
  }, []);

  const getStatusIcon = (status: string) => {
    switch (status.toLowerCase()) {
      case 'completed':
        return <CheckCircle className="w-5 h-5 text-success" />;
      case 'failed':
        return <AlertCircle className="w-5 h-5 text-error" />;
      case 'in_progress':
        return <Activity className="w-5 h-5 text-info animate-pulse" />;
      default:
        return <Clock className="w-5 h-5 text-text-secondary" />;
    }
  };

  return (
    <div className="p-8 space-y-8">
      <div>
        <h1 className="text-h1 font-bold text-text-primary">Audit Trail</h1>
        <p className="text-body text-text-secondary mt-2">
          Complete decision lineage and action history.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Action Timeline</CardTitle>
        </CardHeader>
        <CardBody>
          {uniqueActions.length > 0 ? (
            <div className="space-y-4">
              {uniqueActions.map((action, index) => (
                <div
                  key={`${action.action_id}-${index}`}
                  className="p-6 rounded-lg bg-background-elevated border-l-4 border-accent"
                >
                  <div className="flex items-start justify-between mb-4">
                    <div className="flex items-center gap-3">
                      {getStatusIcon(action.status)}
                      <div>
                        <h4 className="font-mono text-small font-semibold text-text-primary">
                          {action.action_id}
                        </h4>
                        <p className="text-micro text-text-secondary mt-1">
                          {new Date(action.timestamp).toLocaleString()}
                        </p>
                      </div>
                    </div>
                    <div className="flex gap-2">
                      <Badge variant={action.status === 'completed' ? 'success' : 'default'}>
                        {action.status}
                      </Badge>
                      <Badge variant="info">{action.action_type}</Badge>
                    </div>
                  </div>

                  <div className="space-y-2 text-small">
                    <div>
                      <span className="text-text-secondary">Workload:</span>
                      <span className="text-text-primary ml-2 font-mono">{action.workload_id}</span>
                    </div>
                    <div>
                      <span className="text-text-secondary">Outcome:</span>
                      <p className="text-text-primary mt-1">{action.outcome}</p>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-center py-12">
              <Activity className="w-12 h-12 text-text-muted mx-auto mb-4 opacity-20" />
              <p className="text-text-secondary">No actions executed yet</p>
              <p className="text-micro text-text-muted mt-2">
                Trigger a traffic spike from the Payment Simulator to see ESA in action
              </p>
            </div>
          )}
        </CardBody>
      </Card>
    </div>
  );
}

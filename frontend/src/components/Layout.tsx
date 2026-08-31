import { Outlet, Link, useLocation } from 'react-router-dom';
import { cn } from '@/lib/utils';
import { useWebSocket } from '@/hooks/useWebSocket';
import { useEsaStore } from '@/lib/store';
import { queryClient } from '@/lib/queryClient';
import { useQuery } from '@tanstack/react-query';
import { apiClient } from '@/lib/api';
import type { TelemetryMessage } from '@/types';

const navigation = [
  { name: 'Dashboard', path: '/dashboard' },
  { name: 'Runtime', path: '/runtime' },
  { name: 'Agents', path: '/agents' },
  { name: 'Audit', path: '/audit' },
  { name: 'Effects', path: '/effects' },
  { name: 'Costs', path: '/costs' },
  { name: 'Policy', path: '/policy' },
];

function invalidateLiveQueries(type?: string) {
  const all = [
    ['actions'],
    ['audit'],
    ['effects'],
    ['verdicts'],
    ['verdict-stats'],
    ['agents'],
    ['ai-thinking'],
    ['ai-costs'],
    ['costs-per-agent'],
    ['workloads'],
    ['vitals'],
  ];

  if (!type) {
    all.forEach((key) => queryClient.invalidateQueries({ queryKey: key }));
    return;
  }

  switch (type) {
    case 'action_executed':
    case 'action_proposed':
    case 'policy_decision':
      queryClient.invalidateQueries({ queryKey: ['actions'] });
      queryClient.invalidateQueries({ queryKey: ['audit'] });
      queryClient.invalidateQueries({ queryKey: ['effects'] });
      queryClient.invalidateQueries({ queryKey: ['verdicts'] });
      queryClient.invalidateQueries({ queryKey: ['verdict-stats'] });
      break;
    case 'agent_activity':
      queryClient.invalidateQueries({ queryKey: ['agents'] });
      queryClient.invalidateQueries({ queryKey: ['ai-thinking'] });
      queryClient.invalidateQueries({ queryKey: ['ai-costs'] });
      queryClient.invalidateQueries({ queryKey: ['costs-per-agent'] });
      break;
    case 'workload_update':
    case 'condition_detected':
      queryClient.invalidateQueries({ queryKey: ['workloads'] });
      queryClient.invalidateQueries({ queryKey: ['agents'] });
      queryClient.invalidateQueries({ queryKey: ['vitals'] });
      break;
    case 'vitals_update':
      queryClient.invalidateQueries({ queryKey: ['vitals'] });
      break;
    default:
      invalidateLiveQueries();
  }
}

export function Layout() {
  const location = useLocation();
  const { updateWorkload, appendVitals, updateAgentStatus, addCondition, addExecution } = useEsaStore();

  const { data: workloads } = useQuery({
    queryKey: ['workloads'],
    queryFn: () => apiClient.getWorkloads(),
    refetchInterval: 3000,
  });

  const totalPods =
    workloads?.reduce(
      (sum, w) => sum + (w.replication?.current_replicas || 2),
      0
    ) ?? 18;

  const handleTelemetryMessage = (message: TelemetryMessage) => {
    invalidateLiveQueries(message.type);

    switch (message.type) {
      case 'vitals_update':
        appendVitals({
          timestamp: message.timestamp as string,
          total_tps: message.total_tps as number,
          avg_p95_ms: message.avg_p95_ms as number,
          avg_error_rate: message.avg_error_rate as number,
          total_queue: message.total_queue as number,
          healthy_count: message.healthy_count as number,
          degraded_count: message.degraded_count as number,
        });
        break;
      case 'workload_update':
        if (message.workload_id) {
          updateWorkload({
            workload_id: message.workload_id,
            state: (message.state || 'HEALTHY') as any,
            metrics: (message.metrics || {}) as any,
            shard_id: '',
            region: 'IN-SOUTH',
            replication: {
              min_replicas: 2,
              max_replicas: 10,
              current_replicas: (message.metrics as any)?.current_replicas || 2,
              consistency_mode: 'STRONG',
            },
            locality: { preferred_region: 'IN-SOUTH', fallback_regions: [] },
            lifecycle: 'ACTIVE',
            version: 1,
            updated_at: new Date().toISOString(),
          });
        }
        break;
      case 'agent_activity':
        if (message.agent_id) {
          updateAgentStatus(message.agent_id, {
            agent_id: message.agent_id as any,
            latest_observation: message.activity,
            status: 'ACTING',
          });
        }
        break;
      case 'condition_detected':
        if (message.condition_type) {
          addCondition({
            condition_type: message.condition_type as any,
            workload_id: message.workload_id || 'payment-service',
            severity: (message.severity || 'MEDIUM') as any,
            description: message.description || '',
            metrics: {},
          });
        }
        break;
      case 'action_executed':
        if (message.execution_id) {
          addExecution({
            execution_id: message.execution_id,
            proposal_id: message.proposal_id || message.execution_id,
            action: {
              action: 'CREATE_REPLICA',
              workload_id: 'payment-service',
              target_region: 'IN-SOUTH',
              reason: 'Auto recovery scaling',
              confidence: 0.95,
              risk: 'LOW',
              state_version: 1,
            },
            executed_at: new Date().toISOString(),
            completed_at: new Date().toISOString(),
            outcome: (message.outcome || 'SUCCESS') as any,
            before_metrics: {},
            after_metrics: {},
            error_message: null,
          });
        }
        break;
      default:
        break;
    }
  };

  const { isConnected } = useWebSocket(handleTelemetryMessage);

  return (
    <div className="min-h-screen bg-[#1D1E1C] text-text-primary selection:bg-accent/30 selection:text-white">
      {/* Top Header - Explicit 3-Zone Architecture */}
      <header className="w-full border-b border-white/[0.04]">
        <div className="w-[min(100%-48px,1952px)] mx-auto h-24 px-4 sm:px-6 md:px-12 flex items-center justify-between">
          {/* Zone 1: Logo (Left) */}
          <div className="flex items-center gap-4">
            <Link
              to="/dashboard"
              className="flex items-center justify-center border-2 border-white/90 rounded-[14px] px-3.5 py-1.5 hover:border-accent transition-colors"
            >
              <span className="font-extrabold tracking-wider text-lg text-white">
                ESA.
              </span>
            </Link>
            <div className="hidden lg:flex flex-col">
              <span className="text-[11px] font-semibold text-text-secondary tracking-wide uppercase">
                Payment Gateway
              </span>
              <span className="text-[10px] text-text-muted">
                Kubernetes Pod Autoscaling
              </span>
            </div>
          </div>

          {/* Zone 2: Navigation (Center) */}
          <nav className="hidden md:flex items-center gap-7 lg:gap-10">
            {navigation.map((item) => {
              const isActive =
                location.pathname === item.path ||
                (item.path === '/dashboard' && location.pathname === '/');

              return (
                <Link
                  key={item.path}
                  to={item.path}
                  className={cn(
                    'text-[15px] font-medium transition-colors duration-150 relative py-1',
                    isActive
                      ? 'text-white font-semibold after:absolute after:bottom-0 after:left-0 after:right-0 after:h-[2px] after:bg-white/80'
                      : 'text-text-secondary hover:text-white'
                  )}
                >
                  {item.name}
                </Link>
              );
            })}
          </nav>

          {/* Zone 3: Actions & Kubernetes Pods Badge (Right) */}
          <div className="flex items-center gap-3 sm:gap-4">
            {/* Kubernetes Pods Counter Badge */}
            <div className="hidden sm:flex items-center gap-2 px-3.5 py-2 rounded-full bg-[#272727] border border-white/[0.06] text-xs">
              <div className="w-2 h-2 rounded-full bg-accent animate-pulse" />
              <span className="text-text-secondary">K8s Pods:</span>
              <span className="font-bold text-white font-mono">
                {totalPods} Running
              </span>
            </div>

            {/* Connection Status */}
            <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-[#272727] border border-white/[0.06] text-xs">
              <div
                className={cn(
                  'w-2 h-2 rounded-full',
                  isConnected ? 'bg-accent' : 'bg-error'
                )}
              />
              <span className="text-text-muted text-[11px]">
                {isConnected ? 'Live' : 'Offline'}
              </span>
            </div>

            {/* Quick Action Pill Button (JOIN NOW / TRIGGER SPIKE style) */}
            <button
              onClick={async () => {
                if (workloads && workloads.length > 0) {
                  await apiClient.triggerSpike(workloads[0].workload_id, 3.0);
                }
              }}
              className="h-11 px-5 rounded-full bg-[#474745] hover:bg-[#5A5A58] text-white text-xs font-bold transition-all active:translate-y-[1px]"
            >
              TRIGGER SPIKE
            </button>
          </div>
        </div>
      </header>

      {/* Mobile navigation bar */}
      <div className="md:hidden flex items-center justify-around border-b border-white/[0.04] bg-[#272727] px-2 py-2 overflow-x-auto">
        {navigation.map((item) => {
          const isActive =
            location.pathname === item.path ||
            (item.path === '/dashboard' && location.pathname === '/');
          return (
            <Link
              key={item.path}
              to={item.path}
              className={cn(
                'px-3 py-1.5 rounded-full text-xs whitespace-nowrap',
                isActive
                  ? 'bg-[#4B4B4B] text-white font-semibold'
                  : 'text-text-secondary hover:text-white'
              )}
            >
              {item.name}
            </Link>
          );
        })}
      </div>

      {/* Main Content with generous outer breathing room */}
      <main className="w-[min(100%-48px,1952px)] mx-auto px-2 sm:px-4 md:px-12 py-8 md:py-10">
        <Outlet />
      </main>
    </div>
  );
}


import { Outlet, Link, useLocation } from 'react-router-dom';
import { Activity, GitBranch, Users, FileText, TrendingUp, DollarSign, Shield } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useWebSocket } from '@/hooks/useWebSocket';
import { useEsaStore } from '@/lib/store';
import { queryClient } from '@/lib/queryClient';
import type { TelemetryMessage } from '@/types';

const navigation = [
  { name: 'Command Center', path: '/dashboard', icon: Activity },
  { name: 'Runtime', path: '/runtime', icon: GitBranch },
  { name: 'Agents', path: '/agents', icon: Users },
  { name: 'Audit', path: '/audit', icon: FileText },
  { name: 'Effects', path: '/effects', icon: TrendingUp },
  { name: 'Costs', path: '/costs', icon: DollarSign },
  { name: 'Policy', path: '/policy', icon: Shield },
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
  const { updateWorkload, appendVitals } = useEsaStore();

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
        if (message.workload_id && message.metrics) {
          updateWorkload({
            workload_id: message.workload_id,
            state: message.state as any,
            metrics: message.metrics as any,
            shard_id: '',
            region: 'IN-SOUTH',
            replication: { min_replicas: 2, max_replicas: 10, current_replicas: 2, consistency_mode: 'STRONG' },
            locality: { preferred_region: 'IN-SOUTH', fallback_regions: [] },
            lifecycle: 'ACTIVE',
            version: 1,
            updated_at: new Date().toISOString(),
          });
        }
        break;
      default:
        break;
    }
  };

  const { isConnected } = useWebSocket(handleTelemetryMessage);

  return (
    <div className="flex h-screen bg-background">
      {/* Sidebar */}
      <aside className="w-64 bg-background-elevated border-r border-border flex flex-col">
        <div className="p-6 border-b border-border">
          <h1 className="text-h3 font-bold text-gradient">ESA</h1>
          <p className="text-small text-text-secondary mt-1">Executable State Architecture</p>
        </div>

        <nav className="flex-1 p-4 space-y-1">
          {navigation.map((item) => {
            const Icon = item.icon;
            const isActive = location.pathname === item.path;

            return (
              <Link
                key={item.path}
                to={item.path}
                className={cn(
                  'flex items-center gap-3 px-3 py-2 rounded-md text-small font-medium transition-colors',
                  isActive
                    ? 'bg-accent text-background'
                    : 'text-text-secondary hover:bg-background-card hover:text-text-primary'
                )}
              >
                <Icon className="w-5 h-5" />
                {item.name}
              </Link>
            );
          })}
        </nav>

        <div className="p-4 border-t border-border">
          <div className="flex items-center gap-2 text-micro">
            <div
              className={cn(
                'w-2 h-2 rounded-full',
                isConnected ? 'bg-success animate-pulse' : 'bg-error'
              )}
            />
            <span className="text-text-secondary">
              {isConnected ? 'Connected' : 'Disconnected'}
            </span>
          </div>
        </div>
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-auto">
        <Outlet />
      </main>
    </div>
  );
}

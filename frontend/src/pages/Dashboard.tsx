import { useQuery } from '@tanstack/react-query';
import { motion } from 'framer-motion';
import { Activity, Zap, AlertCircle, TrendingUp } from 'lucide-react';
import { Card, CardHeader, CardBody, CardTitle } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { Button } from '@/components/ui/Button';
import { apiClient } from '@/lib/api';
import { VitalsChart } from '@/components/VitalsChart';
import { formatLatency, formatThroughput } from '@/lib/utils';

export function Dashboard() {
  const { data: workloads } = useQuery({
    queryKey: ['workloads'],
    queryFn: () => apiClient.getWorkloads(),
    refetchInterval: 5000,
  });

  const { data: tokenStats } = useQuery({
    queryKey: ['tokenStats'],
    queryFn: () => apiClient.getTokenMetrics(),
    refetchInterval: 10000,
  });

  const handleTriggerSpike = async () => {
    if (workloads && workloads.length > 0) {
      await apiClient.triggerSpike(workloads[0].workload_id, 3.0);
    }
  };

  const healthyWorkloads = workloads?.filter((w) => w.state === 'HEALTHY').length ?? 0;
  const totalWorkloads = workloads?.length ?? 0;

  return (
    <div className="p-8 space-y-8">
      {/* Header */}
      <div>
        <h1 className="text-display font-bold text-text-primary">Command Center</h1>
        <p className="text-body text-text-secondary mt-2">
          Live infrastructure vitals, workloads, and autonomous recovery status.
        </p>
      </div>

      <VitalsChart />

      {/* Status Overview */}
      <motion.div
        initial={{ opacity: 0, y: 20 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.3 }}
        className="grid grid-cols-1 md:grid-cols-4 gap-6"
      >
        <Card className="p-6">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-small text-text-secondary">Runtime Status</p>
              <p className="text-h2 font-bold text-success mt-2">HEALTHY</p>
            </div>
            <Activity className="w-10 h-10 text-success opacity-20" />
          </div>
          <p className="text-micro text-text-secondary mt-4">
            99.98% availability
          </p>
        </Card>

        <Card className="p-6">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-small text-text-secondary">Workloads</p>
              <p className="text-h2 font-bold text-text-primary mt-2">
                {healthyWorkloads}/{totalWorkloads}
              </p>
            </div>
            <Zap className="w-10 h-10 text-accent opacity-20" />
          </div>
          <p className="text-micro text-text-secondary mt-4">
            Active workloads
          </p>
        </Card>

        <Card className="p-6">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-small text-text-secondary">P95 Latency</p>
              <p className="text-h2 font-bold text-text-primary mt-2">
                {workloads?.[0]?.metrics?.p95_latency_ms ? formatLatency(workloads[0].metrics.p95_latency_ms) : '—'}
              </p>
            </div>
            <TrendingUp className="w-10 h-10 text-info opacity-20" />
          </div>
          <p className="text-micro text-text-secondary mt-4">
            Last 5 minutes
          </p>
        </Card>

        <Card className="p-6">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-small text-text-secondary">Incidents</p>
              <p className="text-h2 font-bold text-text-primary mt-2">0</p>
            </div>
            <AlertCircle className="w-10 h-10 text-text-muted opacity-20" />
          </div>
          <p className="text-micro text-text-secondary mt-4">
            Last 24 hours
          </p>
        </Card>
      </motion.div>

      {/* Workloads */}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Active Workloads</CardTitle>
          <Button onClick={handleTriggerSpike} size="sm">
            Trigger Spike
          </Button>
        </CardHeader>
        <CardBody>
          <div className="space-y-4">
            {workloads?.map((workload) => (
              <div
                key={workload.workload_id}
                className="flex items-center justify-between p-4 rounded-md bg-background-elevated"
              >
                <div>
                  <div className="flex items-center gap-3">
                    <span className="font-mono text-small text-text-primary">
                      {workload.workload_id}
                    </span>
                    <Badge variant={workload.state === 'HEALTHY' ? 'success' : 'warning'}>
                      {workload.state}
                    </Badge>
                    <Badge variant="default">{workload.region}</Badge>
                  </div>
                  <div className="flex items-center gap-6 mt-2 text-micro text-text-secondary">
                    <span>{formatThroughput(workload.metrics.rate_per_min)}</span>
                    <span>P95: {formatLatency(workload.metrics.p95_latency_ms)}</span>
                    <span>Queue: {workload.metrics.queue_depth}</span>
                    <span>Replicas: {workload.replication.current_replicas}</span>
                  </div>
                </div>
              </div>
            ))}
            {(!workloads || workloads.length === 0) && (
              <p className="text-center text-text-secondary py-8">
                No workloads active
              </p>
            )}
          </div>
        </CardBody>
      </Card>

      {/* Token Usage */}
      {tokenStats && tokenStats.total_requests > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>AI Token Usage (Real-time)</CardTitle>
          </CardHeader>
          <CardBody>
            <div className="grid grid-cols-3 gap-6">
              <div>
                <p className="text-small text-text-secondary">Total Requests</p>
                <p className="text-h3 font-bold text-text-primary mt-1">
                  {tokenStats.total_requests}
                </p>
              </div>
              <div>
                <p className="text-small text-text-secondary">Input Tokens</p>
                <p className="text-h3 font-bold text-text-primary mt-1">
                  {tokenStats.total_input_tokens.toLocaleString()}
                </p>
              </div>
              <div>
                <p className="text-small text-text-secondary">Output Tokens</p>
                <p className="text-h3 font-bold text-text-primary mt-1">
                  {tokenStats.total_output_tokens.toLocaleString()}
                </p>
              </div>
            </div>
            <p className="text-micro text-text-secondary mt-4">
              Token usage from Ollama AI model inference
            </p>
          </CardBody>
        </Card>
      )}
    </div>
  );
}

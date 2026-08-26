import { useEffect, useMemo } from 'react';
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
} from 'recharts';
import { useQuery } from '@tanstack/react-query';
import { Activity, Gauge, Layers, AlertTriangle, HeartPulse } from 'lucide-react';
import { apiClient } from '@/lib/api';
import { useEsaStore } from '@/lib/store';
import type { VitalsSnapshot } from '@/types';
import { Card, CardBody, CardHeader, CardTitle } from '@/components/ui/Card';

function formatTime(ts: string) {
  try {
    return new Date(ts).toLocaleTimeString([], {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch {
    return ts;
  }
}

function VitalTile({
  label,
  value,
  sub,
  icon: Icon,
  accent,
}: {
  label: string;
  value: string;
  sub?: string;
  icon: typeof Activity;
  accent: string;
}) {
  return (
    <div className="rounded-md bg-background-elevated border border-border p-4">
      <div className="flex items-center justify-between gap-2">
        <p className="text-micro text-text-secondary uppercase tracking-wide">{label}</p>
        <Icon className={`w-4 h-4 shrink-0 ${accent}`} />
      </div>
      <p className="text-h3 font-bold text-text-primary mt-2 font-mono">{value}</p>
      {sub && <p className="text-micro text-text-secondary mt-1">{sub}</p>}
    </div>
  );
}

export function VitalsChart() {
  const { vitalsHistory, setVitalsHistory } = useEsaStore();

  const { data: initial, isLoading, isError, refetch } = useQuery({
    queryKey: ['vitals'],
    queryFn: () => apiClient.getVitalsHistory(),
    refetchInterval: 5000,
  });

  useEffect(() => {
    if (initial?.snapshots?.length) {
      setVitalsHistory(initial.snapshots);
    }
  }, [initial, setVitalsHistory]);

  const snapshots: VitalsSnapshot[] = useMemo(() => {
    if (vitalsHistory.length > 0) return vitalsHistory;
    return initial?.snapshots ?? [];
  }, [vitalsHistory, initial?.snapshots]);

  const latest: VitalsSnapshot | undefined =
    snapshots[snapshots.length - 1] ?? initial?.latest ?? undefined;

  const chartData = useMemo(
    () =>
      snapshots.map((s) => ({
        time: formatTime(s.timestamp),
        tps: Math.round(s.total_tps),
        p95: Math.round(s.avg_p95_ms),
        queue: s.total_queue,
        errors: Number((s.avg_error_rate * 100).toFixed(2)),
      })),
    [snapshots]
  );

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between gap-4">
        <div>
          <CardTitle>Live Infrastructure Vitals</CardTitle>
          <p className="text-micro text-text-secondary mt-1">
            Real-time payment gateway telemetry — updates every 2s via WebSocket
          </p>
        </div>
        {latest && (
          <span className="text-micro text-text-secondary font-mono shrink-0">
            Last sample {formatTime(latest.timestamp)}
          </span>
        )}
      </CardHeader>
      <CardBody className="space-y-6">
        {isLoading && !latest ? (
          <p className="text-center text-text-secondary py-8 text-small">Loading vitals…</p>
        ) : isError && !latest ? (
          <div className="text-center py-8 space-y-3">
            <p className="text-small text-error">Could not load vitals from API</p>
            <button
              type="button"
              onClick={() => refetch()}
              className="text-small text-accent hover:underline"
            >
              Retry
            </button>
          </div>
        ) : latest ? (
          <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
            <VitalTile
              label="Throughput"
              value={`${Math.round(latest.total_tps)} TPS`}
              sub="Aggregate across workloads"
              icon={Activity}
              accent="text-info"
            />
            <VitalTile
              label="P95 Latency"
              value={`${Math.round(latest.avg_p95_ms)} ms`}
              sub="Average across regions"
              icon={Gauge}
              accent="text-warning"
            />
            <VitalTile
              label="Queue Depth"
              value={latest.total_queue.toLocaleString()}
              sub="Pending transactions"
              icon={Layers}
              accent="text-accent"
            />
            <VitalTile
              label="Error Rate"
              value={`${(latest.avg_error_rate * 100).toFixed(2)}%`}
              sub="Rolling average"
              icon={AlertTriangle}
              accent="text-error"
            />
            <VitalTile
              label="Workload Health"
              value={`${latest.healthy_count} / ${latest.healthy_count + latest.degraded_count}`}
              sub={`${latest.degraded_count} degraded`}
              icon={HeartPulse}
              accent="text-success"
            />
          </div>
        ) : (
          <p className="text-center text-text-secondary py-4 text-small">
            Waiting for first vitals sample… ensure the API is running on port 8080.
          </p>
        )}

        {chartData.length >= 1 ? (
          <div className="h-72 w-full min-h-[18rem]">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={chartData} margin={{ top: 8, right: 16, left: 0, bottom: 0 }}>
                <CartesianGrid strokeDasharray="3 3" stroke="#2a2a35" />
                <XAxis dataKey="time" tick={{ fill: '#888', fontSize: 11 }} minTickGap={24} />
                <YAxis yAxisId="left" tick={{ fill: '#888', fontSize: 11 }} />
                <YAxis yAxisId="right" orientation="right" tick={{ fill: '#888', fontSize: 11 }} />
                <Tooltip
                  contentStyle={{
                    background: '#1a1a24',
                    border: '1px solid #333',
                    borderRadius: 8,
                  }}
                />
                <Legend />
                <Line
                  yAxisId="left"
                  type="monotone"
                  dataKey="tps"
                  name="TPS"
                  stroke="#3b82f6"
                  strokeWidth={2}
                  dot={false}
                  isAnimationActive={false}
                />
                <Line
                  yAxisId="right"
                  type="monotone"
                  dataKey="p95"
                  name="P95 (ms)"
                  stroke="#f59e0b"
                  strokeWidth={2}
                  dot={false}
                  isAnimationActive={false}
                />
                <Line
                  yAxisId="right"
                  type="monotone"
                  dataKey="queue"
                  name="Queue"
                  stroke="#a855f7"
                  strokeWidth={2}
                  dot={false}
                  isAnimationActive={false}
                />
                <Line
                  yAxisId="left"
                  type="monotone"
                  dataKey="errors"
                  name="Error %"
                  stroke="#ef4444"
                  strokeWidth={2}
                  dot={false}
                  isAnimationActive={false}
                />
              </LineChart>
            </ResponsiveContainer>
          </div>
        ) : (
          <p className="text-center text-text-secondary py-8 text-small border border-dashed border-border rounded-md">
            Graph will appear after the API records a few samples (about 4 seconds with active workloads).
          </p>
        )}
      </CardBody>
    </Card>
  );
}

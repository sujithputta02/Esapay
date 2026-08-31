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
import { Activity, Gauge, Layers, AlertTriangle, HeartPulse, Server } from 'lucide-react';
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
    <div className="rounded-[22px] bg-[#333333] p-5 min-h-[110px] flex flex-col justify-between border border-white/[0.03]">
      <div className="flex items-center justify-between gap-2">
        <p className="text-[14px] text-[#B8B8B8] font-medium">{label}</p>
        <Icon className={`w-4 h-4 shrink-0 ${accent}`} />
      </div>
      <div>
        <p className="text-[24px] font-extrabold text-white tracking-tight">{value}</p>
        {sub && <p className="text-[12px] text-[#777777] mt-0.5">{sub}</p>}
      </div>
    </div>
  );
}

export function VitalsChart() {
  const { vitalsHistory, setVitalsHistory } = useEsaStore();

  const { data: initial, isLoading, isError, refetch } = useQuery({
    queryKey: ['vitals'],
    queryFn: () => apiClient.getVitalsHistory(),
    refetchInterval: 3000,
  });

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
    <Card className="rounded-[32px] bg-[#272727] border border-white/[0.04] p-2">
      <CardHeader className="flex flex-row items-center justify-between gap-4 border-b border-white/[0.04] px-7 py-5">
        <div>
          <CardTitle className="text-[20px] font-bold text-white">
            Infrastructure Vitals & Kubernetes Scaling
          </CardTitle>
          <p className="text-[13px] text-[#B8B8B8] mt-1">
            Real-time telemetry stream - updates every 2s via WebSocket
          </p>
        </div>
        {latest && (
          <span className="text-[12px] text-[#777777] font-mono shrink-0 px-3 py-1 bg-[#333333] rounded-full">
            Last sample {formatTime(latest.timestamp)}
          </span>
        )}
      </CardHeader>
      <CardBody className="space-y-6 px-7 py-6">
        {isLoading && !latest ? (
          <p className="text-center text-[#B8B8B8] py-8 text-[14px]">Loading vitals…</p>
        ) : isError && !latest ? (
          <div className="text-center py-8 space-y-3">
            <p className="text-[14px] text-error">Could not load vitals from API</p>
            <button
              type="button"
              onClick={() => refetch()}
              className="text-[14px] text-accent hover:underline"
            >
              Retry
            </button>
          </div>
        ) : latest ? (
          <div className="grid grid-cols-2 md:grid-cols-6 gap-4">
            <VitalTile
              label="K8s Pods"
              value={`${totalPods}`}
              sub="Active cluster pods"
              icon={Server}
              accent="text-accent"
            />
            <VitalTile
              label="Throughput"
              value={`${Math.round(latest.total_tps)} TPS`}
              sub="Across workloads"
              icon={Activity}
              accent="text-white"
            />
            <VitalTile
              label="P95 Latency"
              value={`${Math.round(latest.avg_p95_ms)} ms`}
              sub="Target < 100ms"
              icon={Gauge}
              accent="text-accent"
            />
            <VitalTile
              label="Queue Depth"
              value={latest.total_queue.toLocaleString()}
              sub="Pending requests"
              icon={Layers}
              accent="text-[#B8B8B8]"
            />
            <VitalTile
              label="Error Rate"
              value={`${(latest.avg_error_rate * 100).toFixed(2)}%`}
              sub="Rolling average"
              icon={AlertTriangle}
              accent="text-error"
            />
            <VitalTile
              label="Health"
              value={`${latest.healthy_count} / ${latest.healthy_count + latest.degraded_count}`}
              sub={`${latest.degraded_count} degraded`}
              icon={HeartPulse}
              accent="text-accent"
            />
          </div>
        ) : (
          <p className="text-center text-[#B8B8B8] py-4 text-[14px]">
            Waiting for first vitals sample… ensure the API is running on port 8080.
          </p>
        )}

        {chartData.length >= 1 ? (
          <div className="h-72 w-full min-h-[18rem]">
            <ResponsiveContainer width="100%" height="100%">
              <LineChart data={chartData} margin={{ top: 8, right: 16, left: 0, bottom: 0 }}>
                <CartesianGrid strokeDasharray="6 8" stroke="rgba(255,255,255,0.08)" vertical={false} />
                <XAxis dataKey="time" tick={{ fill: 'rgba(255,255,255,0.40)', fontSize: 11 }} minTickGap={24} axisLine={false} tickLine={false} />
                <YAxis yAxisId="left" tick={{ fill: 'rgba(255,255,255,0.40)', fontSize: 11 }} axisLine={false} tickLine={false} />
                <YAxis yAxisId="right" orientation="right" tick={{ fill: 'rgba(255,255,255,0.40)', fontSize: 11 }} axisLine={false} tickLine={false} />
                <Tooltip
                  contentStyle={{
                    background: '#1D1E1C',
                    border: '1px solid rgba(255,255,255,0.10)',
                    borderRadius: '14px',
                    color: '#F5F5F5',
                    fontSize: '12px',
                  }}
                />
                <Legend wrapperStyle={{ color: '#B8B8B8', fontSize: '12px', paddingTop: '10px' }} />
                <Line
                  yAxisId="left"
                  type="monotone"
                  dataKey="tps"
                  name="TPS"
                  stroke="#C7F25C"
                  strokeWidth={2.5}
                  dot={false}
                  isAnimationActive={false}
                />
                <Line
                  yAxisId="right"
                  type="monotone"
                  dataKey="p95"
                  name="P95 (ms)"
                  stroke="#FFFFFF"
                  strokeWidth={2}
                  dot={false}
                  isAnimationActive={false}
                />
                <Line
                  yAxisId="right"
                  type="monotone"
                  dataKey="queue"
                  name="Queue"
                  stroke="rgba(255,255,255,0.30)"
                  strokeWidth={1.5}
                  dot={false}
                  isAnimationActive={false}
                />
                <Line
                  yAxisId="left"
                  type="monotone"
                  dataKey="errors"
                  name="Error %"
                  stroke="#EF4444"
                  strokeWidth={1.5}
                  dot={false}
                  isAnimationActive={false}
                />
              </LineChart>
            </ResponsiveContainer>
          </div>
        ) : (
          <p className="text-center text-[#777777] py-8 text-[13px] border border-dashed border-white/[0.06] rounded-[20px]">
            Graph will appear after the API records a few samples (about 4 seconds with active workloads).
          </p>
        )}
      </CardBody>
    </Card>
  );
}


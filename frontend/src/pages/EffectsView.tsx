import { useQuery } from '@tanstack/react-query';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { TrendingUp, CheckCircle, AlertCircle, XCircle } from 'lucide-react';

interface EffectMeasurement {
  measurement_id: string;
  action_id: string;
  workload_id: string;
  timestamp: string;
  expected: {
    latency_reduction_pct: number;
    error_reduction_pct: number;
    capacity_increase_pct?: number;
  };
  observed: {
    actual_latency_reduction_pct: number;
    actual_error_reduction_pct: number;
    actual_capacity_increase_pct?: number;
  };
  effectiveness: number;
  status: string;
  deviation_reasons: string[];
}

const getStatusIcon = (status: string) => {
  switch (status.toLowerCase()) {
    case 'successful':
      return <CheckCircle className="w-5 h-5 text-accent" />;
    case 'degraded':
      return <AlertCircle className="w-5 h-5 text-warning" />;
    case 'failed':
      return <XCircle className="w-5 h-5 text-error" />;
    default:
      return <TrendingUp className="w-5 h-5 text-accent" />;
  }
};

export function EffectsView() {
  const { data: measurementsData } = useQuery({
    queryKey: ['effects'],
    queryFn: async () => {
      const response = await apiClient.getEffectMeasurements();
      return response as { measurements: EffectMeasurement[]; avg_effectiveness: number };
    },
    refetchInterval: 2000,
  });

  const measurements = measurementsData?.measurements || [];
  const avgEffectiveness = measurementsData?.avg_effectiveness || 0;

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-[28px] font-bold text-white tracking-tight">Effect Measurements & Verifications</h1>
        <p className="text-[15px] text-[#B8B8B8] mt-1">
          Pre vs. Post action effect measurement with mathematical effectiveness scoring (0.0 - 1.0).
        </p>
      </div>

      {/* 3 Metric Cards Row */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-5">
        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Total Measurements</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-white tracking-tight">
            {measurements.length}
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Average Effectiveness</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-accent tracking-tight">
            {(avgEffectiveness * 100).toFixed(1)}%
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Recovery Success Rate</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-white tracking-tight">
            {measurements.length > 0
              ? `${((measurements.filter((m) => m.status === 'Successful').length / measurements.length) * 100).toFixed(0)}%`
              : '100%'}
          </span>
        </div>
      </div>

      {/* Measurements List */}
      <div className="bg-[#272727] rounded-[32px] p-7 sm:p-9 border border-white/[0.04] space-y-6">
        <div className="flex items-center justify-between">
          <h3 className="text-[20px] font-bold text-white">Effect Measurements Timeline</h3>
          <span className="text-xs font-mono text-[#777777]">{measurements.length} Records</span>
        </div>

        {measurements.length > 0 ? (
          <div className="space-y-5">
            {measurements.map((measurement) => (
              <div
                key={measurement.measurement_id}
                className="p-6 rounded-[22px] bg-[#333333] border border-white/[0.03] space-y-4"
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    {getStatusIcon(measurement.status)}
                    <div>
                      <h4 className="font-mono text-sm font-bold text-white">
                        {measurement.action_id}
                      </h4>
                      <p className="text-xs text-[#777777] mt-0.5">
                        {new Date(measurement.timestamp).toLocaleString()}
                      </p>
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <Badge variant={measurement.status === 'Successful' ? 'success' : 'default'}>
                      {measurement.status}
                    </Badge>
                    <Badge variant="accent">
                      {(measurement.effectiveness * 100).toFixed(1)}% Effective
                    </Badge>
                  </div>
                </div>

                {/* Expected vs Observed Comparison */}
                <div className="grid grid-cols-1 md:grid-cols-2 gap-4 text-xs">
                  <div className="p-4 bg-[#1D1E1C] rounded-[16px] border border-white/[0.04] space-y-1.5">
                    <p className="font-semibold text-accent uppercase tracking-wide">Expected Effects</p>
                    <ul className="text-[#B8B8B8] space-y-1 font-mono">
                      <li>P95 Latency: -{measurement.expected.latency_reduction_pct.toFixed(1)}%</li>
                      <li>Error Rate: -{measurement.expected.error_reduction_pct.toFixed(1)}%</li>
                      {measurement.expected.capacity_increase_pct && (
                        <li>Kubernetes Capacity: +{measurement.expected.capacity_increase_pct.toFixed(1)}%</li>
                      )}
                    </ul>
                  </div>

                  <div className="p-4 bg-[#1D1E1C] rounded-[16px] border border-white/[0.04] space-y-1.5">
                    <p className="font-semibold text-white uppercase tracking-wide">Observed Effects</p>
                    <ul className="text-white space-y-1 font-mono">
                      <li>P95 Latency: -{measurement.observed.actual_latency_reduction_pct.toFixed(1)}%</li>
                      <li>Error Rate: -{measurement.observed.actual_error_reduction_pct.toFixed(1)}%</li>
                      {measurement.observed.actual_capacity_increase_pct && (
                        <li>Kubernetes Capacity: +{measurement.observed.actual_capacity_increase_pct.toFixed(1)}%</li>
                      )}
                    </ul>
                  </div>
                </div>

                {/* Deviation Reasons */}
                {measurement.deviation_reasons.length > 0 && (
                  <div className="p-3 bg-[#1D1E1C] rounded-[14px] border border-warning/20 text-xs text-warning">
                    <p className="font-semibold mb-1">Deviation Reasons:</p>
                    <ul className="space-y-0.5">
                      {measurement.deviation_reasons.map((reason, idx) => (
                        <li key={idx}>• {reason}</li>
                      ))}
                    </ul>
                  </div>
                )}

                <div className="text-xs text-[#777777]">
                  Workload: <span className="font-mono text-white">{measurement.workload_id}</span>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="text-center py-16 text-[#777777] space-y-2">
            <TrendingUp className="w-12 h-12 text-[#777777] mx-auto mb-2 opacity-20" />
            <p className="text-[15px] font-medium text-white">No effect measurements recorded yet</p>
            <p className="text-xs text-[#777777]">
              Execute actions to see expected vs. observed effect comparisons
            </p>
          </div>
        )}
      </div>
    </div>
  );
}


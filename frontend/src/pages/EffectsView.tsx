import { useQuery } from '@tanstack/react-query';
import { Card, CardHeader, CardTitle, CardBody } from '@/components/ui/Card';
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
      return <CheckCircle className="w-5 h-5 text-green-500" />;
    case 'degraded':
      return <AlertCircle className="w-5 h-5 text-yellow-500" />;
    case 'failed':
      return <XCircle className="w-5 h-5 text-red-500" />;
    default:
      return <TrendingUp className="w-5 h-5 text-blue-500" />;
  }
};

const getEffectivenessColor = (effectiveness: number) => {
  if (effectiveness >= 0.95) return 'bg-green-100 text-green-700';
  if (effectiveness >= 0.75) return 'bg-blue-100 text-blue-700';
  if (effectiveness >= 0.5) return 'bg-yellow-100 text-yellow-700';
  return 'bg-red-100 text-red-700';
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
    <div className="p-8 space-y-8">
      <div>
        <h1 className="text-h1 font-bold text-text-primary">Effect Measurements</h1>
        <p className="text-body text-text-secondary mt-2">
          Expected vs. Observed effect analysis with effectiveness scoring (0.0 - 1.0)
        </p>
      </div>

      {/* Summary Card */}
      <Card>
        <CardBody>
          <div className="grid grid-cols-3 gap-6">
            <div className="text-center">
              <p className="text-text-secondary text-small">Total Measurements</p>
              <p className="text-h2 font-bold text-text-primary mt-2">{measurements.length}</p>
            </div>
            <div className="text-center">
              <p className="text-text-secondary text-small">Average Effectiveness</p>
              <div className={`text-h2 font-bold mt-2 px-3 py-1 rounded inline-block ${getEffectivenessColor(avgEffectiveness)}`}>
                {(avgEffectiveness * 100).toFixed(1)}%
              </div>
            </div>
            <div className="text-center">
              <p className="text-text-secondary text-small">Success Rate</p>
              <p className="text-h2 font-bold text-text-primary mt-2">
                {measurements.length > 0
                  ? ((measurements.filter(m => m.status === 'Successful').length / measurements.length) * 100).toFixed(0)
                  : 0}
                %
              </p>
            </div>
          </div>
        </CardBody>
      </Card>

      {/* Measurements List */}
      <Card>
        <CardHeader>
          <CardTitle>Effect Measurements Timeline</CardTitle>
        </CardHeader>
        <CardBody>
          {measurements.length > 0 ? (
            <div className="space-y-6">
              {measurements.map((measurement) => (
                <div
                  key={measurement.measurement_id}
                  className="p-6 rounded-lg bg-background-elevated border-l-4 border-accent space-y-4"
                >
                  <div className="flex items-start justify-between">
                    <div className="flex items-center gap-3">
                      {getStatusIcon(measurement.status)}
                      <div>
                        <h4 className="font-mono text-small font-semibold text-text-primary">
                          {measurement.action_id}
                        </h4>
                        <p className="text-micro text-text-secondary mt-1">
                          {new Date(measurement.timestamp).toLocaleString()}
                        </p>
                      </div>
                    </div>
                    <div className="flex gap-2">
                      <Badge variant={measurement.status === 'Successful' ? 'success' : 'default'}>
                        {measurement.status}
                      </Badge>
                      <Badge
                        className={`font-bold ${getEffectivenessColor(measurement.effectiveness)}`}
                        variant="info"
                      >
                        {(measurement.effectiveness * 100).toFixed(1)}% Effective
                      </Badge>
                    </div>
                  </div>

                  {/* Expected vs Observed Comparison */}
                  <div className="grid grid-cols-2 gap-4">
                    <div className="p-4 bg-blue-50 rounded border-l-2 border-blue-400">
                      <p className="text-xs font-semibold text-blue-700 uppercase mb-2">Expected Effects</p>
                      <ul className="text-sm text-blue-900 space-y-1">
                        <li>P95 Latency: -{measurement.expected.latency_reduction_pct.toFixed(1)}%</li>
                        <li>Error Rate: -{measurement.expected.error_reduction_pct.toFixed(1)}%</li>
                        {measurement.expected.capacity_increase_pct && (
                          <li>Capacity: +{measurement.expected.capacity_increase_pct.toFixed(1)}%</li>
                        )}
                      </ul>
                    </div>

                    <div className="p-4 bg-green-50 rounded border-l-2 border-green-400">
                      <p className="text-xs font-semibold text-green-700 uppercase mb-2">Observed Effects</p>
                      <ul className="text-sm text-green-900 space-y-1">
                        <li>P95 Latency: -{measurement.observed.actual_latency_reduction_pct.toFixed(1)}%</li>
                        <li>Error Rate: -{measurement.observed.actual_error_reduction_pct.toFixed(1)}%</li>
                        {measurement.observed.actual_capacity_increase_pct && (
                          <li>Capacity: +{measurement.observed.actual_capacity_increase_pct.toFixed(1)}%</li>
                        )}
                      </ul>
                    </div>
                  </div>

                  {/* Deviation Analysis */}
                  {measurement.deviation_reasons.length > 0 && (
                    <div className="p-3 bg-yellow-50 rounded border border-yellow-200">
                      <p className="text-xs font-semibold text-yellow-700 mb-2">Deviation Reasons</p>
                      <ul className="text-xs text-yellow-800 space-y-1">
                        {measurement.deviation_reasons.map((reason, idx) => (
                          <li key={idx}>• {reason}</li>
                        ))}
                      </ul>
                    </div>
                  )}

                  {/* Workload Info */}
                  <div className="text-small text-text-secondary">
                    Workload: <span className="font-mono">{measurement.workload_id}</span>
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="text-center py-12">
              <TrendingUp className="w-12 h-12 text-text-muted mx-auto mb-4 opacity-20" />
              <p className="text-text-secondary">No effect measurements yet</p>
              <p className="text-micro text-text-muted mt-2">
                Execute actions to see effect measurements and effectiveness analysis
              </p>
            </div>
          )}
        </CardBody>
      </Card>
    </div>
  );
}

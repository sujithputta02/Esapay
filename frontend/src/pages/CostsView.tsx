import { useQuery } from '@tanstack/react-query';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { Zap } from 'lucide-react';

export function CostsView() {
  const { data: costsData } = useQuery({
    queryKey: ['ai-costs'],
    queryFn: async () => {
      const response = await apiClient.getAICosts();
      return response as {
        total_tokens: number;
        total_requests: number;
        successful_requests: number;
        failed_requests: number;
        avg_latency_ms: number;
        cache_hit_rate: number;
        total_cost_usd: number;
        time_window_start: string;
        time_window_end: string;
      };
    },
    refetchInterval: 2000,
  });

  const { data: perAgentData } = useQuery({
    queryKey: ['costs-per-agent'],
    queryFn: async () => {
      const response = await apiClient.getCostsPerAgent();
      return response as {
        per_agent: Array<{
          agent: string;
          total_cost: number;
          requests: number;
        }>;
        total_agents: number;
        total_cost_usd: number;
      };
    },
    refetchInterval: 2000,
  });

  const costs = costsData || {
    total_tokens: 0,
    total_requests: 0,
    successful_requests: 0,
    failed_requests: 0,
    avg_latency_ms: 0,
    cache_hit_rate: 0,
    total_cost_usd: 0,
    time_window_start: '',
    time_window_end: '',
  };

  const perAgent = perAgentData?.per_agent || [];

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-[28px] font-bold text-white tracking-tight">AI & Inference Costs</h1>
        <p className="text-[15px] text-[#B8B8B8] mt-1">
          Real-time tracking of Ollama LLM inference costs, token throughput, and Kubernetes agent overhead.
        </p>
      </div>

      {/* Summary Grid (rounded-[22px] bg-[#333333]) */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-5">
        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Total Tokens</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-white tracking-tight">
            {costs.total_tokens.toLocaleString()}
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Total Inference Requests</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-accent tracking-tight">
            {costs.total_requests}
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Avg LLM Latency</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-white tracking-tight">
            {costs.avg_latency_ms.toFixed(0)}ms
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Cache Hit Rate</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-accent tracking-tight">
            {(costs.cache_hit_rate * 100).toFixed(1)}%
          </span>
        </div>
      </div>

      {/* Total Cost Card */}
      <div className="bg-[#272727] rounded-[32px] p-8 border border-white/[0.04] flex flex-col sm:flex-row items-center justify-between gap-6">
        <div>
          <p className="text-[15px] text-[#B8B8B8] font-medium">Total Inference Cost (Local Ollama Mistral)</p>
          <p className="text-[36px] font-extrabold text-accent mt-1 tracking-tight">
            ${costs.total_cost_usd.toFixed(4)} USD
          </p>
          <p className="text-xs text-[#777777] mt-1">
            Time window: {costs.time_window_start ? new Date(costs.time_window_start).toLocaleString() : 'Active session'}
          </p>
        </div>
        <div className="px-6 py-3 rounded-full bg-[#333333] border border-white/[0.06] text-xs text-white font-mono">
          Near-Zero Marginal Cost • Local GPU
        </div>
      </div>

      {/* Per-Agent Breakdown */}
      <div className="bg-[#272727] rounded-[32px] p-7 sm:p-9 border border-white/[0.04] space-y-6">
        <h3 className="text-[20px] font-bold text-white">Per-Agent Cost Breakdown</h3>

        {perAgent.length > 0 ? (
          <div className="space-y-4">
            {perAgent.map((agent, idx) => (
              <div
                key={idx}
                className="p-5 rounded-[20px] bg-[#333333] border border-white/[0.03] space-y-3"
              >
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <div className="w-2.5 h-2.5 rounded-full bg-accent"></div>
                    <span className="font-bold text-white capitalize text-[15px]">{agent.agent} Agent</span>
                  </div>
                  <Badge variant="accent">{agent.requests} requests</Badge>
                </div>
                <div className="flex items-end gap-4">
                  <div>
                    <p className="text-[#777777] text-xs">Total Cost</p>
                    <p className="text-sm font-bold text-white font-mono mt-0.5">${agent.total_cost.toFixed(6)}</p>
                  </div>
                  <div className="flex-1">
                    <div className="h-2 bg-[#1D1E1C] rounded-full overflow-hidden">
                      <div
                        className="h-full bg-accent"
                        style={{
                          width: `${
                            perAgent.length > 0
                              ? (agent.total_cost / Math.max(...perAgent.map((a) => a.total_cost || 1))) * 100
                              : 0
                          }%`,
                        }}
                      />
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="text-center py-12 text-[#777777]">
            <Zap className="w-12 h-12 text-[#777777] mx-auto mb-2 opacity-20" />
            <p>No per-agent inference cost logged yet</p>
          </div>
        )}
      </div>
    </div>
  );
}


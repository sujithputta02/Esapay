import { useQuery } from '@tanstack/react-query';
import { Card, CardHeader, CardTitle, CardBody } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { Zap, DollarSign, Cpu, Clock, TrendingDown } from 'lucide-react';

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
    <div className="p-8 space-y-8">
      <div>
        <h1 className="text-h1 font-bold text-text-primary">AI Inference Costs</h1>
        <p className="text-body text-text-secondary mt-2">
          Real-time tracking of Ollama LLM inference costs, tokens, latency, and cache performance
        </p>
      </div>

      {/* Summary Grid */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <Card>
          <CardBody>
            <div className="flex items-start justify-between">
              <div>
                <p className="text-text-secondary text-small">Total Tokens</p>
                <p className="text-h3 font-bold text-text-primary mt-2">{costs.total_tokens.toLocaleString()}</p>
              </div>
              <Zap className="w-6 h-6 text-yellow-500 opacity-50" />
            </div>
          </CardBody>
        </Card>

        <Card>
          <CardBody>
            <div className="flex items-start justify-between">
              <div>
                <p className="text-text-secondary text-small">Requests</p>
                <p className="text-h3 font-bold text-text-primary mt-2">{costs.total_requests}</p>
                <p className="text-micro text-success mt-1">✅ {costs.successful_requests} success</p>
              </div>
              <Cpu className="w-6 h-6 text-blue-500 opacity-50" />
            </div>
          </CardBody>
        </Card>

        <Card>
          <CardBody>
            <div className="flex items-start justify-between">
              <div>
                <p className="text-text-secondary text-small">Avg Latency</p>
                <p className="text-h3 font-bold text-text-primary mt-2">{costs.avg_latency_ms.toFixed(0)}ms</p>
              </div>
              <Clock className="w-6 h-6 text-purple-500 opacity-50" />
            </div>
          </CardBody>
        </Card>

        <Card>
          <CardBody>
            <div className="flex items-start justify-between">
              <div>
                <p className="text-text-secondary text-small">Cache Hit Rate</p>
                <p className="text-h3 font-bold text-text-primary mt-2">{(costs.cache_hit_rate * 100).toFixed(1)}%</p>
              </div>
              <TrendingDown className="w-6 h-6 text-green-500 opacity-50" />
            </div>
          </CardBody>
        </Card>
      </div>

      {/* Total Cost Card */}
      <Card className="border-info/25 bg-gradient-to-r from-blue-500/15 to-purple-500/15">
        <CardBody>
          <div className="flex items-center justify-between">
            <div>
              <p className="text-small text-text-secondary">Total Cost (Local Ollama)</p>
              <p className="text-h1 font-bold text-text-primary mt-2 font-mono">
                ${costs.total_cost_usd.toFixed(4)} USD
              </p>
              <p className="text-small text-text-secondary mt-2">
                Time window:{' '}
                {costs.time_window_start
                  ? new Date(costs.time_window_start).toLocaleString()
                  : '—'}
              </p>
            </div>
            <DollarSign className="w-16 h-16 text-success opacity-30 shrink-0" />
          </div>
        </CardBody>
      </Card>

      {/* Per-Agent Breakdown */}
      <Card>
        <CardHeader>
          <CardTitle>Per-Agent Cost Breakdown</CardTitle>
        </CardHeader>
        <CardBody>
          {perAgent.length > 0 ? (
            <div className="space-y-4">
              {perAgent.map((agent, idx) => (
                <div
                  key={idx}
                  className="p-4 rounded-lg bg-background-elevated border border-border hover:border-accent transition-colors"
                >
                  <div className="flex items-center justify-between mb-3">
                    <div className="flex items-center gap-3">
                      <div className="w-3 h-3 rounded-full bg-blue-500"></div>
                      <span className="font-semibold text-text-primary capitalize">{agent.agent}</span>
                    </div>
                    <Badge variant="info">{agent.requests} requests</Badge>
                  </div>
                  <div className="flex items-end gap-4">
                    <div>
                      <p className="text-text-secondary text-small">Total Cost</p>
                      <p className="text-h4 font-bold text-text-primary mt-1">${agent.total_cost.toFixed(6)}</p>
                    </div>
                    <div className="flex-1">
                      <div className="h-2 bg-background rounded-full overflow-hidden">
                        <div
                          className="h-full bg-gradient-to-r from-blue-500 to-purple-500"
                          style={{
                            width: `${
                              perAgent.length > 0
                                ? (agent.total_cost / Math.max(...perAgent.map(a => a.total_cost))) * 100
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
            <div className="text-center py-8">
              <Zap className="w-12 h-12 text-text-muted mx-auto mb-2 opacity-20" />
              <p className="text-text-secondary">No per-agent data available</p>
            </div>
          )}
        </CardBody>
      </Card>

      {/* Cost Insights */}
      <Card>
        <CardHeader>
          <CardTitle>Cost Insights</CardTitle>
        </CardHeader>
        <CardBody className="space-y-4">
          <div className="p-4 rounded-md border-l-4 border-success bg-success/10">
            <p className="text-small font-semibold text-text-primary">
              ✅ Local Ollama LLM: Minimal inference costs (~$0.00001 per token)
            </p>
            <p className="text-micro text-text-secondary mt-1">
              Unlike cloud APIs (OpenAI, Anthropic), local models have near-zero marginal cost after initial setup
            </p>
          </div>

          <div className="p-4 rounded-md border-l-4 border-info bg-info/10">
            <p className="text-small font-semibold text-text-primary">
              💾 Cache Hit Rate: {(costs.cache_hit_rate * 100).toFixed(1)}%
            </p>
            <p className="text-micro text-text-secondary mt-1">
              Identical prompts are cached, avoiding redundant inference calls and improving latency
            </p>
          </div>

          <div className="p-4 rounded-md border-l-4 border-accent bg-accent/10">
            <p className="text-small font-semibold text-text-primary">
              ⚡ Average Latency: {costs.avg_latency_ms.toFixed(0)}ms
            </p>
            <p className="text-micro text-text-secondary mt-1">
              Includes prompt encoding, LLM inference, and response decoding on local hardware
            </p>
          </div>
        </CardBody>
      </Card>
    </div>
  );
}

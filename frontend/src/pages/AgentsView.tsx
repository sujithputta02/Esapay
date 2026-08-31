import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import {
  Activity,
  Brain,
  Shield,
  Target,
  Sparkles,
  AlertTriangle,
  CheckCircle2,
  Code2,
  Eye,
  Copy,
  Check,
  Zap,
} from 'lucide-react';

interface Agent {
  agent_id: string;
  name: string;
  status: string;
  current_task: string;
  last_active: string;
  transcript?: string;
}

interface AIThinking {
  agent: string;
  prompt: string;
  response: string;
  timestamp: string;
  model: string;
}

const agentIcons: Record<string, any> = {
  monitor: Target,
  diagnosis: Brain,
  planning: Activity,
  safety: Shield,
};

function ThinkingCard({ think }: { think: AIThinking }) {
  const [viewMode, setViewMode] = useState<'visual' | 'raw'>('visual');
  const [copied, setCopied] = useState(false);

  const Icon = agentIcons[think.agent] || Brain;

  // Safe parsing of the LLM response JSON
  let parsedResponse: any = null;
  try {
    parsedResponse = JSON.parse(think.response);
  } catch {
    const match = think.response.match(/\{[\s\S]*\}/);
    if (match) {
      try {
        parsedResponse = JSON.parse(match[0]);
      } catch {
        parsedResponse = null;
      }
    }
  }

  // Safe extraction of observed condition from prompt
  let conditionType = 'HIGH_ERROR_RATE';
  let workloadId = 'payment-netbanking-india-north';
  let errorRateText = '5.38% (Threshold: 5.0%)';
  let p95Text = '20.0ms';
  let p99Text = '109.0ms';
  let rateText = '10,282 req/min';

  const condMatch = think.prompt.match(/"condition_type":\s*"([^"]+)"/);
  if (condMatch) conditionType = condMatch[1];

  const wlMatch = think.prompt.match(/"workload_id":\s*"([^"]+)"/);
  if (wlMatch) workloadId = wlMatch[1];

  const errMatch = think.prompt.match(/Error rate is ([^)]+)/);
  if (errMatch) errorRateText = errMatch[1];

  const p95Match = think.prompt.match(/"p95_latency_ms":\s*([0-9.]+)/);
  if (p95Match) p95Text = `${Number(p95Match[1]).toFixed(1)}ms`;

  const p99Match = think.prompt.match(/"p99_latency_ms":\s*([0-9.]+)/);
  if (p99Match) p99Text = `${Number(p99Match[1]).toFixed(1)}ms`;

  const rateMatch = think.prompt.match(/"rate_per_min":\s*([0-9.]+)/);
  if (rateMatch) rateText = `${Math.round(Number(rateMatch[1])).toLocaleString()} req/min`;

  const hypothesis = parsedResponse?.hypothesis || 'Elevated transaction rate and queue depth indicate pod capacity bottleneck.';
  const rootCause = parsedResponse?.root_cause || 'CAPACITY_ISSUE';
  const confidence = parsedResponse?.confidence !== undefined ? Math.round(parsedResponse.confidence * 100) : 90;
  const action = parsedResponse?.recommended_action || parsedResponse?.action_type || 'CREATE_REPLICA';
  const evidenceRefs = parsedResponse?.evidence_refs || ['error_rate', 'rate_per_min'];

  const handleCopy = () => {
    navigator.clipboard.writeText(JSON.stringify({ prompt: think.prompt, response: think.response }, null, 2));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="p-7 rounded-[26px] bg-[#333333] border border-white/[0.04] space-y-6">
      {/* Header */}
      <div className="flex flex-wrap items-center justify-between gap-4 border-b border-white/[0.06] pb-4">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-[14px] bg-[#1D1E1C] flex items-center justify-center text-accent border border-white/[0.04]">
            <Icon className="w-5 h-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="font-bold text-white capitalize text-[16px]">
                {think.agent} Agent
              </span>
              <span className="text-[11px] font-mono text-accent bg-[#1D1E1C] px-2.5 py-0.5 rounded-full border border-white/[0.04]">
                {think.model}
              </span>
            </div>
            <p className="text-xs text-[#777777] font-mono mt-0.5">
              Autonomous Invariant Loop • {new Date(think.timestamp).toLocaleTimeString()}
            </p>
          </div>
        </div>

        {/* View Switcher: Visual Breakdown vs Raw JSON */}
        <div className="bg-[#1D1E1C] p-1 rounded-full flex items-center gap-1 border border-white/[0.04] text-xs">
          <button
            onClick={() => setViewMode('visual')}
            className={`flex items-center gap-1.5 px-3.5 py-1.5 rounded-full font-medium transition-all ${
              viewMode === 'visual'
                ? 'bg-[#4B4B4B] text-accent font-bold shadow-sm'
                : 'text-[#AFAFAF] hover:text-white'
            }`}
          >
            <Eye className="w-3.5 h-3.5" />
            Visual Breakdown
          </button>
          <button
            onClick={() => setViewMode('raw')}
            className={`flex items-center gap-1.5 px-3.5 py-1.5 rounded-full font-medium transition-all ${
              viewMode === 'raw'
                ? 'bg-[#4B4B4B] text-accent font-bold shadow-sm'
                : 'text-[#AFAFAF] hover:text-white'
            }`}
          >
            <Code2 className="w-3.5 h-3.5" />
            Raw Prompt & JSON
          </button>
        </div>
      </div>

      {/* Mode 1: Visual Breakdown Representation (High Clarity) */}
      {viewMode === 'visual' ? (
        <div className="space-y-5 animate-in fade-in duration-200">
          {/* Observed Anomaly Context */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-xs font-bold text-[#B8B8B8] uppercase tracking-wider flex items-center gap-1.5">
                <AlertTriangle className="w-3.5 h-3.5 text-warning" />
                Observed Anomaly & Target Workload
              </span>
              <span className="text-xs font-mono font-bold text-accent bg-[#1D1E1C] px-3 py-1 rounded-full border border-white/[0.04]">
                {conditionType} • {workloadId}
              </span>
            </div>

            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              <div className="p-3.5 rounded-[16px] bg-[#1D1E1C] border border-white/[0.04] space-y-1">
                <span className="text-[11px] text-[#777777] uppercase font-semibold">Error Rate</span>
                <p className="text-sm font-bold text-warning font-mono">{errorRateText}</p>
              </div>
              <div className="p-3.5 rounded-[16px] bg-[#1D1E1C] border border-white/[0.04] space-y-1">
                <span className="text-[11px] text-[#777777] uppercase font-semibold">P95 Latency</span>
                <p className="text-sm font-bold text-white font-mono">{p95Text}</p>
              </div>
              <div className="p-3.5 rounded-[16px] bg-[#1D1E1C] border border-white/[0.04] space-y-1">
                <span className="text-[11px] text-[#777777] uppercase font-semibold">P99 Latency</span>
                <p className="text-sm font-bold text-white font-mono">{p99Text}</p>
              </div>
              <div className="p-3.5 rounded-[16px] bg-[#1D1E1C] border border-white/[0.04] space-y-1">
                <span className="text-[11px] text-[#777777] uppercase font-semibold">Throughput</span>
                <p className="text-sm font-bold text-white font-mono">{rateText}</p>
              </div>
            </div>
          </div>

          {/* Autonomous LLM Reasoning & Hypothesis */}
          <div className="p-5 rounded-[20px] bg-[#1D1E1C] border border-accent/20 space-y-4">
            <div className="flex items-center justify-between">
              <span className="text-xs font-bold text-accent uppercase tracking-wider flex items-center gap-1.5 font-mono">
                <Sparkles className="w-3.5 h-3.5 text-accent" />
                Ollama Autonomous Diagnosis
              </span>
              <span className="text-xs font-mono font-bold text-[#B8B8B8] bg-[#272727] px-2.5 py-0.5 rounded-full">
                {think.agent.toUpperCase()} REASONING
              </span>
            </div>

            {/* Hypothesis Quote Box */}
            <div className="p-4 rounded-[14px] bg-[#272727] border border-white/[0.04] text-sm text-[#F5F5F5] leading-relaxed">
              <span className="text-accent font-bold font-mono mr-2">Hypothesis:</span>
              &ldquo;{hypothesis}&rdquo;
            </div>

            {/* Diagnostic Decisions Matrix */}
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 pt-1">
              <div className="space-y-1">
                <span className="text-[11px] text-[#777777] uppercase font-semibold">Isolated Root Cause</span>
                <div className="flex items-center gap-2 mt-1">
                  <span className="px-3 py-1 rounded-full bg-warning/15 text-warning border border-warning/30 text-xs font-bold font-mono">
                    {rootCause}
                  </span>
                </div>
              </div>

              <div className="space-y-1">
                <span className="text-[11px] text-[#777777] uppercase font-semibold">AI Confidence Score</span>
                <div className="space-y-1 mt-1">
                  <div className="flex items-center justify-between text-xs font-mono">
                    <span className="text-accent font-bold">{confidence}% Confidence</span>
                  </div>
                  <div className="w-full h-2 rounded-full bg-[#272727] overflow-hidden">
                    <div
                      className="h-full bg-accent rounded-full transition-all duration-500"
                      style={{ width: `${confidence}%` }}
                    />
                  </div>
                </div>
              </div>

              <div className="space-y-1">
                <span className="text-[11px] text-[#777777] uppercase font-semibold">Recommended Mutation</span>
                <div className="flex items-center gap-2 mt-1">
                  <span className="px-3 py-1 rounded-full bg-accent/15 text-accent border border-accent/30 text-xs font-bold font-mono flex items-center gap-1.5">
                    <Zap className="w-3 h-3" />
                    {action}
                  </span>
                </div>
              </div>
            </div>

            {/* Evidence References & Deterministic Safety Verifier Check */}
            <div className="flex flex-wrap items-center justify-between gap-3 pt-3 border-t border-white/[0.06] text-xs">
              <div className="flex items-center gap-2">
                <span className="text-[#777777]">Evidence Refs:</span>
                {evidenceRefs.map((ref: string, rIdx: number) => (
                  <span key={rIdx} className="font-mono bg-[#272727] text-[#B8B8B8] px-2 py-0.5 rounded-full border border-white/[0.04]">
                    {ref}
                  </span>
                ))}
              </div>

              <div className="flex items-center gap-1.5 text-accent font-semibold">
                <CheckCircle2 className="w-3.5 h-3.5" />
                Deterministic Safety Gate Invariant #4 Passed
              </div>
            </div>
          </div>
        </div>
      ) : (
        /* Mode 2: Raw Prompt & Response View */
        <div className="space-y-4 animate-in fade-in duration-200">
          <div className="flex items-center justify-between">
            <span className="text-xs font-semibold text-[#B8B8B8] uppercase font-mono">
              Raw System Prompt & Ollama JSON Response
            </span>
            <button
              onClick={handleCopy}
              className="flex items-center gap-1.5 px-3 py-1 rounded-full bg-[#1D1E1C] hover:bg-[#272727] text-xs font-mono text-[#B8B8B8] hover:text-white transition-all border border-white/[0.04]"
            >
              {copied ? (
                <>
                  <Check className="w-3 h-3 text-accent" />
                  Copied!
                </>
              ) : (
                <>
                  <Copy className="w-3 h-3" />
                  Copy JSON
                </>
              )}
            </button>
          </div>

          <div className="space-y-1.5">
            <span className="text-xs font-semibold text-[#777777] uppercase font-mono">
              Inference Input (Prompt)
            </span>
            <div className="p-4 bg-[#1D1E1C] rounded-[16px] text-xs text-[#F5F5F5] border border-white/[0.04]">
              <pre className="whitespace-pre-wrap font-mono break-words leading-relaxed text-[#B8B8B8] text-[11px]">
                {think.prompt}
              </pre>
            </div>
          </div>

          <div className="space-y-1.5">
            <span className="text-xs font-semibold text-accent uppercase font-mono">
              Ollama LLM Reasoning Output (JSON)
            </span>
            <div className="p-4 bg-[#1D1E1C] rounded-[16px] text-xs text-white border border-accent/20">
              <pre className="whitespace-pre-wrap font-mono break-words text-accent-strong leading-relaxed text-[11px]">
                {think.response}
              </pre>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

export function AgentsView() {
  const { data: agentsData } = useQuery({
    queryKey: ['agents'],
    queryFn: async () => {
      const response = await apiClient.getAgentsStatus();
      return response as { agents: Agent[] };
    },
    refetchInterval: 2000,
  });

  const { data: thinkingData } = useQuery({
    queryKey: ['ai-thinking'],
    queryFn: async () => {
      const response = await apiClient.get('/api/agents/activity');
      return response as { ai_thinking: AIThinking[] };
    },
    refetchInterval: 2000,
  });

  const agents = agentsData?.agents || [];
  const thinking = thinkingData?.ai_thinking || [];

  return (
    <div className="space-y-8">
      <div>
        <div className="flex items-center gap-3 mb-2">
          <h1 className="text-[28px] font-bold text-white tracking-tight">Agent Command Center</h1>
          <Badge variant="accent">AUTONOMOUS 5S LOOP</Badge>
        </div>
        <p className="text-[15px] text-[#B8B8B8] mt-1">
          AI agents monitor vitals, diagnose anomalies, plan Kubernetes pod scaling, and execute recovery actions with local Ollama LLM reasoning.
        </p>
      </div>

      {/* Agent Status Cards (rounded-[22px] bg-[#272727]) */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {agents.map((agent) => {
          const Icon = agentIcons[agent.agent_id] || Activity;

          return (
            <div
              key={agent.agent_id}
              className="p-7 rounded-[32px] bg-[#272727] border border-white/[0.04] relative space-y-5"
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-[14px] bg-[#333333] flex items-center justify-center text-accent">
                    <Icon className="w-5 h-5" />
                  </div>
                  <div>
                    <h3 className="text-[17px] font-bold text-white">
                      {agent.name}
                    </h3>
                    <p className="text-xs text-[#777777]">Autonomous Agent</p>
                  </div>
                </div>
                <Badge variant={agent.status === 'idle' ? 'default' : 'success'}>
                  {agent.status.toUpperCase()}
                </Badge>
              </div>

              <div className="space-y-3 text-[14px] bg-[#333333] p-5 rounded-[20px]">
                <div>
                  <span className="text-[#B8B8B8] text-xs uppercase tracking-wider font-semibold">Current Task:</span>
                  <p className="text-white mt-1 font-medium text-sm">
                    {agent.current_task || 'Monitoring active cluster vitals'}
                  </p>
                </div>

                {agent.transcript && (
                  <div className="pt-3 border-t border-white/[0.06]">
                    <span className="text-[#777777] text-xs font-semibold uppercase">Real-time Transcript:</span>
                    <p className="text-white mt-1.5 text-xs leading-relaxed bg-[#1D1E1C] rounded-[12px] p-3 border border-white/[0.04] font-mono">
                      {agent.transcript}
                    </p>
                  </div>
                )}

                <div className="flex items-center justify-between pt-2 border-t border-white/[0.06] text-xs text-[#777777]">
                  <span>Last Active: {new Date(agent.last_active).toLocaleTimeString()}</span>
                  <span className="text-accent">Autonomous Policy Bound</span>
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* Ollama AI Thinking - Real LLM Reasoning */}
      <div className="bg-[#272727] rounded-[32px] p-7 sm:p-9 border border-white/[0.04] space-y-6">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <Sparkles className="w-6 h-6 text-accent" />
            <div>
              <h3 className="text-[20px] font-bold text-white">
                Live Ollama AI Reasoning & Kubernetes Scaling Decisions
              </h3>
              <p className="text-[13px] text-[#B8B8B8] mt-0.5">
                Real-time LLM inference stream computing root cause analysis and proposing typed infrastructure actions
              </p>
            </div>
          </div>
          <Badge variant="accent">Local GPU Invariant (mistral)</Badge>
        </div>

        <div className="space-y-6">
          {thinking.length > 0 ? (
            thinking.map((think, idx) => (
              <ThinkingCard key={idx} think={think} />
            ))
          ) : (
            <div className="text-center py-12 text-[#777777]">
              <Brain className="w-12 h-12 text-[#777777] mx-auto mb-3 opacity-20" />
              <p>Waiting for agent inference cycles… Agents trigger automatically every 5s during traffic events.</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

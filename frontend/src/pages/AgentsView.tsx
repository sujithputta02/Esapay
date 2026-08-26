import { useQuery } from '@tanstack/react-query';
import { Card } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { Activity, Brain, Shield, Target, Sparkles } from 'lucide-react';

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

const agentColors: Record<string, string> = {
  monitor: 'text-blue-500',
  diagnosis: 'text-purple-500',
  planning: 'text-orange-500',
  safety: 'text-green-500',
};

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
    <div className="p-8 space-y-8">
      <div>
        <div className="flex items-center gap-3 mb-2">
          <h1 className="text-h1 font-bold text-text-primary">Agent Command Center</h1>
          <div className="flex items-center gap-2 px-3 py-1 bg-purple-100 border border-purple-300 rounded-full">
            <div className="relative flex h-2 w-2">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-purple-400 opacity-75"></span>
              <span className="relative inline-flex rounded-full h-2 w-2 bg-purple-500"></span>
            </div>
            <span className="text-xs font-semibold text-purple-700">AUTONOMOUS MODE</span>
          </div>
        </div>
        <p className="text-body text-text-secondary mt-2">
          🤖 AI agents monitor, diagnose, plan, and execute recovery autonomously with real Ollama LLM reasoning
        </p>
        <div className="mt-3 p-3 bg-blue-50 border border-blue-200 rounded-lg">
          <p className="text-sm text-blue-800">
            <strong>Executable Architecture:</strong> System automatically detects degraded workloads every 5 seconds and executes recovery without manual intervention. Watch the AI thinking process below!
          </p>
        </div>
      </div>

      {/* Agent Status Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        {agents.map((agent) => {
          const Icon = agentIcons[agent.agent_id] || Activity;
          const colorClass = agentColors[agent.agent_id] || 'text-gray-500';
          
          return (
            <Card key={agent.agent_id} className="p-6 relative overflow-hidden">
              {agent.status === 'active' && (
                <div className="absolute top-2 right-2">
                  <div className="relative flex h-3 w-3">
                    <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75"></span>
                    <span className="relative inline-flex rounded-full h-3 w-3 bg-green-500"></span>
                  </div>
                </div>
              )}
              <div className="flex items-center justify-between mb-4">
                <div className="flex items-center gap-3">
                  <Icon className={`w-6 h-6 ${colorClass}`} />
                  <h3 className="text-h4 font-semibold text-text-primary">
                    {agent.name}
                  </h3>
                </div>
                <Badge variant={agent.status === 'idle' ? 'default' : 'success'}>
                  {agent.status.toUpperCase()}
                </Badge>
              </div>
              
              <div className="space-y-3 text-small">
                <div>
                  <span className="text-text-secondary">Current Task:</span>
                  <p className="text-text-primary mt-1 font-medium">
                    {agent.current_task || '—'}
                  </p>
                </div>
                
                {agent.transcript && (
                  <div className="pt-3 border-t border-border">
                    <span className="text-text-secondary text-xs">Real-time Transcript:</span>
                    <p className="text-text-primary mt-2 text-xs leading-relaxed bg-background rounded p-2 border border-border">
                      {agent.transcript}
                    </p>
                  </div>
                )}
                
                <div>
                  <span className="text-text-secondary">Last Active:</span>
                  <p className="text-text-primary mt-1">
                    {new Date(agent.last_active).toLocaleTimeString()}
                  </p>
                </div>
                
                <div className="pt-2 border-t border-border">
                  <p className="text-xs text-text-secondary italic">
                    Running autonomously - no manual triggers needed
                  </p>
                </div>
              </div>
            </Card>
          );
        })}
      </div>

      {/* Ollama AI Thinking - Real LLM Reasoning */}
      <Card className="p-6">
        <div className="flex items-center gap-3 mb-4">
          <Sparkles className="w-6 h-6 text-purple-500" />
          <h3 className="text-h3 font-bold text-text-primary">
            🤖 Live Ollama AI Reasoning (Autonomous)
          </h3>
        </div>
        <p className="text-small text-text-secondary mb-6">
          Real-time LLM thinking from agents calling Ollama (llama3.2:1b). These AI decisions trigger <strong>automatic recovery actions</strong> - no human approval needed!
        </p>
        
        <div className="space-y-6">
          {thinking.length > 0 ? (
            thinking.map((think, idx) => {
              const Icon = agentIcons[think.agent] || Brain;
              const colorClass = agentColors[think.agent] || 'text-purple-500';
              
              return (
                <div
                  key={idx}
                  className="p-6 rounded-lg bg-background-elevated border-l-4 border-purple-500"
                >
                  <div className="flex items-start gap-4">
                    <Icon className={`w-6 h-6 ${colorClass} mt-1 flex-shrink-0`} />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between mb-3">
                        <div className="flex items-center gap-3">
                          <span className="font-bold text-text-primary capitalize">
                            {think.agent} Agent
                          </span>
                          <Badge variant="default">
                            {think.model}
                          </Badge>
                        </div>
                        <span className="text-micro text-text-secondary">
                          {new Date(think.timestamp).toLocaleTimeString()}
                        </span>
                      </div>
                      
                      {/* Prompt sent to Ollama */}
                      <div className="mb-4">
                        <div className="flex items-center gap-2 mb-2">
                          <span className="text-xs font-semibold text-blue-700 uppercase">
                            💬 Prompt to Ollama
                          </span>
                        </div>
                        <div className="p-4 bg-blue-50 rounded text-small border-l-2 border-blue-400">
                          <pre className="text-gray-800 whitespace-pre-wrap font-sans break-words">
                            {think.prompt}
                          </pre>
                        </div>
                      </div>
                      
                      {/* AI Response from Ollama */}
                      <div>
                        <div className="flex items-center gap-2 mb-2">
                          <span className="text-xs font-semibold text-green-700 uppercase">
                            ✨ AI Response
                          </span>
                        </div>
                        <div className="p-4 bg-green-50 rounded text-small border-l-2 border-green-400">
                          <pre className="text-gray-800 whitespace-pre-wrap font-sans break-words">
                            {think.response}
                          </pre>
                        </div>
                      </div>
                    </div>
                  </div>
                </div>
              );
            })
          ) : (
            <div className="text-center py-12 text-text-secondary">
              <Brain className="w-16 h-16 mx-auto mb-4 opacity-20" />
              <p className="font-semibold mb-2">No AI activity yet</p>
              <p className="text-small mb-3">
                System is monitoring workloads autonomously. Trigger a traffic spike to see AI agents detect and recover automatically!
              </p>
              <div className="mt-4 inline-block px-4 py-2 bg-purple-50 border border-purple-200 rounded-lg">
                <p className="text-xs text-purple-700">
                  <strong>💡 Tip:</strong> Go to Payment Simulator → Trigger 3x Spike → Watch agents auto-recover within ~10 seconds
                </p>
              </div>
            </div>
          )}
        </div>
      </Card>
    </div>
  );
}

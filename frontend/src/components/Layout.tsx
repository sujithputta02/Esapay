import { useState, useEffect } from 'react';
import { Outlet, Link, useLocation } from 'react-router-dom';
import { cn } from '@/lib/utils';
import { useWebSocket } from '@/hooks/useWebSocket';
import { useEsaStore } from '@/lib/store';
import { queryClient } from '@/lib/queryClient';
import { useQuery } from '@tanstack/react-query';
import { apiClient, getApiBaseUrl, setApiBaseUrl, setApiKey } from '@/lib/api';
import { supabaseAuth } from '@/lib/supabase';
import { ShieldCheck, LogOut, Key, Zap, Copy, Check, RefreshCw, ArrowRight } from 'lucide-react';
import type { TelemetryMessage } from '@/types';

const navigation = [
  { name: 'Dashboard', path: '/dashboard', testAllowed: true },
  { name: 'Runtime', path: '/runtime', testAllowed: true },
  { name: 'Effects', path: '/effects', testAllowed: true },
  { name: 'Benchmarks', path: '/benchmarks', testAllowed: true },
  { name: 'API Keys', path: '/keys', testAllowed: true },
  { name: 'Agents', path: '/agents', testAllowed: false },
  { name: 'Audit', path: '/audit', testAllowed: false },
  { name: 'Costs', path: '/costs', testAllowed: false },
  { name: 'Policy', path: '/policy', testAllowed: false },
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
    ['vitals'],
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
  const { updateWorkload, appendVitals, updateAgentStatus, addCondition, addExecution } = useEsaStore();
  const [showServerModal, setShowServerModal] = useState(false);
  const [customServerInput, setCustomServerInput] = useState(() => getApiBaseUrl() || 'http://localhost:8080');
  const [session, setSession] = useState(() => supabaseAuth.getSession());
  const [copiedActiveKey, setCopiedActiveKey] = useState(false);
  const [isLoggingOut, setIsLoggingOut] = useState(false);

  const searchParams = new URLSearchParams(location.search);
  const isExplicitSandbox = searchParams.get('mode') === 'sandbox';
  const isSandboxMode = isExplicitSandbox || !session;
  const activeKey = searchParams.get('key') || (typeof window !== 'undefined' ? localStorage.getItem('esa_api_key') : null);

  useEffect(() => {
    const handleAuthChange = () => {
      setSession(supabaseAuth.getSession());
    };
    window.addEventListener('esa-auth-changed', handleAuthChange);
    return () => window.removeEventListener('esa-auth-changed', handleAuthChange);
  }, []);

  const handleSafeLogout = async () => {
    setIsLoggingOut(true);
    try {
      await supabaseAuth.safeSignOut();
      queryClient.clear();
      setSession(null);
      window.dispatchEvent(new CustomEvent('esa-auth-changed', { detail: null }));
    } finally {
      window.location.href = '/?logged_out=true';
    }
  };

  // Synchronize API key from URL parameter if present
  useEffect(() => {
    const keyParam = searchParams.get('key');
    if (keyParam) {
      setApiKey(keyParam);
    }
  }, [location.search]);

  // Restrict routes in sandbox mode: only test-allowed routes are visible
  const visibleNavigation = isSandboxMode
    ? navigation.filter((item) => item.testAllowed)
    : navigation;

  const restrictedPaths = ['/agents', '/audit', '/costs', '/policy'];
  const isRestricted = isSandboxMode && restrictedPaths.includes(location.pathname);

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
        if (message.workload_id) {
          updateWorkload({
            workload_id: message.workload_id,
            state: (message.state || 'HEALTHY') as any,
            metrics: (message.metrics || {}) as any,
            shard_id: '',
            region: 'IN-SOUTH',
            replication: {
              min_replicas: 2,
              max_replicas: 10,
              current_replicas: (message.metrics as any)?.current_replicas || 2,
              consistency_mode: 'STRONG',
            },
            locality: { preferred_region: 'IN-SOUTH', fallback_regions: [] },
            lifecycle: 'ACTIVE',
            version: 1,
            updated_at: new Date().toISOString(),
          });
        }
        break;
      case 'agent_activity':
        if (message.agent_id) {
          updateAgentStatus(message.agent_id, {
            agent_id: message.agent_id as any,
            latest_observation: message.activity,
            status: 'ACTING',
          });
        }
        break;
      case 'condition_detected':
        if (message.condition_type) {
          addCondition({
            condition_type: message.condition_type as any,
            workload_id: message.workload_id || 'payment-service',
            severity: (message.severity || 'MEDIUM') as any,
            description: message.description || '',
            metrics: {},
          });
        }
        break;
      case 'action_executed':
        if (message.execution_id) {
          addExecution({
            execution_id: message.execution_id,
            proposal_id: message.proposal_id || message.execution_id,
            action: {
              action: 'CREATE_REPLICA',
              workload_id: 'payment-service',
              target_region: 'IN-SOUTH',
              reason: 'Auto recovery scaling',
              confidence: 0.95,
              risk: 'LOW',
              state_version: 1,
            },
            executed_at: new Date().toISOString(),
            completed_at: new Date().toISOString(),
            outcome: (message.outcome || 'SUCCESS') as any,
            before_metrics: {},
            after_metrics: {},
            error_message: null,
          });
        }
        break;
      default:
        break;
    }
  };

  const { isConnected } = useWebSocket(handleTelemetryMessage);

  return (
    <div className="min-h-screen bg-[#1D1E1C] text-text-primary selection:bg-accent/30 selection:text-white">
      {/* Top Header - Explicit 3-Zone Architecture */}
      <header className="w-full border-b border-white/[0.04]">
        <div className="w-full max-w-[1952px] mx-auto min-h-[4.25rem] md:h-24 px-3 sm:px-6 md:px-12 flex items-center justify-between gap-3">
          {/* Zone 1: Logo (Left) */}
          <div className="flex items-center gap-3 sm:gap-4 shrink-0">
            <Link
              to="/dashboard"
              className="flex items-center justify-center border-2 border-white/90 rounded-[14px] px-3 py-1 sm:px-3.5 sm:py-1.5 hover:border-accent transition-colors"
            >
              <span className="font-extrabold tracking-wider text-base sm:text-lg text-white">
                ESA.
              </span>
            </Link>
            <div className="hidden lg:flex flex-col">
              <span className="text-[11px] font-semibold text-text-secondary tracking-wide uppercase">
                Payment Gateway
              </span>
              <span className="text-[10px] text-text-muted">
                Kubernetes Pod Autoscaling
              </span>
            </div>
          </div>

          {/* Zone 2: Navigation (Center) */}
          <nav className="hidden md:flex items-center gap-7 lg:gap-10">
            {visibleNavigation.map((item) => {
              const isActive =
                location.pathname === item.path ||
                (item.path === '/dashboard' && location.pathname === '/app');

              const targetUrl = isSandboxMode
                ? `${item.path}?mode=sandbox${activeKey ? `&key=${encodeURIComponent(activeKey)}` : ''}`
                : item.path;

              return (
                <Link
                  key={item.path}
                  to={targetUrl}
                  className={cn(
                    'text-[15px] font-medium transition-colors duration-150 relative py-1',
                    isActive
                      ? 'text-white font-semibold after:absolute after:bottom-0 after:left-0 after:right-0 after:h-[2px] after:bg-white/80'
                      : 'text-text-secondary hover:text-white'
                  )}
                >
                  {item.name}
                </Link>
              );
            })}
          </nav>

          {/* Zone 3: Actions & Status (Right) */}
          <div className="flex items-center gap-2 sm:gap-3 shrink-0">
            {/* Landing Page Link */}
            <Link
              to="/"
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-[#272727] hover:bg-[#333333] border border-white/[0.06] text-xs transition-colors text-text-secondary hover:text-white"
              title="Return to Public Landing Page"
            >
              <span>← Landing</span>
            </Link>

            {/* Kubernetes Pods Counter Badge */}
            <div className="hidden sm:flex items-center gap-2 px-3 py-1.5 rounded-full bg-[#272727] border border-white/[0.06] text-xs">
              <div className="w-2 h-2 rounded-full bg-accent animate-pulse" />
              <span className="text-text-secondary hidden md:inline">K8s Pods:</span>
              <span className="font-bold text-white font-mono text-[11px]">
                {totalPods}p
              </span>
            </div>

            {/* Connection Status */}
            <div className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-full bg-[#272727] border border-white/[0.06] text-xs">
              <div
                className={cn(
                  'w-2 h-2 rounded-full',
                  isConnected ? 'bg-accent' : 'bg-error'
                )}
              />
              <span className="text-text-muted text-[11px]">
                {isConnected ? 'Live' : 'Offline'}
              </span>
            </div>

            {/* Server Endpoint Switcher Pill */}
            <button
              onClick={() => setShowServerModal(true)}
              className="hidden lg:flex items-center gap-1.5 px-3 py-1.5 rounded-full bg-[#272727] hover:bg-[#333333] border border-white/[0.06] text-xs transition-colors cursor-pointer"
              title="Click to point dashboard to your own ESA server"
            >
              <span className="text-text-muted text-[11px]">Server:</span>
              <span className="text-accent font-mono text-[11px] max-w-[120px] truncate">
                {getApiBaseUrl() ? getApiBaseUrl().replace(/^https?:\/\//, '') : 'local:8080'}
              </span>
            </button>
          </div>
        </div>
      </header>

      {/* Dynamic Environment Ribbon: Stark Contrast between TEST / SANDBOX vs LIVE ENTERPRISE */}
      <div
        className={cn(
          'w-full border-b px-3 sm:px-6 md:px-12 py-2.5 flex flex-wrap items-center justify-between gap-3 text-xs transition-colors',
          isSandboxMode
            ? 'bg-[#18150D] border-amber-500/30 text-amber-200'
            : 'bg-[#070D1A] border-[#1F51FF]/30 text-slate-200'
        )}
      >
        <div className="flex items-center gap-3">
          <span
            className={cn(
              'px-3 py-1 rounded-full text-[10px] font-mono font-bold uppercase tracking-wider flex items-center gap-1.5 shrink-0 shadow-sm',
              isSandboxMode
                ? 'bg-amber-400/20 text-amber-300 border border-amber-400/50'
                : 'bg-emerald-400/20 text-emerald-300 border border-emerald-400/50'
            )}
          >
            <span
              className={cn(
                'h-2 w-2 rounded-full',
                isSandboxMode ? 'bg-amber-400 animate-ping' : 'bg-emerald-400 animate-pulse'
              )}
            />
            {isSandboxMode ? '🧪 TEST BENCH / ISOLATED SANDBOX' : '🟢 LIVE PRODUCTION MESH (ap-south-1)'}
          </span>

          <div className="hidden sm:flex items-center gap-2 text-xs">
            {isSandboxMode ? (
              <span className="flex items-center gap-2 text-amber-200/90 font-mono text-[11px]">
                <span>Zero-risk mock routing · Synthetic corridor injection active</span>
                {activeKey && (
                  <span className="bg-amber-400/10 text-amber-300 border border-amber-400/30 px-2 py-0.5 rounded text-[11px] flex items-center gap-1">
                    <span>Key: {activeKey.length > 20 ? `${activeKey.substring(0, 16)}...` : activeKey}</span>
                    <button
                      onClick={() => {
                        navigator.clipboard.writeText(activeKey);
                        setCopiedActiveKey(true);
                        setTimeout(() => setCopiedActiveKey(false), 2000);
                      }}
                      className="hover:text-white transition-colors cursor-pointer"
                      title="Copy active key"
                    >
                      {copiedActiveKey ? <Check className="h-3 w-3 text-emerald-400" /> : <Copy className="h-3 w-3" />}
                    </button>
                  </span>
                )}
              </span>
            ) : (
              <div className="flex items-center gap-2 font-mono text-[11px]">
                <span className="text-white font-bold">
                  {session?.user?.organization_name || 'My Enterprise Workspace'}
                </span>
                <span className="text-white/20">|</span>
                <span className="text-slate-400">{session?.user?.email}</span>
                <span className="hidden md:inline-flex items-center gap-1 px-2 py-0.2 rounded-full bg-emerald-500/10 text-emerald-400 text-[10px] border border-emerald-500/20">
                  <ShieldCheck className="h-3 w-3" />
                  PCI-DSS 4.0 Level 1
                </span>
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center gap-2 shrink-0">
          {isSandboxMode ? (
            <div className="flex items-center gap-2">
              <Link
                to="/signup"
                className="px-3.5 py-1.5 rounded-full bg-amber-400 hover:bg-amber-300 text-slate-950 font-bold text-xs font-mono transition-all shadow-md flex items-center gap-1.5 cursor-pointer"
              >
                <span>Provision Live Account</span>
                <ArrowRight className="h-3 w-3" />
              </Link>
              <Link
                to="/login"
                className="px-3 py-1.5 rounded-full bg-white/5 hover:bg-white/10 text-slate-300 hover:text-white font-mono text-xs transition-colors"
              >
                Sign In
              </Link>
            </div>
          ) : (
            <div className="flex items-center gap-2.5">
              <Link
                to="/keys"
                className={cn(
                  'px-3 py-1 rounded-full text-xs font-mono transition-all flex items-center gap-1.5',
                  location.pathname === '/keys'
                    ? 'bg-[#1F51FF] text-white shadow-md font-bold'
                    : 'bg-white/10 hover:bg-white/15 text-slate-300 hover:text-white'
                )}
                title="Manage Multiple Merchant API Keys & Expiration Schedules"
              >
                <Key className="h-3 w-3 text-blue-400" />
                <span>API Keys</span>
              </Link>

              <Link
                to={activeKey ? `/dashboard?mode=sandbox&key=${encodeURIComponent(activeKey)}` : '/dashboard?mode=sandbox'}
                className="px-3 py-1 rounded-full bg-amber-400/10 hover:bg-amber-400/20 border border-amber-400/30 text-amber-300 text-xs font-mono transition-colors flex items-center gap-1"
                title="Switch to Isolated Sandbox"
              >
                <Zap className="h-3 w-3 text-amber-400" />
                <span>Sandbox Mode</span>
              </Link>

              <button
                onClick={handleSafeLogout}
                disabled={isLoggingOut}
                className="px-3 py-1 rounded-full bg-rose-500/20 hover:bg-rose-500/30 text-rose-300 hover:text-rose-200 border border-rose-500/30 text-xs font-mono font-semibold transition-all flex items-center gap-1.5 cursor-pointer"
                title="Safely terminate session, purge tokens and clear sensitive keys"
              >
                {isLoggingOut ? (
                  <RefreshCw className="h-3 w-3 animate-spin" />
                ) : (
                  <LogOut className="h-3 w-3" />
                )}
                <span>Safe Logout</span>
              </button>
            </div>
          )}
        </div>
      </div>

      {/* Mobile navigation bar */}
      <div className="md:hidden flex items-center gap-1.5 border-b border-white/[0.06] bg-[#222222] px-3 py-2.5 overflow-x-auto no-scrollbar scroll-smooth">
        {visibleNavigation.map((item) => {
          const isActive =
            location.pathname === item.path ||
            (item.path === '/dashboard' && location.pathname === '/app');
          const targetUrl = isSandboxMode
            ? `${item.path}?mode=sandbox${activeKey ? `&key=${encodeURIComponent(activeKey)}` : ''}`
            : item.path;

          return (
            <Link
              key={item.path}
              to={targetUrl}
              className={cn(
                'px-3.5 py-1.5 rounded-full text-xs font-medium whitespace-nowrap transition-all flex-shrink-0',
                isActive
                  ? 'bg-accent text-[#1D1E1C] font-bold shadow-sm'
                  : 'text-text-secondary hover:text-white bg-[#2e2e2e]'
              )}
            >
              {item.name}
            </Link>
          );
        })}
      </div>

      {/* Main Content with responsive breathing room */}
      <main className="w-full max-w-[1952px] mx-auto px-3 sm:px-6 md:px-12 py-5 sm:py-8 md:py-10">
        {isRestricted ? (
          <div className="py-16 px-4 flex flex-col items-center justify-center text-center">
            <div className="max-w-md w-full bg-[#272727] border border-white/[0.08] rounded-3xl p-8 space-y-6 shadow-2xl">
              <div className="h-14 w-14 rounded-2xl bg-amber-500/10 border border-amber-500/30 flex items-center justify-center text-amber-400 mx-auto text-2xl font-mono">
                🔒
              </div>
              <div className="space-y-2">
                <h2 className="text-xl font-bold text-white">Merchant Account Required</h2>
                <p className="text-xs text-text-secondary leading-relaxed">
                  The <span className="font-mono text-amber-300 font-bold uppercase">{location.pathname.replace('/', '')}</span> subsystem (Production AI Deliberations, SHA-256 Audit Trail & Policy OCC) is restricted to authenticated merchant accounts.
                </p>
                <p className="text-[11px] text-text-muted">
                  Test mode provides full access to Dashboard vitals, Runtime pod scaling, Chaos effects, and Benchmarks.
                </p>
              </div>
              <div className="pt-2 flex flex-col sm:flex-row items-center justify-center gap-3">
                <Link
                  to="/signup"
                  className="w-full sm:w-auto px-5 py-2.5 rounded-full bg-accent hover:bg-accent/80 text-white font-bold text-xs shadow-md transition-all flex items-center justify-center gap-1.5 font-mono"
                >
                  <span>Provision Enterprise Account</span>
                  <span>&rarr;</span>
                </Link>
                <Link
                  to={activeKey ? `/dashboard?mode=sandbox&key=${encodeURIComponent(activeKey)}` : '/dashboard?mode=sandbox'}
                  className="w-full sm:w-auto px-4 py-2.5 rounded-full bg-white/5 hover:bg-white/10 text-text-secondary hover:text-white font-semibold text-xs transition-colors"
                >
                  Back to Test Dashboard
                </Link>
              </div>
            </div>
          </div>
        ) : (
          <Outlet />
        )}
      </main>

      {/* Server Switcher Modal */}
      {showServerModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/75 backdrop-blur-sm p-4">
          <div className="bg-[#1f1f1f] border border-white/10 rounded-2xl w-full max-w-lg p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
            <div className="flex items-center justify-between pb-4 border-b border-white/10">
              <div className="flex items-center gap-2">
                <span className="text-lg">⚡</span>
                <h3 className="font-bold text-white text-base">Connect to ESA Server</h3>
              </div>
              <button
                onClick={() => setShowServerModal(false)}
                className="text-text-muted hover:text-white text-sm px-2 py-1 rounded"
              >
                ✕
              </button>
            </div>

            <p className="text-xs text-text-secondary mt-3 leading-relaxed">
              Point this dashboard to your own <strong>Executable State Architecture (ESA)</strong> instance — whether running on localhost, in Kubernetes, or on a remote server.
            </p>

            <div className="mt-4 space-y-2">
              <label className="text-xs font-semibold text-text-secondary">ESA Server Base URL</label>
              <input
                type="text"
                value={customServerInput}
                onChange={(e) => setCustomServerInput(e.target.value)}
                placeholder="http://localhost:8080 or https://api.yourdomain.com"
                className="w-full px-4 py-2.5 bg-[#141414] border border-white/10 rounded-xl text-sm font-mono text-white placeholder-text-muted focus:outline-none focus:border-accent"
              />
            </div>

            <div className="flex items-center gap-2 mt-3 text-xs text-text-muted">
              <span>Quick Presets:</span>
              <button
                type="button"
                onClick={() => setCustomServerInput('http://localhost:8080')}
                className="px-2.5 py-1 rounded bg-[#2b2b2b] hover:bg-[#383838] text-white transition-colors"
              >
                localhost:8080
              </button>
              <button
                type="button"
                onClick={() => setCustomServerInput('')}
                className="px-2.5 py-1 rounded bg-[#2b2b2b] hover:bg-[#383838] text-white transition-colors"
              >
                Same-Origin (Default)
              </button>
            </div>

            <div className="flex items-center justify-end gap-3 mt-6 pt-4 border-t border-white/10">
              <button
                type="button"
                onClick={() => setShowServerModal(false)}
                className="px-4 py-2 rounded-xl text-xs font-medium text-text-secondary hover:text-white transition-colors"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  setApiBaseUrl(customServerInput);
                  setShowServerModal(false);
                  queryClient.invalidateQueries();
                }}
                className="px-5 py-2 rounded-xl bg-accent hover:bg-accent/90 text-black text-xs font-bold transition-all"
              >
                Connect & Save
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}


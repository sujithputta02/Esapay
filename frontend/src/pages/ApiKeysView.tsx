import { useState, useEffect } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import {
  Key,
  Plus,
  Copy,
  Check,
  AlertTriangle,
  Clock,
  Trash2,
  RefreshCw,
  Eye,
  EyeOff,
  Zap,
  CheckCircle2,
  X,
  Play,
} from 'lucide-react';
import { supabaseAuth, ApiKeyItem, getApiKeyStatusInfo, isApiKeyExpired } from '@/lib/supabase';
import { cn } from '@/lib/utils';

export function ApiKeysView() {
  const [keys, setKeys] = useState<ApiKeyItem[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [showWelcomeBanner, setShowWelcomeBanner] = useState(false);

  // New Key Modal state
  const [isCreateModalOpen, setIsCreateModalOpen] = useState(false);
  const [keyName, setKeyName] = useState('');
  const [keyEnv, setKeyEnv] = useState<'live' | 'test'>('live');
  const [expiryOption, setExpiryOption] = useState<'forever' | '7' | '30' | '90' | '365' | 'custom'>('90');
  const [customDays, setCustomDays] = useState('60');
  const [isGenerating, setIsGenerating] = useState(false);

  // Generated Secret Key Reveal Dialog state
  const [newlyCreatedKey, setNewlyCreatedKey] = useState<{ rawKey: string; item: ApiKeyItem } | null>(null);
  const [showRawSecret, setShowRawSecret] = useState(true);
  const [copiedRawSecret, setCopiedRawSecret] = useState(false);

  // Validation Test Playground state
  const [testSelectedKeyId, setTestSelectedKeyId] = useState<string>('');
  const [validationResult, setValidationResult] = useState<{
    status: 'success' | 'expired' | 'revoked' | 'error';
    message: string;
    details?: string;
  } | null>(null);
  const [isValidating, setIsValidating] = useState(false);

  const fetchKeys = async () => {
    setIsLoading(true);
    try {
      const data = await supabaseAuth.listApiKeys();
      setKeys(data);
      if (data.length > 0 && !testSelectedKeyId) {
        setTestSelectedKeyId(data[0].id);
      }
    } finally {
      setIsLoading(false);
    }
  };

  useEffect(() => {
    fetchKeys();
    if (typeof window !== 'undefined') {
      const params = new URLSearchParams(window.location.search);
      if (params.get('welcome') === '1') {
        setShowWelcomeBanner(true);
        setIsCreateModalOpen(true);
      }
    }
  }, []);

  const handleCreateKey = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!keyName.trim()) return;

    let days: number | null = null;
    if (expiryOption === 'forever') {
      days = null;
    } else if (expiryOption === 'custom') {
      const parsed = parseInt(customDays, 10);
      days = !isNaN(parsed) && parsed > 0 ? parsed : 30;
    } else {
      days = parseInt(expiryOption, 10);
    }

    setIsGenerating(true);
    try {
      const result = await supabaseAuth.createApiKey(keyName.trim(), keyEnv, days);
      setNewlyCreatedKey({ rawKey: result.key, item: result.item });
      setIsCreateModalOpen(false);
      setKeyName('');
      setExpiryOption('90');
      await fetchKeys();
      setTestSelectedKeyId(result.item.id);
    } finally {
      setIsGenerating(false);
    }
  };

  const handleRevokeKey = async (id: string) => {
    await supabaseAuth.revokeApiKey(id);
    await fetchKeys();
  };

  const handleDeleteKey = async (id: string) => {
    if (window.confirm('Are you sure you want to permanently delete this API key? This action cannot be undone.')) {
      await supabaseAuth.deleteApiKey(id);
      await fetchKeys();
      if (testSelectedKeyId === id) {
        setTestSelectedKeyId('');
        setValidationResult(null);
      }
    }
  };

  const handleTestKeyValidation = async () => {
    const targetKey = keys.find((k) => k.id === testSelectedKeyId);
    if (!targetKey) return;

    setIsValidating(true);
    setValidationResult(null);

    setTimeout(() => {
      setIsValidating(false);
      if (!targetKey.is_active) {
        setValidationResult({
          status: 'revoked',
          message: '403 Forbidden: API Key Revoked',
          details: 'This key has been deactivated by merchant policy. All routing requests are blocked.',
        });
        return;
      }

      if (isApiKeyExpired(targetKey)) {
        setValidationResult({
          status: 'expired',
          message: '401 Unauthorized: API Key Expired',
          details: `This key expired on ${new Date(targetKey.expires_at!).toLocaleDateString('en-IN', {
            day: 'numeric',
            month: 'short',
            year: 'numeric',
          })}. The autonomous gateway enforces strict credential expiration.`,
        });
        return;
      }

      // Valid key
      const statusInfo = getApiKeyStatusInfo(targetKey);
      setValidationResult({
        status: 'success',
        message: '200 OK: Valid Cryptographic Authorization',
        details: `Key authenticated for ${targetKey.environment.toUpperCase()} cluster. ${
          targetKey.expires_at
            ? `Expires in ${statusInfo.daysRemaining} days (${new Date(targetKey.expires_at).toLocaleDateString()}).`
            : 'Key is permanent (Never Expires).'
        }`,
      });
    }, 600);
  };

  const activeCount = keys.filter((k) => k.is_active && !isApiKeyExpired(k)).length;
  const expiredCount = keys.filter((k) => isApiKeyExpired(k)).length;
  const permanentCount = keys.filter((k) => k.expires_at === null && k.is_active).length;

  return (
    <div className="space-y-8 font-sans pb-16">
      {/* Top Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-4 border-b border-white/[0.08] pb-6">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <div className="p-1.5 rounded-lg bg-[#1F51FF]/20 text-[#60A5FA] border border-[#1F51FF]/30">
              <Key className="h-4 w-4" />
            </div>
            <h1 className="text-2xl font-bold tracking-tight text-white">
              Enterprise API Key Management
            </h1>
            <span className="px-2.5 py-0.5 rounded-full text-[10px] font-mono font-bold bg-emerald-500/15 text-emerald-400 border border-emerald-500/30">
              SHA-256 HSM Enforced
            </span>
          </div>
          <p className="text-xs sm:text-sm text-text-secondary max-w-2xl font-sans">
            Provision and configure multiple API secrets for backend services, microservices, and CLI pipelines. Configure custom expiration schedules or permanent keys with automated cryptographic expiry.
          </p>
        </div>

        <button
          onClick={() => setIsCreateModalOpen(true)}
          className="px-4 py-2.5 rounded-xl bg-gradient-to-r from-[#1F51FF] via-[#2A5CFF] to-[#1644DF] hover:brightness-110 text-white font-bold text-xs flex items-center justify-center gap-2 shadow-lg shadow-[#1F51FF]/25 transition-all cursor-pointer font-mono shrink-0 border border-white/20"
        >
          <Plus className="h-4 w-4" />
          <span>Generate New API Key</span>
        </button>
      </div>

      {/* Welcome Banner for newly signed-up accounts */}
      <AnimatePresence>
        {showWelcomeBanner && (
          <motion.div
            initial={{ opacity: 0, y: -10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -10 }}
            className="p-4 rounded-2xl bg-gradient-to-r from-emerald-500/10 via-[#1F51FF]/10 to-transparent border border-emerald-500/30 flex items-start sm:items-center justify-between gap-4"
          >
            <div className="flex items-start sm:items-center gap-3">
              <div className="p-2 rounded-xl bg-emerald-500/20 text-emerald-400 shrink-0">
                <CheckCircle2 className="h-5 w-5" />
              </div>
              <div>
                <h3 className="text-sm font-bold text-white flex items-center gap-2">
                  Welcome to ESA Command Center
                  <span className="text-[10px] font-mono px-2 py-0.5 rounded-full bg-emerald-500/20 text-emerald-300 border border-emerald-500/40">
                    Live Entity Initialized
                  </span>
                </h3>
                <p className="text-xs text-text-secondary mt-0.5">
                  Your enterprise account is ready. Use the API Key generator below to provision credentials with customized active durations (e.g. 7 days, 30 days, 90 days, or permanent/forever).
                </p>
              </div>
            </div>
            <button
              onClick={() => setShowWelcomeBanner(false)}
              className="p-1 rounded-lg hover:bg-white/10 text-slate-400 hover:text-white transition-colors"
            >
              <X className="h-4 w-4" />
            </button>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Metrics Cards */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 font-mono">
        <div className="p-4 rounded-2xl bg-[#1A1D24] border border-white/[0.08] space-y-1">
          <span className="text-[11px] text-text-muted uppercase tracking-wider">Total Credentials</span>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-extrabold text-white">{keys.length}</span>
            <span className="text-xs text-slate-400">keys provisioned</span>
          </div>
        </div>

        <div className="p-4 rounded-2xl bg-[#1A1D24] border border-emerald-500/20 space-y-1">
          <span className="text-[11px] text-emerald-400 uppercase tracking-wider flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-full bg-emerald-400 animate-pulse" />
            Active Usable Keys
          </span>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-extrabold text-emerald-400">{activeCount}</span>
            <span className="text-xs text-text-muted">ready for traffic</span>
          </div>
        </div>

        <div className="p-4 rounded-2xl bg-[#1A1D24] border border-blue-500/20 space-y-1">
          <span className="text-[11px] text-blue-400 uppercase tracking-wider">Permanent Keys (Forever)</span>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-extrabold text-blue-400">{permanentCount}</span>
            <span className="text-xs text-text-muted">no expiry limit</span>
          </div>
        </div>

        <div className="p-4 rounded-2xl bg-[#1A1D24] border border-rose-500/20 space-y-1">
          <span className="text-[11px] text-rose-400 uppercase tracking-wider flex items-center gap-1.5">
            <Clock className="h-3 w-3 text-rose-400" />
            Expired Keys
          </span>
          <div className="flex items-baseline gap-2">
            <span className="text-2xl font-extrabold text-rose-400">{expiredCount}</span>
            <span className="text-xs text-rose-400/80">auto-rejected</span>
          </div>
        </div>
      </div>

      {/* Key List Table */}
      <div className="rounded-3xl bg-[#1A1D24] border border-white/[0.08] overflow-hidden shadow-2xl">
        <div className="px-6 py-4 border-b border-white/[0.08] bg-[#14171E] flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <h3 className="font-bold text-sm text-white">Configured Gateway Secrets</h3>
            <span className="text-xs font-mono text-text-muted">({keys.length} items)</span>
          </div>
          <button
            onClick={fetchKeys}
            className="text-xs font-mono text-slate-400 hover:text-white flex items-center gap-1.5 transition-colors self-start sm:self-auto cursor-pointer"
          >
            <RefreshCw className={cn('h-3.5 w-3.5', isLoading && 'animate-spin')} />
            <span>Refresh Key Registry</span>
          </button>
        </div>

        {keys.length === 0 ? (
          <div className="p-12 text-center space-y-4">
            <div className="h-12 w-12 rounded-2xl bg-white/5 border border-white/10 flex items-center justify-center text-slate-400 mx-auto">
              <Key className="h-6 w-6" />
            </div>
            <div className="space-y-1">
              <h4 className="font-bold text-white text-base">No API Keys Found</h4>
              <p className="text-xs text-text-muted max-w-sm mx-auto">
                Generate your first production or test API key to connect your application backend with ESAPay.
              </p>
            </div>
            <button
              onClick={() => setIsCreateModalOpen(true)}
              className="px-4 py-2 rounded-xl bg-[#1F51FF] hover:bg-[#1644DF] text-white text-xs font-bold font-mono transition-all cursor-pointer"
            >
              + Create First Key
            </button>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full text-left font-mono text-xs">
              <thead className="bg-[#12151C] text-text-muted uppercase text-[10px] tracking-wider border-b border-white/[0.06]">
                <tr>
                  <th className="py-3 px-6">Key Name & Environment</th>
                  <th className="py-3 px-6">Key Identifier (Prefix)</th>
                  <th className="py-3 px-6">Created Date</th>
                  <th className="py-3 px-6">Expiration Schedule</th>
                  <th className="py-3 px-6">Status</th>
                  <th className="py-3 px-6 text-right">Actions</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/[0.04]">
                {keys.map((keyItem) => {
                  const statusInfo = getApiKeyStatusInfo(keyItem);
                  const isExpired = isApiKeyExpired(keyItem);

                  return (
                    <tr
                      key={keyItem.id}
                      className={cn(
                        'hover:bg-white/[0.02] transition-colors',
                        isExpired && 'bg-rose-500/[0.03]'
                      )}
                    >
                      {/* Name & Env */}
                      <td className="py-4 px-6 font-sans">
                        <div className="flex items-center gap-2.5">
                          <span
                            className={cn(
                              'px-2 py-0.5 rounded text-[10px] font-mono font-bold uppercase shrink-0',
                              keyItem.environment === 'live'
                                ? 'bg-emerald-500/15 text-emerald-400 border border-emerald-500/30'
                                : 'bg-amber-500/15 text-amber-400 border border-amber-500/30'
                            )}
                          >
                            {keyItem.environment}
                          </span>
                          <div>
                            <div className="font-bold text-white text-xs sm:text-sm">{keyItem.name}</div>
                            <div className="text-[10px] text-text-muted font-mono">
                              ID: {keyItem.id.substring(0, 14)}
                            </div>
                          </div>
                        </div>
                      </td>

                      {/* Key Prefix */}
                      <td className="py-4 px-6">
                        <div className="flex items-center gap-2 bg-[#12151C] px-2.5 py-1.5 rounded-lg border border-white/[0.06] w-fit">
                          <span className="text-slate-300 font-mono text-[11px] select-all">
                            {keyItem.key_prefix}
                          </span>
                          <button
                            onClick={() => {
                              navigator.clipboard.writeText(keyItem.key_prefix);
                              setCopiedId(keyItem.id);
                              setTimeout(() => setCopiedId(null), 2000);
                            }}
                            className="text-slate-400 hover:text-white transition-colors cursor-pointer"
                            title="Copy key prefix"
                          >
                            {copiedId === keyItem.id ? (
                              <Check className="h-3.5 w-3.5 text-emerald-400" />
                            ) : (
                              <Copy className="h-3.5 w-3.5" />
                            )}
                          </button>
                        </div>
                      </td>

                      {/* Created At */}
                      <td className="py-4 px-6 text-text-secondary text-[11px]">
                        {new Date(keyItem.created_at).toLocaleDateString('en-IN', {
                          day: 'numeric',
                          month: 'short',
                          year: 'numeric',
                        })}
                      </td>

                      {/* Expiration Schedule */}
                      <td className="py-4 px-6">
                        {keyItem.expires_at ? (
                          <div className="space-y-0.5">
                            <div
                              className={cn(
                                'text-[11px] font-bold flex items-center gap-1',
                                isExpired ? 'text-rose-400' : 'text-slate-200'
                              )}
                            >
                              <Clock className="h-3 w-3" />
                              <span>
                                {new Date(keyItem.expires_at).toLocaleDateString('en-IN', {
                                  day: 'numeric',
                                  month: 'short',
                                  year: 'numeric',
                                })}
                              </span>
                            </div>
                            <span
                              className={cn(
                                'text-[10px]',
                                isExpired ? 'text-rose-400 font-semibold' : 'text-text-muted'
                              )}
                            >
                              {isExpired ? '⚠️ Expired — Gateway calls blocked' : statusInfo.label}
                            </span>
                          </div>
                        ) : (
                          <span className="inline-flex items-center gap-1 text-blue-400 text-[11px] font-bold bg-blue-500/10 px-2 py-0.5 rounded-full border border-blue-500/20">
                            <span>♾️ Never Expires (Permanent)</span>
                          </span>
                        )}
                      </td>

                      {/* Status Badge */}
                      <td className="py-4 px-6">
                        <span
                          className={cn(
                            'inline-flex items-center gap-1 px-2.5 py-1 rounded-full text-[10px] font-bold border',
                            statusInfo.badgeClass
                          )}
                        >
                          <span
                            className={cn(
                              'h-1.5 w-1.5 rounded-full',
                              statusInfo.status === 'expired'
                                ? 'bg-rose-400'
                                : statusInfo.status === 'active'
                                ? 'bg-emerald-400 animate-pulse'
                                : statusInfo.status === 'forever'
                                ? 'bg-blue-400'
                                : 'bg-slate-400'
                            )}
                          />
                          {statusInfo.status.toUpperCase()}
                        </span>
                      </td>

                      {/* Actions */}
                      <td className="py-4 px-6 text-right">
                        <div className="flex items-center justify-end gap-2">
                          <button
                            onClick={() => handleRevokeKey(keyItem.id)}
                            className={cn(
                              'px-2.5 py-1 rounded-lg text-[10px] font-bold transition-colors cursor-pointer border',
                              keyItem.is_active
                                ? 'bg-white/5 hover:bg-white/10 text-slate-300 border-white/10'
                                : 'bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border-emerald-500/30'
                            )}
                            title={keyItem.is_active ? 'Deactivate key' : 'Reactivate key'}
                          >
                            {keyItem.is_active ? 'Pause' : 'Activate'}
                          </button>

                          <button
                            onClick={() => handleDeleteKey(keyItem.id)}
                            className="p-1.5 rounded-lg bg-rose-500/10 hover:bg-rose-500/20 text-rose-400 border border-rose-500/30 transition-colors cursor-pointer"
                            title="Delete API Key"
                          >
                            <Trash2 className="h-3.5 w-3.5" />
                          </button>
                        </div>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

      {/* Validation Playground: Prove Expiration Works */}
      <div className="p-6 rounded-3xl bg-[#14171E] border border-white/[0.08] space-y-4 font-mono">
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <Zap className="h-4 w-4 text-amber-400" />
            <h3 className="font-bold text-sm text-white">Live Key Expiration & Access Authenticator</h3>
          </div>
          <span className="text-[11px] text-text-muted">
            Directly test how gateway requests respond when keys expire
          </span>
        </div>

        <div className="flex flex-col sm:flex-row items-center gap-3">
          <select
            value={testSelectedKeyId}
            onChange={(e) => {
              setTestSelectedKeyId(e.target.value);
              setValidationResult(null);
            }}
            className="w-full sm:w-80 px-3.5 py-2.5 rounded-xl border border-white/[0.12] bg-[#1A1D24] text-white text-xs focus:outline-none focus:ring-2 focus:ring-[#1F51FF]"
          >
            {keys.map((k) => (
              <option key={k.id} value={k.id}>
                {k.name} [{k.environment.toUpperCase()}] {isApiKeyExpired(k) ? '(EXPIRED)' : ''}
              </option>
            ))}
          </select>

          <button
            onClick={handleTestKeyValidation}
            disabled={isValidating || !testSelectedKeyId}
            className="w-full sm:w-auto px-5 py-2.5 rounded-xl bg-slate-800 hover:bg-slate-700 text-white font-bold text-xs flex items-center justify-center gap-2 border border-white/10 transition-colors cursor-pointer shrink-0"
          >
            {isValidating ? (
              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Play className="h-3.5 w-3.5 text-emerald-400" />
            )}
            <span>Simulate API Call with Key</span>
          </button>
        </div>

        {validationResult && (
          <motion.div
            initial={{ opacity: 0, y: 5 }}
            animate={{ opacity: 1, y: 0 }}
            className={cn(
              'p-4 rounded-2xl border text-xs space-y-1',
              validationResult.status === 'success'
                ? 'bg-emerald-500/10 border-emerald-500/30 text-emerald-300'
                : 'bg-rose-500/10 border-rose-500/30 text-rose-300'
            )}
          >
            <div className="flex items-center gap-2 font-bold text-sm">
              {validationResult.status === 'success' ? (
                <CheckCircle2 className="h-4 w-4 text-emerald-400" />
              ) : (
                <AlertTriangle className="h-4 w-4 text-rose-400" />
              )}
              <span>{validationResult.message}</span>
            </div>
            {validationResult.details && (
              <p className="text-[11px] leading-relaxed opacity-90">{validationResult.details}</p>
            )}
          </motion.div>
        )}
      </div>

      {/* CREATE API KEY MODAL */}
      <AnimatePresence>
        {isCreateModalOpen && (
          <div className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-4 bg-black/85 backdrop-blur-md">
            <motion.div
              initial={{ opacity: 0, scale: 0.95, y: 15 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.95, y: 15 }}
              className="w-full max-w-lg bg-[#0E1322] border border-white/[0.12] rounded-3xl p-6 sm:p-8 shadow-2xl text-white space-y-6 relative overflow-hidden"
            >
              <div className="flex items-start justify-between">
                <div>
                  <h3 className="font-extrabold text-lg sm:text-xl text-white flex items-center gap-2">
                    <Key className="h-5 w-5 text-[#1F51FF]" />
                    <span>Generate New API Key</span>
                  </h3>
                  <p className="text-xs text-text-secondary mt-0.5">
                    Generate an HMAC/SHA-256 merchant credential with automated expiration.
                  </p>
                </div>
                <button
                  onClick={() => setIsCreateModalOpen(false)}
                  className="h-8 w-8 rounded-full bg-white/5 hover:bg-white/10 text-slate-400 hover:text-white flex items-center justify-center transition-colors cursor-pointer"
                >
                  <X className="h-4 w-4" />
                </button>
              </div>

              <form onSubmit={handleCreateKey} className="space-y-4 font-mono text-xs">
                {/* Key Name */}
                <div className="space-y-1.5">
                  <label className="text-[11px] text-text-muted uppercase tracking-wider block">
                    Key Purpose / Name
                  </label>
                  <input
                    type="text"
                    required
                    value={keyName}
                    onChange={(e) => setKeyName(e.target.value)}
                    placeholder="e.g. Production Core Gateway, Mobile App SDK"
                    className="w-full px-4 py-2.5 rounded-xl border border-white/[0.12] bg-[#141724] text-white text-xs focus:outline-none focus:ring-2 focus:ring-[#1F51FF]"
                  />
                </div>

                {/* Environment Selector */}
                <div className="space-y-1.5">
                  <label className="text-[11px] text-text-muted uppercase tracking-wider block">
                    Environment Mesh
                  </label>
                  <div className="grid grid-cols-2 gap-2">
                    <button
                      type="button"
                      onClick={() => setKeyEnv('live')}
                      className={cn(
                        'py-2.5 px-4 rounded-xl border font-bold text-center transition-all cursor-pointer flex items-center justify-center gap-2',
                        keyEnv === 'live'
                          ? 'bg-emerald-500/15 border-emerald-500/50 text-emerald-400 shadow-md'
                          : 'bg-white/5 border-white/10 text-slate-400 hover:text-white'
                      )}
                    >
                      <span className="h-2 w-2 rounded-full bg-emerald-400" />
                      <span>Live Production (`esa_live_`)</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => setKeyEnv('test')}
                      className={cn(
                        'py-2.5 px-4 rounded-xl border font-bold text-center transition-all cursor-pointer flex items-center justify-center gap-2',
                        keyEnv === 'test'
                          ? 'bg-amber-500/15 border-amber-500/50 text-amber-400 shadow-md'
                          : 'bg-white/5 border-white/10 text-slate-400 hover:text-white'
                      )}
                    >
                      <span className="h-2 w-2 rounded-full bg-amber-400" />
                      <span>Sandbox / Test (`esa_test_`)</span>
                    </button>
                  </div>
                </div>

                {/* Expiration Options */}
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <label className="text-[11px] text-text-muted uppercase tracking-wider block">
                      Validity & Expiration Schedule
                    </label>
                    <span className="text-[10px] text-emerald-400">Enforced by Gateway</span>
                  </div>

                  <div className="grid grid-cols-3 gap-2">
                    {[
                      { id: '7', label: '7 Days' },
                      { id: '30', label: '30 Days' },
                      { id: '90', label: '90 Days' },
                      { id: '365', label: '365 Days' },
                      { id: 'custom', label: 'Custom Days' },
                      { id: 'forever', label: '♾️ Forever' },
                    ].map((opt) => (
                      <button
                        key={opt.id}
                        type="button"
                        onClick={() => setExpiryOption(opt.id as any)}
                        className={cn(
                          'py-2 px-3 rounded-xl border text-[11px] font-bold transition-all cursor-pointer text-center',
                          expiryOption === opt.id
                            ? 'bg-[#1F51FF] text-white border-[#1F51FF] shadow-sm'
                            : 'bg-white/5 border-white/10 text-slate-400 hover:text-white'
                        )}
                      >
                        {opt.label}
                      </button>
                    ))}
                  </div>

                  {expiryOption === 'custom' && (
                    <div className="pt-1.5 flex items-center gap-2">
                      <input
                        type="number"
                        min="1"
                        max="3650"
                        value={customDays}
                        onChange={(e) => setCustomDays(e.target.value)}
                        placeholder="Number of days"
                        className="w-32 px-3 py-2 rounded-xl border border-white/[0.12] bg-[#141724] text-white text-xs focus:outline-none focus:ring-2 focus:ring-[#1F51FF]"
                      />
                      <span className="text-slate-400 text-xs">
                        days until automatic expiration
                      </span>
                    </div>
                  )}

                  <div className="p-3 rounded-xl bg-white/[0.03] border border-white/[0.06] text-[11px] text-slate-400">
                    {expiryOption === 'forever' ? (
                      <span className="text-blue-300 font-bold">
                        ♾️ Key will never expire. Recommended only for permanent backend services.
                      </span>
                    ) : (
                      <span>
                        Key will expire automatically on:{' '}
                        <strong className="text-white">
                          {new Date(
                            Date.now() +
                              (expiryOption === 'custom'
                                ? parseInt(customDays || '30', 10)
                                : parseInt(expiryOption, 10)) *
                                86400 *
                                1000
                          ).toLocaleDateString('en-IN', {
                            day: 'numeric',
                            month: 'long',
                            year: 'numeric',
                          })}
                        </strong>
                      </span>
                    )}
                  </div>
                </div>

                <div className="pt-2">
                  <button
                    type="submit"
                    disabled={isGenerating || !keyName.trim()}
                    className="w-full py-3.5 rounded-2xl bg-gradient-to-r from-[#1F51FF] via-[#2A5CFF] to-[#1644DF] hover:brightness-110 text-white font-bold text-xs flex items-center justify-center gap-2 shadow-lg shadow-[#1F51FF]/30 transition-all cursor-pointer border border-white/20"
                  >
                    {isGenerating ? (
                      <RefreshCw className="h-4 w-4 animate-spin" />
                    ) : (
                      <Check className="h-4 w-4" />
                    )}
                    <span>Generate & Activate API Key</span>
                  </button>
                </div>
              </form>
            </motion.div>
          </div>
        )}
      </AnimatePresence>

      {/* ONE-TIME SECRET KEY REVEAL MODAL */}
      <AnimatePresence>
        {newlyCreatedKey && (
          <div className="fixed inset-0 z-50 flex items-center justify-center p-3 sm:p-4 bg-black/90 backdrop-blur-md">
            <motion.div
              initial={{ opacity: 0, scale: 0.95, y: 15 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.95, y: 15 }}
              className="w-full max-w-lg bg-[#0B0F19] border border-emerald-500/40 rounded-3xl p-6 sm:p-8 shadow-2xl text-white space-y-6 relative"
            >
              <div className="flex items-center gap-3">
                <div className="h-10 w-10 rounded-2xl bg-emerald-500/20 text-emerald-400 flex items-center justify-center border border-emerald-500/30">
                  <CheckCircle2 className="h-5 w-5" />
                </div>
                <div>
                  <h3 className="font-extrabold text-lg text-white">API Key Successfully Created</h3>
                  <p className="text-xs text-emerald-400/90 font-mono">
                    {newlyCreatedKey.item.name} [{newlyCreatedKey.item.environment.toUpperCase()}]
                  </p>
                </div>
              </div>

              <div className="p-4 rounded-2xl bg-amber-500/10 border border-amber-500/30 text-amber-300 text-xs font-mono space-y-1">
                <div className="flex items-center gap-2 font-bold">
                  <AlertTriangle className="h-4 w-4 text-amber-400 shrink-0" />
                  <span>Important Security Notice</span>
                </div>
                <p className="text-[11px] leading-relaxed text-amber-200/90">
                  Please copy and store this API secret immediately. For security reasons, ESAPay does not store raw secrets and you will not be able to view it again.
                </p>
              </div>

              {/* Secret display box */}
              <div className="space-y-2 font-mono">
                <label className="text-[10px] text-slate-400 uppercase tracking-wider block">
                  Your Full Secret API Key
                </label>
                <div className="p-3.5 rounded-2xl bg-[#06080E] border border-white/10 flex items-center justify-between gap-3">
                  <span className="text-emerald-400 font-mono text-xs select-all break-all">
                    {showRawSecret
                      ? newlyCreatedKey.rawKey
                      : '••••••••••••••••••••••••••••••••••••••••••••••••'}
                  </span>
                  <div className="flex items-center gap-1.5 shrink-0">
                    <button
                      onClick={() => setShowRawSecret(!showRawSecret)}
                      className="p-1.5 rounded-lg text-slate-400 hover:text-white transition-colors cursor-pointer"
                      title={showRawSecret ? 'Hide secret' : 'Show secret'}
                    >
                      {showRawSecret ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
                    </button>
                    <button
                      onClick={() => {
                        navigator.clipboard.writeText(newlyCreatedKey.rawKey);
                        setCopiedRawSecret(true);
                        setTimeout(() => setCopiedRawSecret(false), 2500);
                      }}
                      className="px-3 py-1.5 rounded-xl bg-emerald-500 hover:bg-emerald-400 text-slate-950 font-bold text-xs flex items-center gap-1.5 transition-all cursor-pointer"
                    >
                      {copiedRawSecret ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
                      <span>{copiedRawSecret ? 'Copied!' : 'Copy Key'}</span>
                    </button>
                  </div>
                </div>
              </div>

              {/* Quick CLI usage */}
              <div className="p-3.5 rounded-2xl bg-[#141724] border border-white/[0.08] text-[11px] font-mono space-y-1 text-slate-300">
                <span className="text-slate-400 block text-[10px] uppercase">Quick CLI Test Command:</span>
                <code>npx esapay-cli --key {newlyCreatedKey.rawKey.substring(0, 16)}... status</code>
              </div>

              <button
                onClick={() => setNewlyCreatedKey(null)}
                className="w-full py-3 rounded-2xl bg-white/10 hover:bg-white/15 text-white font-bold text-xs font-mono transition-colors cursor-pointer"
              >
                I have securely saved my key
              </button>
            </motion.div>
          </div>
        )}
      </AnimatePresence>
    </div>
  );
}

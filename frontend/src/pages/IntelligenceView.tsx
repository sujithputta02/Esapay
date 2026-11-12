import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { apiClient } from '../lib/api';
import type {
  PaymentAttemptSummary,
  PaymentIntelligenceSummaryResponse,
  PaymentInvestigationResponse,
  PaymentMemoryRecord,
  PaymentMemoryRecentResponse,
} from '../types';

export default function IntelligenceView() {
  const [selectedTx, setSelectedTx] = useState<string | null>(null);
  const [question, setQuestion] = useState<string>('');
  const [memoryFilter, setMemoryFilter] = useState<string>('');
  const [investigation, setInvestigation] = useState<PaymentInvestigationResponse | null>(null);
  const [investigateError, setInvestigateError] = useState<string | null>(null);
  const [investigating, setInvestigating] = useState(false);

  const { data: summary, isLoading: summaryLoading } = useQuery<PaymentIntelligenceSummaryResponse>({
    queryKey: ['intelligence-summary'],
    queryFn: () => apiClient.getPaymentIntelligenceSummary(50),
    refetchInterval: 15000,
  });

  const { data: providersHealth } = useQuery<any>({
    queryKey: ['intelligence-providers-health'],
    queryFn: () => apiClient.getIntelligenceProvidersHealth(),
    refetchInterval: 15000,
  });

  const { data: memoryData, isLoading: memoryLoading } = useQuery<PaymentMemoryRecentResponse>({
    queryKey: ['intelligence-memory', memoryFilter],
    queryFn: () => apiClient.getRecentPaymentMemory(memoryFilter || undefined, 20),
    refetchInterval: 20000,
  });

  const handleInvestigate = async (attempt: PaymentAttemptSummary) => {
    setSelectedTx(attempt.transaction_id);
    setInvestigateError(null);
    setInvestigating(true);
    try {
      const resp = await apiClient.investigatePayment({
        transaction_id: attempt.transaction_id,
        question: question.trim() || undefined,
      });
      setInvestigation(resp);
    } catch (err: any) {
      setInvestigateError(err?.message || 'Failed to investigate payment');
    } finally {
      setInvestigating(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold text-slate-900 dark:text-white">
            Payment Intelligence &amp; Memory
          </h1>
          <p className="text-sm text-slate-600 dark:text-slate-400">
            Read-only, tenant-isolated, evidence-grounded payment diagnosis and persistent operational memory.
          </p>
        </div>
        <div className="flex items-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-1.5 text-xs font-medium text-emerald-700 dark:text-emerald-300">
          <span>Read-Only Advisory Mode</span>
          <span>•</span>
          <span>
            {summary?.inference_provider ?? 'ollama'} ({summary?.model_id ?? 'default'})
          </span>
        </div>
      </div>

      {/* Summary Metrics */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <div className="text-xs font-medium uppercase tracking-wider text-slate-500">
            Merchant Scope
          </div>
          <div className="mt-1 truncate text-lg font-semibold text-slate-900 dark:text-white">
            {summary?.merchant_id ?? '—'}
          </div>
        </div>
        <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <div className="text-xs font-medium uppercase tracking-wider text-slate-500">
            Recent Attempts
          </div>
          <div className="mt-1 text-2xl font-bold text-slate-900 dark:text-white">
            {summaryLoading ? '...' : summary?.total_attempts ?? 0}
          </div>
        </div>
        <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <div className="text-xs font-medium uppercase tracking-wider text-slate-500">
            Active Gateways
          </div>
          <div className="mt-1 text-2xl font-bold text-slate-900 dark:text-white">
            {providersHealth?.providers?.length ?? 0}
          </div>
        </div>
        <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <div className="text-xs font-medium uppercase tracking-wider text-slate-500">
            Memory Entries
          </div>
          <div className="mt-1 text-2xl font-bold text-slate-900 dark:text-white">
            {memoryLoading ? '...' : memoryData?.count ?? 0}
          </div>
        </div>
      </div>

      {/* Operator Question Input */}
      <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
        <label className="block text-xs font-medium uppercase tracking-wider text-slate-500">
          Optional Investigation Question (Read-Only)
        </label>
        <input
          type="text"
          value={question}
          onChange={(e) => setQuestion(e.target.value)}
          placeholder="e.g., Why is this payment still authorized without capture?"
          className="mt-2 w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 focus:border-indigo-500 focus:outline-none dark:border-slate-700 dark:bg-slate-950 dark:text-white"
        />
      </div>

      {/* Main Split: Recent Attempts + Investigation Report */}
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
        <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <h2 className="text-base font-semibold text-slate-900 dark:text-white">
            Recent Payment Attempts
          </h2>
          <div className="mt-3 overflow-x-auto">
            <table className="min-w-full divide-y divide-slate-200 text-sm dark:divide-slate-800">
              <thead>
                <tr className="text-left text-xs font-medium uppercase text-slate-500">
                  <th className="py-2 pr-3">Transaction</th>
                  <th className="py-2 pr-3">Provider</th>
                  <th className="py-2 pr-3">Amount</th>
                  <th className="py-2 pr-3">State</th>
                  <th className="py-2">Action</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 dark:divide-slate-800/60">
                {(summary?.recent_attempts ?? []).map((att) => (
                  <tr
                    key={att.transaction_id}
                    className={
                      selectedTx === att.transaction_id
                        ? 'bg-indigo-50/70 dark:bg-indigo-950/30'
                        : ''
                    }
                  >
                    <td className="py-2 pr-3 font-mono text-xs text-slate-800 dark:text-slate-200">
                      {att.transaction_id}
                    </td>
                    <td className="py-2 pr-3 capitalize text-slate-700 dark:text-slate-300">
                      {att.provider}
                    </td>
                    <td className="py-2 pr-3 text-slate-700 dark:text-slate-300">
                      {(att.amount_minor / 100).toFixed(2)} {att.currency}
                    </td>
                    <td className="py-2 pr-3">
                      <span className="rounded bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-700 dark:bg-slate-800 dark:text-slate-300">
                        {att.state}
                      </span>
                    </td>
                    <td className="py-2">
                      <button
                        type="button"
                        onClick={() => handleInvestigate(att)}
                        disabled={investigating}
                        className="rounded-md bg-indigo-600 px-2.5 py-1 text-xs font-medium text-white hover:bg-indigo-700 disabled:opacity-50"
                      >
                        Investigate
                      </button>
                    </td>
                  </tr>
                ))}
                {(summary?.recent_attempts?.length ?? 0) === 0 && (
                  <tr>
                    <td colSpan={5} className="py-6 text-center text-slate-500">
                      No payment attempts recorded for this merchant yet.
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
          </div>
        </div>

        <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
          <h2 className="text-base font-semibold text-slate-900 dark:text-white">
            Evidence-Grounded Investigation Report
          </h2>
          {investigateError && (
            <div className="mt-3 rounded-lg border border-red-300 bg-red-50 p-3 text-sm text-red-700 dark:border-red-800 dark:bg-red-950/40 dark:text-red-300">
              {investigateError}
            </div>
          )}
          {!investigation && !investigateError && (
            <p className="mt-4 text-sm text-slate-500">
              Select any payment attempt on the left to run a read-only, evidence-grounded investigation.
            </p>
          )}
          {investigation && (
            <div className="mt-3 space-y-3 text-sm">
              <div className="flex flex-wrap items-center gap-2">
                <span className="rounded bg-indigo-100 px-2 py-0.5 text-xs font-semibold text-indigo-800 dark:bg-indigo-950 dark:text-indigo-300">
                  {investigation.report.probable_root_cause}
                </span>
                <span className="rounded bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-700 dark:bg-slate-800 dark:text-slate-300">
                  State: {investigation.report.authoritative_payment_state}
                </span>
                <span className="rounded bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-700 dark:bg-slate-800 dark:text-slate-300">
                  Confidence: {(investigation.report.confidence * 100).toFixed(0)}%
                </span>
                {investigation.report.deterministic_fallback_used && (
                  <span className="rounded bg-amber-100 px-2 py-0.5 text-xs font-medium text-amber-800 dark:bg-amber-950 dark:text-amber-300">
                    Deterministic Fallback
                  </span>
                )}
              </div>

              <p className="text-slate-800 dark:text-slate-200">
                {investigation.report.issue_summary}
              </p>

              <div>
                <div className="text-xs font-semibold uppercase text-slate-500">
                  Supporting Evidence Refs
                </div>
                <div className="mt-1 flex flex-wrap gap-1.5">
                  {investigation.report.supporting_evidence_refs.map((refId) => (
                    <span
                      key={refId}
                      className="rounded border border-slate-200 bg-slate-50 px-2 py-0.5 font-mono text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-300"
                    >
                      {refId}
                    </span>
                  ))}
                </div>
              </div>

              {investigation.report.uncertainty_notes.length > 0 && (
                <div>
                  <div className="text-xs font-semibold uppercase text-slate-500">
                    Uncertainty &amp; Missing Data
                  </div>
                  <ul className="mt-1 list-disc pl-5 text-xs text-amber-700 dark:text-amber-300">
                    {investigation.report.uncertainty_notes.map((u, idx) => (
                      <li key={idx}>{u}</li>
                    ))}
                  </ul>
                </div>
              )}

              <div>
                <div className="text-xs font-semibold uppercase text-slate-500">
                  Recommended Read-Only Next Steps
                </div>
                <ul className="mt-1 list-disc pl-5 text-xs text-slate-700 dark:text-slate-300">
                  {investigation.report.recommended_next_investigation.map((s, idx) => (
                    <li key={idx}>{s}</li>
                  ))}
                </ul>
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Persistent Memory Browser */}
      <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-800 dark:bg-slate-900">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-base font-semibold text-slate-900 dark:text-white">
            Persistent Payment Memory
          </h2>
          <select
            value={memoryFilter}
            onChange={(e) => setMemoryFilter(e.target.value)}
            className="rounded-lg border border-slate-300 bg-white px-2.5 py-1 text-xs text-slate-800 dark:border-slate-700 dark:bg-slate-950 dark:text-slate-200"
          >
            <option value="">All Memory Types</option>
            <option value="episodic">Episodic (Merchant Verified)</option>
            <option value="semantic">Semantic (System Curated)</option>
            <option value="procedural">Procedural (Read-Only Workflows)</option>
          </select>
        </div>

        <div className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2">
          {(memoryData?.records ?? []).map((mem: PaymentMemoryRecord) => (
            <div
              key={mem.memory_id}
              className="rounded-lg border border-slate-200 p-3 dark:border-slate-800"
            >
              <div className="flex items-center justify-between gap-2 text-xs">
                <span className="rounded bg-slate-100 px-2 py-0.5 font-medium uppercase text-slate-700 dark:bg-slate-800 dark:text-slate-300">
                  {mem.memory_type} • {mem.scope}
                </span>
                <span className="text-slate-500">
                  v{mem.version} • {mem.verification_status}
                </span>
              </div>
              <div className="mt-1.5 font-medium text-slate-900 dark:text-white">{mem.title}</div>
              <p className="mt-1 text-xs text-slate-600 dark:text-slate-400">{mem.summary}</p>
              {mem.source_refs.length > 0 && (
                <div className="mt-2 flex flex-wrap gap-1">
                  {mem.source_refs.map((sr) => (
                    <span
                      key={sr}
                      className="rounded bg-slate-100 px-1.5 py-0.5 font-mono text-[10px] text-slate-600 dark:bg-slate-800 dark:text-slate-400"
                    >
                      {sr}
                    </span>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}


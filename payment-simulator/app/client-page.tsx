'use client';

import { useState, useEffect, useCallback } from 'react';

interface WorkloadMetrics {
  rate_per_min: number;
  p50_latency_ms: number;
  p95_latency_ms: number;
  p99_latency_ms: number;
  error_rate: number;
  queue_depth: number;
}

interface Workload {
  workload_id: string;
  state: string;
  metrics: WorkloadMetrics;
  region: string;
}

interface RazorpayStatus {
  razorpay?: {
    enabled: boolean;
    mode?: string;
    key_id?: string;
    key_id_prefix?: string;
    message?: string;
  };
  dedupe_cache_size?: number;
}

interface PaymentHistoryItem {
  id: string;
  status: 'success' | 'failed' | 'pending';
  amountPaise: number;
  region: string;
  method: string;
  time: string;
  source: 'razorpay' | 'synthetic';
}

interface PageProps {
  initialWorkloads?: Workload[];
}

interface RazorpaySuccessResponse {
  razorpay_payment_id: string;
  razorpay_order_id: string;
  razorpay_signature: string;
}

interface RazorpayOptions {
  key: string;
  amount: number;
  currency: string;
  name: string;
  description: string;
  order_id: string;
  notes?: Record<string, string>;
  prefill?: { name?: string; email?: string; contact?: string };
  remember_customer?: boolean;
  theme?: { color: string };
  handler: (response: RazorpaySuccessResponse) => void;
  modal?: { ondismiss?: () => void };
}

declare global {
  interface Window {
    Razorpay?: new (options: RazorpayOptions) => {
      open: () => void;
      on: (event: string, handler: (response: unknown) => void) => void;
    };
  }
}

const REGIONS = [
  { value: 'IN-SOUTH', label: 'India South' },
  { value: 'IN-WEST', label: 'India West' },
  { value: 'IN-NORTH', label: 'India North' },
];

const METHODS = [
  { value: 'upi', label: 'UPI' },
  { value: 'card', label: 'Card' },
  { value: 'netbanking', label: 'Net Banking' },
  { value: 'wallet', label: 'Wallet' },
];

const AMOUNT_PRESETS = [
  { label: '₹100', paise: 10000 },
  { label: '₹500', paise: 50000 },
  { label: '₹5,000', paise: 500000 },
  { label: '₹10,000', paise: 1000000 },
];

export default function PaymentSimulator({ initialWorkloads = [] }: PageProps) {
  const [workloads, setWorkloads] = useState<Workload[]>(initialWorkloads);
  const [trafficMultiplier, setTrafficMultiplier] = useState(1);
  const [isSpike, setIsSpike] = useState(false);
  const [eventCount, setEventCount] = useState(0);
  const [isGenerating, setIsGenerating] = useState(false);
  const [razorpayStatus, setRazorpayStatus] = useState<RazorpayStatus | null>(null);
  const [apiKeysValid, setApiKeysValid] = useState<boolean | null>(null);
  const [isPaying, setIsPaying] = useState(false);
  const [amountPaise, setAmountPaise] = useState(50000);
  const [selectedRegion, setSelectedRegion] = useState('IN-SOUTH');
  const [selectedMethod, setSelectedMethod] = useState('upi');
  const [paymentHistory, setPaymentHistory] = useState<PaymentHistoryItem[]>([]);
  const [activeTab, setActiveTab] = useState<'razorpay' | 'synthetic'>('razorpay');
  const [checkoutReady, setCheckoutReady] = useState(false);

  useEffect(() => {
    if (document.getElementById('razorpay-checkout-js')) {
      setCheckoutReady(true);
      return;
    }
    const script = document.createElement('script');
    script.id = 'razorpay-checkout-js';
    script.src = 'https://checkout.razorpay.com/v1/checkout.js';
    script.async = true;
    script.onload = () => setCheckoutReady(true);
    document.body.appendChild(script);
  }, []);

  const fetchWorkloads = useCallback(async () => {
    try {
      const res = await fetch('/api/workloads');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const data = await res.json();
      setWorkloads(data);
    } catch (error) {
      console.error('Failed to fetch workloads:', error);
    }
  }, []);

  const fetchRazorpayStatus = useCallback(async () => {
    try {
      const res = await fetch('/api/razorpay/status');
      if (res.ok) {
        const data = await res.json();
        setRazorpayStatus(data);
      }
    } catch (error) {
      console.error('Failed to fetch Razorpay status:', error);
    }
  }, []);

  const verifyRazorpayKeys = useCallback(async () => {
    try {
      const res = await fetch('/api/razorpay/verify', { method: 'POST' });
      if (res.ok) {
        const data = await res.json();
        setApiKeysValid(data.api_keys_valid);
      } else {
        setApiKeysValid(false);
      }
    } catch {
      setApiKeysValid(false);
    }
  }, []);

  useEffect(() => {
    fetchWorkloads();
    fetchRazorpayStatus();
    verifyRazorpayKeys();
    const interval = setInterval(fetchWorkloads, 2000);
    const statusInterval = setInterval(fetchRazorpayStatus, 10000);
    return () => {
      clearInterval(interval);
      clearInterval(statusInterval);
    };
  }, [fetchWorkloads, fetchRazorpayStatus, verifyRazorpayKeys]);

  useEffect(() => {
    if (workloads.length === 0) return;

    setIsGenerating(true);
    const interval = setInterval(() => {
      const totalTPS = workloads.reduce((sum, w) => sum + w.metrics.rate_per_min / 60, 0);
      setEventCount((prev) => prev + Math.floor(totalTPS * 2));
    }, 2000);

    return () => clearInterval(interval);
  }, [workloads]);

  const triggerSpike = async () => {
    if (workloads.length === 0) {
      alert('No workloads available. Please seed data first.');
      return;
    }

    setIsSpike(true);
    try {
      await fetch('/api/demo/trigger-spike', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ multiplier: trafficMultiplier }),
      });
      await fetchWorkloads();
      setPaymentHistory((prev) => [
        {
          id: `synthetic-${Date.now()}`,
          status: 'success',
          amountPaise: 0,
          region: selectedRegion,
          method: 'synthetic',
          time: new Date().toISOString(),
          source: 'synthetic',
        },
        ...prev,
      ]);
    } catch (error) {
      console.error('Failed to trigger spike:', error);
    }
    setTimeout(() => setIsSpike(false), 1000);
  };

  const seedData = async () => {
    try {
      await fetch('/api/demo/seed', { method: 'POST' });
      await fetchWorkloads();
      alert('Demo data seeded successfully!');
    } catch (error) {
      console.error('Failed to seed data:', error);
      alert('Failed to seed data. Make sure backend is running.');
    }
  };

  const payWithRazorpay = async () => {
    if (!razorpayStatus?.razorpay?.enabled) {
      alert('Razorpay is not configured. Add RAZORPAY_* keys to .env and restart the API.');
      return;
    }
    if (!checkoutReady || !window.Razorpay) {
      alert('Razorpay Checkout is still loading. Try again in a moment.');
      return;
    }
    if (workloads.length === 0) {
      alert('Seed demo workloads first so payments can map to regional infrastructure.');
      return;
    }

    setIsPaying(true);
    const pendingId = `pending-${Date.now()}`;
    setPaymentHistory((prev) => [
      {
        id: pendingId,
        status: 'pending',
        amountPaise,
        region: selectedRegion,
        method: selectedMethod,
        time: new Date().toISOString(),
        source: 'razorpay',
      },
      ...prev,
    ]);

    try {
      const res = await fetch('/api/razorpay/orders', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          amount_cents: amountPaise,
          region: selectedRegion,
          payment_method: selectedMethod,
        }),
      });

      if (!res.ok) {
        const errText = await res.text();
        throw new Error(errText || `HTTP ${res.status}`);
      }

      const order = await res.json();

      const options: RazorpayOptions = {
        key: order.key_id,
        amount: order.amount,
        currency: order.currency,
        name: 'ESA Payment Gateway',
        description: `Test Mode — ${selectedRegion} / ${selectedMethod}`,
        order_id: order.order_id,
        notes: { region: selectedRegion, esa_region: selectedRegion },
        prefill: {
          name: 'ESA Test User',
          email: 'test@esa.demo',
          contact: '+919000090000',
        },
        remember_customer: false,
        theme: { color: '#2563eb' },
        handler: async (response) => {
          try {
            await fetch('/api/razorpay/confirm', {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({
                payment_id: response.razorpay_payment_id,
              }),
            });
          } catch (e) {
            console.error('Payment confirm failed (webhook may still apply):', e);
          }
          setPaymentHistory((prev) =>
            prev.map((p) =>
              p.id === pendingId
                ? {
                    ...p,
                    id: response.razorpay_payment_id,
                    status: 'success' as const,
                  }
                : p
            )
          );
          setIsPaying(false);
          fetchWorkloads();
        },
        modal: {
          ondismiss: () => {
            setPaymentHistory((prev) => prev.filter((p) => p.id !== pendingId));
            setIsPaying(false);
          },
        },
      };

      const rzp = new window.Razorpay!(options);
      rzp.on('payment.failed', () => {
        setPaymentHistory((prev) =>
          prev.map((p) =>
            p.id === pendingId ? { ...p, status: 'failed' as const } : p
          )
        );
        setIsPaying(false);
      });
      rzp.open();
    } catch (error) {
      setPaymentHistory((prev) => prev.filter((p) => p.id !== pendingId));
      setIsPaying(false);
      alert(`Could not start Razorpay payment: ${error instanceof Error ? error.message : error}`);
    }
  };

  const getTotalTPS = () =>
    workloads.reduce((sum, w) => sum + w.metrics.rate_per_min / 60, 0).toFixed(0);

  const getAvgLatency = () => {
    if (workloads.length === 0) return '0';
    const avg =
      workloads.reduce((sum, w) => sum + w.metrics.p95_latency_ms, 0) / workloads.length;
    return avg.toFixed(0);
  };

  const getAvgErrorRate = () => {
    if (workloads.length === 0) return '0';
    const avg = workloads.reduce((sum, w) => sum + w.metrics.error_rate, 0) / workloads.length;
    return (avg * 100).toFixed(2);
  };

  const getTotalQueue = () => workloads.reduce((sum, w) => sum + w.metrics.queue_depth, 0);

  const getHealthStatus = () => {
    const avgLatency = parseFloat(getAvgLatency());
    const avgError = parseFloat(getAvgErrorRate());

    if (avgLatency > 250 || avgError > 3)
      return { text: 'DEGRADED', color: 'text-red-500', bg: 'bg-red-100' };
    if (avgLatency > 150 || avgError > 1.5)
      return { text: 'WARNING', color: 'text-yellow-500', bg: 'bg-yellow-100' };
    return { text: 'HEALTHY', color: 'text-green-500', bg: 'bg-green-100' };
  };

  const health = getHealthStatus();
  const razorpayEnabled = razorpayStatus?.razorpay?.enabled ?? false;
  const razorpayMode = razorpayStatus?.razorpay?.mode ?? 'off';

  const formatRupees = (paise: number) =>
    paise === 0 ? '—' : `₹${(paise / 100).toLocaleString('en-IN')}`;

  return (
    <div className="min-h-screen bg-gradient-to-br from-blue-50 to-indigo-100 p-8">
      <div className="max-w-7xl mx-auto">
        {/* Header */}
        <div className="mb-8">
          <div className="flex flex-wrap items-center gap-3 mb-4">
            <div className="bg-blue-600 text-white px-4 py-2 rounded-lg font-bold">
              UI #1: Payment Event Surface
            </div>
            <div className="text-gray-400">→</div>
            <div className="bg-gray-200 text-gray-600 px-4 py-2 rounded-lg font-bold">
              UI #2: ESA Control Plane (Port 3000)
            </div>
            {razorpayEnabled && (
              <span className="bg-emerald-100 text-emerald-800 border border-emerald-300 px-3 py-1 rounded-full text-sm font-semibold">
                Razorpay {razorpayMode.toUpperCase()} connected
              </span>
            )}
          </div>
          <h1 className="text-4xl font-bold text-gray-900 mb-2">
            Payment Infrastructure Simulator
          </h1>
          <p className="text-gray-600">
            Razorpay Test Mode payments + synthetic traffic for ESA autonomous recovery demo
          </p>
        </div>

        {/* Razorpay connection card */}
        <div className="bg-white rounded-lg shadow-lg p-6 mb-8 border border-gray-200">
          <div className="flex flex-wrap items-start justify-between gap-4">
            <div>
              <h2 className="text-xl font-bold text-gray-900 flex items-center gap-2">
                <span className="text-2xl">🔗</span> Razorpay Integration
              </h2>
              <p className="text-sm text-gray-600 mt-1">
                Real Test Mode checkout → webhook → ESA workload metrics
              </p>
            </div>
            <div className="flex flex-wrap gap-2">
              <StatusPill
                label="Adapter"
                ok={razorpayEnabled}
                text={razorpayEnabled ? 'Enabled' : 'Disabled'}
              />
              <StatusPill
                label="API Keys"
                ok={apiKeysValid === true}
                text={
                  apiKeysValid === null
                    ? 'Checking…'
                    : apiKeysValid
                      ? 'Valid'
                      : 'Invalid / offline'
                }
              />
              <StatusPill
                label="Checkout"
                ok={checkoutReady}
                text={checkoutReady ? 'Ready' : 'Loading…'}
              />
            </div>
          </div>
          {razorpayEnabled && razorpayStatus?.razorpay?.key_id_prefix && (
            <p className="text-xs text-gray-500 mt-3 font-mono">
              Key: {razorpayStatus.razorpay.key_id_prefix} • Webhook dedupe cache:{' '}
              {razorpayStatus.dedupe_cache_size ?? 0} events
            </p>
          )}
          {!razorpayEnabled && (
            <div className="mt-4 p-3 bg-amber-50 border border-amber-200 rounded-lg text-sm text-amber-900">
              Add <code className="bg-amber-100 px-1 rounded">RAZORPAY_KEY_ID</code>,{' '}
              <code className="bg-amber-100 px-1 rounded">RAZORPAY_KEY_SECRET</code>, and{' '}
              <code className="bg-amber-100 px-1 rounded">RAZORPAY_WEBHOOK_SECRET</code> to{' '}
              <code className="bg-amber-100 px-1 rounded">.env</code> and restart the API. Synthetic
              spike mode still works without Razorpay.
            </div>
          )}
        </div>

        {/* System Health Banner */}
        <div className={`${health.bg} border-2 rounded-lg p-6 mb-8`}>
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-2xl font-bold text-gray-900">Infrastructure Status</h2>
              <p className={`text-3xl font-bold ${health.color} mt-2`}>{health.text}</p>
              {isGenerating && (
                <div className="mt-2 flex items-center gap-2 text-sm text-gray-600">
                  <span className="relative flex h-3 w-3">
                    <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-green-400 opacity-75" />
                    <span className="relative inline-flex rounded-full h-3 w-3 bg-green-500" />
                  </span>
                  <span>Live metrics from payment events</span>
                </div>
              )}
            </div>
            <div className="text-right">
              <p className="text-sm text-gray-600">Total TPS</p>
              <p className="text-4xl font-bold text-gray-900">{getTotalTPS()}</p>
              <p className="text-xs text-gray-500 mt-1">
                {eventCount.toLocaleString()} events observed
              </p>
            </div>
          </div>
        </div>

        {/* Aggregate Metrics */}
        <div className="grid grid-cols-1 md:grid-cols-4 gap-6 mb-8">
          <MetricCard
            title="P95 Latency"
            value={`${getAvgLatency()}ms`}
            status={
              parseFloat(getAvgLatency()) > 250
                ? 'bad'
                : parseFloat(getAvgLatency()) > 150
                  ? 'warning'
                  : 'good'
            }
            icon="⚡"
          />
          <MetricCard
            title="Error Rate"
            value={`${getAvgErrorRate()}%`}
            status={
              parseFloat(getAvgErrorRate()) > 3
                ? 'bad'
                : parseFloat(getAvgErrorRate()) > 1.5
                  ? 'warning'
                  : 'good'
            }
            icon="⚠️"
          />
          <MetricCard
            title="Queue Depth"
            value={getTotalQueue().toString()}
            status={
              getTotalQueue() > 2000 ? 'bad' : getTotalQueue() > 1000 ? 'warning' : 'good'
            }
            icon="📊"
          />
          <MetricCard
            title="Active Workloads"
            value={workloads.length.toString()}
            status="good"
            icon="🔧"
          />
        </div>

        {/* Tabbed controls */}
        <div className="bg-white rounded-lg shadow-lg p-6 mb-8">
          <div className="flex gap-2 mb-6 border-b border-gray-200 pb-4">
            <button
              onClick={() => setActiveTab('razorpay')}
              className={`px-4 py-2 rounded-lg font-semibold transition ${
                activeTab === 'razorpay'
                  ? 'bg-blue-600 text-white'
                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
              }`}
            >
              Razorpay Test Payment
            </button>
            <button
              onClick={() => setActiveTab('synthetic')}
              className={`px-4 py-2 rounded-lg font-semibold transition ${
                activeTab === 'synthetic'
                  ? 'bg-orange-500 text-white'
                  : 'bg-gray-100 text-gray-600 hover:bg-gray-200'
              }`}
            >
              Synthetic Traffic Spike
            </button>
            {workloads.length === 0 && (
              <button
                onClick={seedData}
                className="ml-auto bg-emerald-500 text-white px-4 py-2 rounded-lg font-semibold hover:bg-emerald-600"
              >
                Seed Demo Data
              </button>
            )}
          </div>

          {activeTab === 'razorpay' ? (
            <div>
              <h3 className="text-xl font-bold text-gray-900 mb-4">Pay with Razorpay (Test Mode)</h3>
              <p className="text-sm text-gray-600 mb-6">
                Creates a real Test Mode order. On success, Razorpay sends a webhook to ESA and
                regional workload metrics update automatically.
              </p>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">Amount</label>
                  <div className="flex flex-wrap gap-2 mb-3">
                    {AMOUNT_PRESETS.map((preset) => (
                      <button
                        key={preset.paise}
                        onClick={() => setAmountPaise(preset.paise)}
                        className={`px-3 py-1.5 rounded-lg text-sm font-medium border transition ${
                          amountPaise === preset.paise
                            ? 'bg-blue-600 text-white border-blue-600'
                            : 'bg-white text-gray-700 border-gray-300 hover:border-blue-400'
                        }`}
                      >
                        {preset.label}
                      </button>
                    ))}
                  </div>
                  <p className="text-xs text-gray-500">
                    Selected: {formatRupees(amountPaise)} ({amountPaise} paise)
                  </p>
                </div>

                <div className="space-y-4">
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-2">Region</label>
                    <select
                      value={selectedRegion}
                      onChange={(e) => setSelectedRegion(e.target.value)}
                      className="w-full border border-gray-300 rounded-lg px-3 py-2 text-gray-900"
                    >
                      {REGIONS.map((r) => (
                        <option key={r.value} value={r.value}>{r.label}</option>
                      ))}
                    </select>
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-2">
                      Payment method class
                    </label>
                    <select
                      value={selectedMethod}
                      onChange={(e) => setSelectedMethod(e.target.value)}
                      className="w-full border border-gray-300 rounded-lg px-3 py-2 text-gray-900"
                    >
                      {METHODS.map((m) => (
                        <option key={m.value} value={m.value}>{m.label}</option>
                      ))}
                    </select>
                  </div>
                </div>
              </div>

              <button
                onClick={payWithRazorpay}
                disabled={isPaying || !razorpayEnabled || workloads.length === 0}
                className={`w-full py-4 rounded-lg font-bold text-white text-lg transition ${
                  isPaying || !razorpayEnabled || workloads.length === 0
                    ? 'bg-gray-400 cursor-not-allowed'
                    : 'bg-gradient-to-r from-blue-600 to-indigo-600 hover:from-blue-700 hover:to-indigo-700'
                }`}
              >
                {isPaying ? 'Opening Razorpay Checkout…' : 'Pay with Razorpay Test Mode'}
              </button>

              <div className="mt-4 p-3 bg-blue-50 border border-blue-200 rounded-lg text-xs text-blue-800 space-y-2">
                <p>
                  <strong>Recommended — UPI:</strong> Choose <strong>UPI</strong> in Checkout →{' '}
                  <code>success@razorpay</code> → no card errors.
                </p>
                <p>
                  <strong>Domestic test cards</strong> (INR only — do not use foreign cards):
                  Mastercard <code>5267 3181 8797 5449</code> • Visa{' '}
                  <code>4718 6091 0820 4366</code>
                </p>
                <p>
                  <strong>Netbanking:</strong> Pick any bank → mock page → click <strong>Success</strong>.
                </p>
                <p className="text-red-700">
                  &quot;International cards not supported&quot; = foreign/real card or wrong test
                  card. Use UPI or domestic numbers above.
                </p>
                <p>
                  Card OTP: <strong>Skip OTP</strong> or <code>1234</code> • Phone:{' '}
                  <code>9000090000</code> • Test Mode — no real money.
                </p>
              </div>
            </div>
          ) : (
            <div>
              <h3 className="text-xl font-bold text-gray-900 mb-4">Synthetic traffic spike</h3>
              {workloads.length === 0 ? (
                <div className="text-center py-8">
                  <p className="text-gray-600 mb-4">No workloads found. Seed demo data first.</p>
                  <button
                    onClick={seedData}
                    className="bg-blue-500 text-white px-6 py-3 rounded-lg font-semibold hover:bg-blue-600"
                  >
                    Seed Demo Data
                  </button>
                </div>
              ) : (
                <>
                  <div className="mb-6">
                    <label className="block text-sm font-medium text-gray-700 mb-2">
                      Traffic spike multiplier:{' '}
                      <span className="font-bold text-lg">{trafficMultiplier}x</span>
                    </label>
                    <input
                      type="range"
                      min="1"
                      max="5"
                      step="0.5"
                      value={trafficMultiplier}
                      onChange={(e) => setTrafficMultiplier(parseFloat(e.target.value))}
                      className="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer"
                    />
                  </div>
                  <button
                    onClick={triggerSpike}
                    disabled={isSpike}
                    className={`w-full py-4 rounded-lg font-bold text-white text-lg transition ${
                      isSpike
                        ? 'bg-gray-400 cursor-not-allowed'
                        : 'bg-gradient-to-r from-red-500 to-orange-500 hover:from-red-600 hover:to-orange-600'
                    }`}
                  >
                    {isSpike ? 'Spike triggered!' : 'Trigger traffic spike'}
                  </button>
                </>
              )}
            </div>
          )}

          {/* Autonomous recovery notice */}
          <div className="mt-6 p-4 bg-purple-50 border-2 border-purple-200 rounded-lg">
            <p className="text-sm font-semibold text-purple-900 mb-1">
              Autonomous recovery active
            </p>
            <p className="text-xs text-purple-700">
              ESA monitors workloads every 5s and auto-executes recovery when degradation is
              detected — Monitor → Diagnose → Plan → Safety → Execute.
            </p>
          </div>
        </div>

        {/* Payment history */}
        {paymentHistory.length > 0 && (
          <div className="bg-white rounded-lg shadow-lg p-6 mb-8">
            <h3 className="text-xl font-bold text-gray-900 mb-4">Recent payment events</h3>
            <div className="space-y-2">
              {paymentHistory.slice(0, 8).map((item) => (
                <div
                  key={item.id}
                  className="flex flex-wrap items-center justify-between gap-2 p-3 border border-gray-200 rounded-lg text-sm"
                >
                  <div className="flex items-center gap-3">
                    <span
                      className={`px-2 py-0.5 rounded text-xs font-bold ${
                        item.source === 'razorpay'
                          ? 'bg-blue-100 text-blue-800'
                          : 'bg-orange-100 text-orange-800'
                      }`}
                    >
                      {item.source}
                    </span>
                    <span className="font-mono text-gray-700">{item.id}</span>
                  </div>
                  <div className="flex items-center gap-4 text-gray-600">
                    <span>{formatRupees(item.amountPaise)}</span>
                    <span>{item.region}</span>
                    <span
                      className={
                        item.status === 'success'
                          ? 'text-green-600 font-semibold'
                          : item.status === 'failed'
                            ? 'text-red-600 font-semibold'
                            : 'text-yellow-600 font-semibold'
                      }
                    >
                      {item.status}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Regional Workloads */}
        <div className="bg-white rounded-lg shadow-lg p-6">
          <h3 className="text-xl font-bold text-gray-900 mb-4">Regional workloads</h3>
          {workloads.length === 0 ? (
            <p className="text-center text-gray-500 py-8">No workloads available</p>
          ) : (
            <div className="space-y-4">
              {workloads.map((workload) => (
                <WorkloadCard key={workload.workload_id} workload={workload} />
              ))}
            </div>
          )}
        </div>

        {/* Architecture */}
        <div className="mt-8 bg-blue-50 border-2 border-blue-200 rounded-lg p-6">
          <h3 className="text-lg font-bold text-blue-900 mb-2">Demo flow</h3>
          <pre className="text-xs text-gray-700 overflow-x-auto bg-white p-4 rounded border border-blue-200 mb-4">
{`Payment UI (Port 5173)
  ├─ Razorpay Test Checkout → POST /api/razorpay/orders
  │       ↓ Razorpay webhook
  │   POST /api/razorpay/webhook (signature verified)
  └─ Synthetic spike → POST /api/demo/trigger-spike
        ↓
ESA API (Port 8080) → State Fabric + Multi-Agent Runtime
        ↓
ESA Control Plane (Port 3000) — agents, audit, policy, effects`}
          </pre>
          <ol className="list-decimal list-inside space-y-2 text-blue-800 text-sm">
            <li>Seed demo workloads</li>
            <li>Pay with Razorpay Test Mode or trigger a synthetic spike</li>
            <li>Watch metrics degrade on this UI</li>
            <li>Open ESA dashboard (port 3000) to see autonomous recovery</li>
          </ol>
        </div>
      </div>
    </div>
  );
}

function StatusPill({
  label,
  ok,
  text,
}: {
  label: string;
  ok: boolean;
  text: string;
}) {
  return (
    <div
      className={`px-3 py-1.5 rounded-full text-sm font-semibold border ${
        ok
          ? 'bg-emerald-50 text-emerald-800 border-emerald-300'
          : 'bg-gray-100 text-gray-600 border-gray-300'
      }`}
    >
      {label}: {text}
    </div>
  );
}

function MetricCard({
  title,
  value,
  status,
  icon,
}: {
  title: string;
  value: string;
  status: string;
  icon: string;
}) {
  const colors = {
    good: 'bg-green-100 border-green-300 text-green-700',
    warning: 'bg-yellow-100 border-yellow-300 text-yellow-700',
    bad: 'bg-red-100 border-red-300 text-red-700',
  };

  return (
    <div className={`${colors[status as keyof typeof colors]} border-2 rounded-lg p-4`}>
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm font-medium opacity-80">{title}</span>
        <span className="text-2xl">{icon}</span>
      </div>
      <p className="text-3xl font-bold">{value}</p>
    </div>
  );
}

function WorkloadCard({ workload }: { workload: Workload }) {
  const stateColors: Record<string, string> = {
    HEALTHY: 'bg-green-100 text-green-700 border-green-300',
    DEGRADED: 'bg-red-100 text-red-700 border-red-300',
    OVERLOADED: 'bg-red-100 text-red-700 border-red-300',
    RECOVERING: 'bg-yellow-100 text-yellow-700 border-yellow-300',
  };

  return (
    <div className="border-2 border-gray-200 rounded-lg p-4 hover:border-blue-300 transition">
      <div className="flex items-center justify-between mb-3">
        <div>
          <h4 className="font-bold text-gray-900">{workload.workload_id}</h4>
          <p className="text-sm text-gray-600">Region: {workload.region}</p>
        </div>
        <span
          className={`px-3 py-1 rounded-full text-sm font-bold border-2 ${
            stateColors[workload.state] ?? 'bg-gray-100 text-gray-700 border-gray-300'
          }`}
        >
          {workload.state}
        </span>
      </div>
      <div className="grid grid-cols-3 gap-4 text-sm">
        <div>
          <p className="text-gray-600">TPS</p>
          <p className="font-bold">{(workload.metrics.rate_per_min / 60).toFixed(0)}</p>
        </div>
        <div>
          <p className="text-gray-600">P95 Latency</p>
          <p className="font-bold">{workload.metrics.p95_latency_ms.toFixed(0)}ms</p>
        </div>
        <div>
          <p className="text-gray-600">Error Rate</p>
          <p className="font-bold">{(workload.metrics.error_rate * 100).toFixed(2)}%</p>
        </div>
      </div>
    </div>
  );
}

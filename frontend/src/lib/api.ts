// API Client for ESA Backend
import type { WorkloadEntity, TokenStats, VitalsSnapshot } from '@/types';

export function getApiBaseUrl(): string {
  if (typeof window !== 'undefined') {
    const params = new URLSearchParams(window.location.search);
    const paramServer = params.get('server') || params.get('api');
    if (paramServer) {
      const clean = paramServer.replace(/\/$/, '');
      localStorage.setItem('esa_server_url', clean);
      return clean;
    }
    const saved = localStorage.getItem('esa_server_url');
    if (saved) {
      return saved.replace(/\/$/, '');
    }
  }
  return (import.meta.env.VITE_API_URL || '').replace(/\/$/, '');
}

export function setApiBaseUrl(url: string) {
  if (typeof window !== 'undefined') {
    const clean = url.trim().replace(/\/$/, '');
    localStorage.setItem('esa_server_url', clean);
    window.dispatchEvent(new CustomEvent('esa-server-changed', { detail: clean }));
  }
}

export function getApiKey(): string {
  if (typeof window !== 'undefined') {
    const params = new URLSearchParams(window.location.search);
    const paramKey = params.get('key') || params.get('api_key');
    if (paramKey) {
      localStorage.setItem('esa_api_key', paramKey.trim());
      return paramKey.trim();
    }
    const saved = localStorage.getItem('esa_api_key');
    if (saved) {
      return saved;
    }

    // Check mode query param or default to environment variables
    const mode = params.get('mode');
    if (mode === 'sandbox' || mode === 'test') {
      return (import.meta.env.VITE_ESA_TEST_API_KEY || import.meta.env.VITE_ESA_SANDBOX_API_KEY || '').trim();
    }
    return (
      import.meta.env.VITE_ESA_LIVE_API_KEY ||
      import.meta.env.VITE_ESA_API_KEY ||
      import.meta.env.VITE_ESA_TEST_API_KEY ||
      ''
    ).trim();
  }
  return '';
}

export function setApiKey(key: string) {
  if (typeof window !== 'undefined') {
    const clean = key.trim();
    if (clean) {
      localStorage.setItem('esa_api_key', clean);
    } else {
      localStorage.removeItem('esa_api_key');
    }
    window.dispatchEvent(new CustomEvent('esa-key-changed', { detail: clean }));
  }
}

export class ApiClient {
  private customBaseUrl?: string;

  constructor(customBaseUrl?: string) {
    this.customBaseUrl = customBaseUrl;
  }

  getBaseUrl(): string {
    return this.customBaseUrl || getApiBaseUrl();
  }

  private getHeaders(customHeaders: Record<string, string> = {}): Record<string, string> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      ...customHeaders,
    };
    const key = getApiKey();
    if (key) {
      headers['Authorization'] = `Bearer ${key}`;
    }
    return headers;
  }

  async get<T>(path: string): Promise<T> {
    const response = await fetch(`${this.getBaseUrl()}${path}`, {
      headers: this.getHeaders(),
    });
    if (!response.ok) {
      throw new Error(`API error: ${response.statusText}`);
    }
    return response.json();
  }

  async post<T>(path: string, data?: any): Promise<T> {
    const response = await fetch(`${this.getBaseUrl()}${path}`, {
      method: 'POST',
      headers: this.getHeaders(),
      body: data !== undefined ? JSON.stringify(data) : undefined,
    });
    if (!response.ok) {
      throw new Error(`API error: ${response.statusText}`);
    }
    return response.json();
  }

  // Workload endpoints
  async getWorkloads(): Promise<WorkloadEntity[]> {
    return this.get<WorkloadEntity[]>('/api/workloads');
  }

  async getWorkload(id: string) {
    return this.get(`/api/workloads/${id}`);
  }

  async createWorkload(workload: any) {
    return this.post('/api/workloads', workload);
  }

  async getVitalsHistory(): Promise<{ snapshots: VitalsSnapshot[]; latest: VitalsSnapshot | null }> {
    return this.get<{ snapshots: VitalsSnapshot[]; latest: VitalsSnapshot | null }>(
      '/api/vitals/history'
    );
  }

  // Demo endpoints
  async triggerSpike(workloadId: string, multiplier: number) {
    return this.post('/api/demo/trigger-spike', { workload_id: workloadId, multiplier });
  }

  // Metrics endpoints
  async getTokenMetrics(): Promise<TokenStats> {
    return this.get<TokenStats>('/api/metrics/tokens');
  }

  // Agent endpoints
  async getAgentsStatus() {
    return this.get('/api/agents/status');
  }

  async getRecentActions() {
    return this.get('/api/actions/recent');
  }

  // Audit Trail endpoints
  async getAuditTrail() {
    return this.get('/api/audit/trail');
  }

  async getDecisionDetail(decisionId: string) {
    return this.get(`/api/audit/decision/${decisionId}`);
  }

  async replayDecision(decisionId: string) {
    return this.post(`/api/audit/replay/${decisionId}`, {});
  }

  // Effect Measurement endpoints
  async getEffectMeasurements() {
    return this.get('/api/effects/measurements');
  }

  async getRecentEffects() {
    return this.get('/api/effects/recent');
  }

  // AI Cost endpoints
  async getAICosts() {
    return this.get('/api/costs/ai');
  }

  async getCostsPerAgent() {
    return this.get('/api/costs/per-agent');
  }

  // Policy Verdict endpoints
  async getRecentVerdicts() {
    return this.get('/api/verdicts/recent');
  }

  async getVerdictStats() {
    return this.get('/api/verdicts/stats');
  }

  // Intent & Constraints endpoints
  async getActiveIntents() {
    return this.get('/api/intent/active');
  }

  async getConstraintViolations() {
    return this.get('/api/intent/violations');
  }

  // Demo scenario triggers
  async triggerScenario(scenario: string, intensity?: number) {
    return this.post(`/api/demo/scenario/${scenario}`, { intensity });
  }

  // Multi-Gateway Corridors
  async getGateways() {
    return this.get<any[]>('/api/gateways');
  }

  async toggleGateway(name: string) {
    return this.post<any>(`/api/gateways/${name}/toggle`);
  }

  async checkout(params: { amount: number; currency?: string; gateway?: string; method?: string }) {
    return this.post<any>('/api/payments/checkout', params);
  }
}

export const apiClient = new ApiClient();

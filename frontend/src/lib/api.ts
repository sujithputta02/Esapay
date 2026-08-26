// API Client for ESA Backend
import type { WorkloadEntity, TokenStats, VitalsSnapshot } from '@/types';

// Empty string uses same-origin + Vite proxy in dev (/api → :8080)
const API_BASE_URL = import.meta.env.VITE_API_URL || '';

export class ApiClient {
  private baseUrl: string;

  constructor(baseUrl: string = API_BASE_URL) {
    this.baseUrl = baseUrl;
  }

  async get<T>(path: string): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`);
    if (!response.ok) {
      throw new Error(`API error: ${response.statusText}`);
    }
    return response.json();
  }

  async post<T>(path: string, data: any): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(data),
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
}

export const apiClient = new ApiClient();

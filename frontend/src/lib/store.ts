import { create } from 'zustand';
import type { WorkloadEntity, Condition, ActionExecution, AgentStatus, VitalsSnapshot } from '@/types';

interface EsaStore {
  workloads: WorkloadEntity[];
  conditions: Condition[];
  executions: ActionExecution[];
  agentStatuses: Record<string, AgentStatus>;
  vitalsHistory: VitalsSnapshot[];

  setWorkloads: (workloads: WorkloadEntity[]) => void;
  updateWorkload: (workload: WorkloadEntity) => void;
  appendVitals: (snapshot: VitalsSnapshot) => void;
  setVitalsHistory: (snapshots: VitalsSnapshot[]) => void;
  addCondition: (condition: Condition) => void;
  addExecution: (execution: ActionExecution) => void;
  updateAgentStatus: (agentId: string, status: Partial<AgentStatus>) => void;
}

export const useEsaStore = create<EsaStore>((set) => ({
  workloads: [],
  conditions: [],
  executions: [],
  vitalsHistory: [],
  agentStatuses: {
    monitor: { agent_id: 'monitor', status: 'IDLE', current_task: null, confidence: null, latest_observation: null, latest_decision: null },
    diagnosis: { agent_id: 'diagnosis', status: 'IDLE', current_task: null, confidence: null, latest_observation: null, latest_decision: null },
    planning: { agent_id: 'planning', status: 'IDLE', current_task: null, confidence: null, latest_observation: null, latest_decision: null },
    safety: { agent_id: 'safety', status: 'IDLE', current_task: null, confidence: null, latest_observation: null, latest_decision: null },
  },

  setWorkloads: (workloads) => set({ workloads }),

  appendVitals: (snapshot) =>
    set((state) => {
      const next = [...state.vitalsHistory, snapshot];
      return { vitalsHistory: next.length > 120 ? next.slice(-120) : next };
    }),

  setVitalsHistory: (snapshots) => set({ vitalsHistory: snapshots }),

  updateWorkload: (workload) => set((state) => ({
    workloads: state.workloads.some((w) => w.workload_id === workload.workload_id)
      ? state.workloads.map((w) => w.workload_id === workload.workload_id ? { ...w, ...workload } : w)
      : [...state.workloads, workload],
  })),
  
  addCondition: (condition) => set((state) => ({
    conditions: [condition, ...state.conditions].slice(0, 50),
  })),
  
  addExecution: (execution) => set((state) => ({
    executions: [execution, ...state.executions].slice(0, 50),
  })),
  
  updateAgentStatus: (agentId, status) => set((state) => ({
    agentStatuses: {
      ...state.agentStatuses,
      [agentId]: { ...state.agentStatuses[agentId], ...status },
    },
  })),
}));

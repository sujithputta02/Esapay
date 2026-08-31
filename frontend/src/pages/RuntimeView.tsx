import { useQuery } from '@tanstack/react-query';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { Network, Server, Zap } from 'lucide-react';
import { motion, PanInfo } from 'framer-motion';
import { useState, useRef, useEffect } from 'react';

interface WorkloadData {
  workload_id: string;
  shard_id: string;
  region: string;
  state: string;
  lifecycle: string;
  version: number;
  metrics: {
    rate_per_min: number;
    p95_latency_ms: number;
    queue_depth: number;
    error_rate: number;
  };
  replication: {
    current_replicas: number;
    max_replicas: number;
    consistency_mode: string;
  };
  locality: {
    fallback_regions: string[];
  };
}

const EMPTY_WORKLOADS: WorkloadData[] = [];
const EMPTY_ACTIONS = { actions: [] as any[] };

export function RuntimeView() {
  const [dragConstraints, setDragConstraints] = useState({ left: 0, right: 0, top: 0, bottom: 0 });
  const [actionPulses, setActionPulses] = useState<number[]>([]);
  const [hoveredWorkload, setHoveredWorkload] = useState<string | null>(null);
  const [nodePositions, setNodePositions] = useState<Record<string, { x: number; y: number }>>({});
  const containerRef = useRef<HTMLDivElement>(null);
  const positionsInitialized = useRef(false);

  const { data: workloadsData } = useQuery({
    queryKey: ['workloads'],
    queryFn: () => apiClient.getWorkloads(),
    refetchInterval: 3000,
  }) as { data: WorkloadData[] | undefined };

  const workloads = workloadsData ?? EMPTY_WORKLOADS;

  const { data: recentActionsData } = useQuery({
    queryKey: ['recent-actions'],
    queryFn: async () => {
      const response = await apiClient.get('/api/actions/recent');
      return response as { actions: any[] };
    },
    refetchInterval: 2000,
  }) as { data: { actions: any[] } | undefined };

  const recentActions = recentActionsData ?? EMPTY_ACTIONS;

  // Track when new actions occur to trigger animations
  const prevActionsCount = useRef(0);
  const actionsLength = recentActions.actions?.length ?? 0;
  useEffect(() => {
    if (actionsLength > prevActionsCount.current && prevActionsCount.current > 0) {
      setActionPulses((prev) => [...prev, Date.now()]);
      setTimeout(() => {
        setActionPulses((prev) => prev.slice(1));
      }, 2000);
    }
    prevActionsCount.current = actionsLength;
  }, [actionsLength]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const updateConstraints = () => {
      const rect = el.getBoundingClientRect();
      setDragConstraints({
        left: -rect.width / 3,
        right: rect.width / 3,
        top: -rect.height / 3,
        bottom: rect.height / 3,
      });
    };

    updateConstraints();
    window.addEventListener('resize', updateConstraints);
    return () => window.removeEventListener('resize', updateConstraints);
  }, []);

  const workloadIds = workloads.map((w) => w.workload_id).join(',');

  // Initialize node positions once when workloads first load
  useEffect(() => {
    if (workloads.length === 0 || positionsInitialized.current) {
      return;
    }

    const initialPositions: Record<string, { x: number; y: number }> = {};
    workloads.forEach((workload, idx) => {
      const angle = (idx / workloads.length) * 2 * Math.PI;
      const radius = 120;
      initialPositions[workload.workload_id] = {
        x: Math.cos(angle) * radius,
        y: Math.sin(angle) * radius,
      };
    });
    setNodePositions(initialPositions);
    positionsInitialized.current = true;
  }, [workloadIds, workloads.length]);

  const regionCounts = workloads.reduce((acc: Record<string, number>, w: any) => {
    acc[w.region] = (acc[w.region] || 0) + 1;
    return acc;
  }, {});

  const totalKubernetesPods = workloads.reduce(
    (sum: number, w: any) => sum + (w.replication?.current_replicas || 2),
    0
  ) || 18;

  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-[28px] font-bold text-white tracking-tight">Runtime & Kubernetes Topology</h1>
        <p className="text-[15px] text-[#B8B8B8] mt-1">
          Live cluster state, active pod distribution, and interactive mesh visualization.
        </p>
      </div>

      {/* 4 Stat Cards Row (rounded-[22px] bg-[#333333]) */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-5">
        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Active Kubernetes Pods</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-accent tracking-tight">
            {totalKubernetesPods} Pods
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Active Workloads</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-white tracking-tight">
            {workloads?.length || 0}
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">Regional Clusters</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-white tracking-tight">
            {Object.keys(regionCounts).length || 3}
          </span>
        </div>

        <div className="bg-[#333333] rounded-[22px] p-6 min-h-[120px] flex flex-col justify-between border border-white/[0.03]">
          <span className="text-[15px] font-medium text-[#B8B8B8]">State Fabric Stores</span>
          <span className="text-[28px] lg:text-[32px] font-extrabold text-white tracking-tight">
            {workloads?.length || 3} Shards
          </span>
        </div>
      </div>

      {/* Animated Topology Graph Card */}
      <div className="bg-[#272727] rounded-[32px] p-7 sm:p-9 border border-white/[0.04]">
        <div className="flex items-center justify-between mb-6">
          <h3 className="text-[20px] font-bold text-white">Live Cluster Topology Graph</h3>
          <span className="text-xs text-[#777777] font-mono">Drag nodes to explore</span>
        </div>

        <div
          ref={containerRef}
          className="relative h-96 bg-[#1D1E1C] rounded-[22px] p-8 overflow-hidden border border-white/[0.04]"
        >
          {workloads && workloads.length > 0 ? (
            <motion.div
              className="relative w-full h-full flex items-center justify-center"
              drag
              dragConstraints={dragConstraints}
              dragElastic={0.1}
              dragTransition={{ bounceStiffness: 300, bounceDamping: 20 }}
              whileDrag={{ cursor: 'grabbing' }}
              style={{ cursor: 'grab' }}
            >
              {/* Central ESA Runtime Hub */}
              <motion.div
                initial={{ scale: 0 }}
                animate={{ scale: 1 }}
                transition={{ duration: 0.5 }}
                className="absolute z-10 w-24 h-24 bg-[#333333] border-2 border-accent rounded-full flex items-center justify-center shadow-lg"
              >
                <Zap className="w-10 h-10 text-accent" />

                {/* Action Pulse Animations */}
                {actionPulses.map((pulse) => (
                  <motion.div
                    key={pulse}
                    initial={{ scale: 1, opacity: 1 }}
                    animate={{ scale: 3, opacity: 0 }}
                    transition={{ duration: 2, ease: "easeOut" }}
                    className="absolute w-full h-full border-2 border-accent/40 rounded-full"
                    style={{ pointerEvents: 'none' }}
                  />
                ))}
              </motion.div>

              {/* Workload Nodes */}
              {workloads.map((workload: any, idx: number) => {
                const position = nodePositions[workload.workload_id] || { x: 0, y: 0 };
                const x = position.x;
                const y = position.y;

                const hasRecentAction = recentActions?.actions?.some(
                  (action: any) => action.workload_id === workload.workload_id &&
                    (Date.now() - new Date(action.timestamp).getTime()) < 5000
                );

                const handleDragEnd = (_event: any, info: PanInfo) => {
                  setNodePositions(prev => ({
                    ...prev,
                    [workload.workload_id]: {
                      x: prev[workload.workload_id].x + info.offset.x,
                      y: prev[workload.workload_id].y + info.offset.y,
                    }
                  }));
                };

                return (
                  <div key={workload.workload_id}>
                    {/* Connection Line */}
                    <motion.svg
                      initial={{ pathLength: 0 }}
                      animate={{ pathLength: 1 }}
                      transition={{ duration: 1, delay: idx * 0.1 }}
                      className="absolute top-0 left-0 w-full h-full pointer-events-none"
                      style={{ zIndex: 0 }}
                    >
                      <motion.line
                        x1="50%"
                        y1="50%"
                        x2={`calc(50% + ${x}px)`}
                        y2={`calc(50% + ${y}px)`}
                        stroke="currentColor"
                        strokeWidth={hasRecentAction ? "3" : "1.5"}
                        className={hasRecentAction ? "text-accent" : "text-white/10"}
                        strokeDasharray="6,8"
                      />
                    </motion.svg>

                    {/* Workload Node - Individually Draggable */}
                    <motion.div
                      initial={{ opacity: 0, scale: 0 }}
                      animate={{
                        opacity: 1,
                        scale: 1,
                        x,
                        y,
                      }}
                      transition={{ delay: idx * 0.15, duration: 0.3 }}
                      drag
                      dragMomentum={false}
                      dragElastic={0}
                      onDragEnd={handleDragEnd}
                      onHoverStart={() => setHoveredWorkload(workload.workload_id)}
                      onHoverEnd={() => setHoveredWorkload(null)}
                      className="absolute z-10 left-1/2 top-1/2"
                      style={{ cursor: 'grab' }}
                      whileDrag={{ cursor: 'grabbing', scale: 1.1, zIndex: 50 }}
                    >
                      <div
                        className={`w-16 h-16 ${workload.state === 'HEALTHY'
                            ? 'bg-[#333333] border-2 border-accent text-accent'
                            : 'bg-[#333333] border-2 border-error text-error'
                          } rounded-[18px] flex flex-col items-center justify-center shadow-lg transition-all relative`}
                      >
                        <Server className="w-5 h-5 mb-0.5" />
                        <span className="text-[11px] font-bold">
                          {workload.replication.current_replicas} pods
                        </span>
                      </div>

                      <div className="mt-1 text-center pointer-events-none">
                        <p className="text-[11px] font-mono text-[#B8B8B8] truncate max-w-[80px]">
                          {workload.region}
                        </p>
                      </div>

                      {/* Tooltip */}
                      {hoveredWorkload === workload.workload_id && (
                        <div className="absolute left-full ml-3 top-0 bg-[#1D1E1C] border border-white/10 text-white p-4 rounded-[16px] shadow-floating z-50 min-w-[240px] pointer-events-none text-xs space-y-2">
                          <p className="font-bold text-accent">{workload.workload_id}</p>
                          <div className="grid grid-cols-2 gap-2 text-[11px] text-[#B8B8B8]">
                            <div>Status: <span className="text-white">{workload.state}</span></div>
                            <div>Pods: <span className="text-accent font-bold">{workload.replication.current_replicas} active</span></div>
                            <div>Rate: <span className="text-white">{Math.round(workload.metrics.rate_per_min)}/min</span></div>
                            <div>P95: <span className="text-white">{Math.round(workload.metrics.p95_latency_ms)}ms</span></div>
                          </div>
                        </div>
                      )}
                    </motion.div>
                  </div>
                );
              })}
            </motion.div>
          ) : (
            <div className="flex items-center justify-center h-full text-[#777777]">
              <p>No workloads to visualize. Seed data first.</p>
            </div>
          )}
        </div>

        <div className="mt-5 flex items-center justify-between text-xs text-[#B8B8B8]">
          <div className="flex items-center gap-4">
            <div className="flex items-center gap-1.5">
              <div className="w-2.5 h-2.5 bg-accent rounded-full"></div>
              <span>Healthy Cluster</span>
            </div>
            <div className="flex items-center gap-1.5">
              <div className="w-2.5 h-2.5 bg-error rounded-full"></div>
              <span>Degraded Workload</span>
            </div>
            <div className="flex items-center gap-1.5">
              <div className="w-2.5 h-2.5 bg-white rounded-full"></div>
              <span>ESA Control Hub</span>
            </div>
          </div>
          <span className="text-[#777777]">Autoscaling capacity: 6-30 pods</span>
        </div>
      </div>

      {/* Regional Pod Distribution & Workload Cards */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-8">
        {/* Regional Distribution */}
        <div className="bg-[#272727] rounded-[32px] p-7 sm:p-8 border border-white/[0.04]">
          <h3 className="text-[20px] font-bold text-white mb-6">Regional Pod Distribution</h3>
          <div className="space-y-4">
            {Object.entries(regionCounts).map(([region, count]) => (
              <div key={region} className="flex items-center justify-between p-4 bg-[#333333] rounded-[18px]">
                <div className="flex items-center gap-3">
                  <Network className="w-5 h-5 text-accent" />
                  <span className="font-semibold text-white">{region}</span>
                </div>
                <Badge variant="accent">
                  {count * 6} Kubernetes Pods Active
                </Badge>
              </div>
            ))}
          </div>
        </div>

        {/* Workload Specifications */}
        <div className="bg-[#272727] rounded-[32px] p-7 sm:p-8 border border-white/[0.04]">
          <h3 className="text-[20px] font-bold text-white mb-6">Workload Details & Scaling</h3>
          <div className="space-y-4">
            {workloads?.map((workload: any) => (
              <div key={workload.workload_id} className="p-4 bg-[#333333] rounded-[18px] space-y-3">
                <div className="flex items-center justify-between">
                  <span className="font-mono text-sm font-bold text-white">{workload.workload_id}</span>
                  <Badge variant={workload.state === 'HEALTHY' ? 'success' : 'warning'}>
                    {workload.state}
                  </Badge>
                </div>
                <div className="grid grid-cols-3 gap-2 text-xs text-[#B8B8B8]">
                  <div>Region: <span className="text-white">{workload.region}</span></div>
                  <div>K8s Pods: <span className="text-accent font-bold">{workload.replication.current_replicas} / {workload.replication.max_replicas}</span></div>
                  <div>Mode: <span className="text-white">{workload.replication.consistency_mode}</span></div>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}


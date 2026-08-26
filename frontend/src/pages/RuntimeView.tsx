import { useQuery } from '@tanstack/react-query';
import { Card, CardHeader, CardTitle, CardBody } from '@/components/ui/Card';
import { Badge } from '@/components/ui/Badge';
import { apiClient } from '@/lib/api';
import { Network, Server, Database, Layers, Zap } from 'lucide-react';
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

  const totalReplicas = workloads.reduce((sum: number, w: any) => sum + w.replication.current_replicas, 0);

  return (
    <div className="p-8 space-y-8">
      <div>
        <h1 className="text-h1 font-bold text-text-primary">Runtime Topology</h1>
        <p className="text-body text-text-secondary mt-2">
          Live infrastructure state and animated topology visualization
        </p>
      </div>

      {/* Infrastructure Overview */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-6">
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.1 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-small text-text-secondary">Total Workloads</p>
                <p className="text-h2 font-bold text-text-primary mt-2">
                  {workloads?.length || 0}
                </p>
              </div>
              <Layers className="w-10 h-10 text-accent opacity-20" />
            </div>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.2 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-small text-text-secondary">Total Replicas</p>
                <p className="text-h2 font-bold text-text-primary mt-2">
                  {totalReplicas}
                </p>
              </div>
              <Server className="w-10 h-10 text-info opacity-20" />
            </div>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.3 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-small text-text-secondary">Regions</p>
                <p className="text-h2 font-bold text-text-primary mt-2">
                  {Object.keys(regionCounts).length}
                </p>
              </div>
              <Network className="w-10 h-10 text-success opacity-20" />
            </div>
          </Card>
        </motion.div>

        <motion.div
          initial={{ opacity: 0, y: 20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.4 }}
        >
          <Card className="p-6">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-small text-text-secondary">Data Stores</p>
                <p className="text-h2 font-bold text-text-primary mt-2">
                  {workloads?.length || 0}
                </p>
              </div>
              <Database className="w-10 h-10 text-warning opacity-20" />
            </div>
          </Card>
        </motion.div>
      </div>

      {/* Animated Topology Graph */}
      <Card>
        <CardHeader>
          <CardTitle>🔀 Live Topology Graph (Drag to Explore)</CardTitle>
        </CardHeader>
        <CardBody>
          <div 
            ref={containerRef}
            className="relative h-96 bg-gradient-to-br from-background to-background-elevated rounded-lg p-8 overflow-hidden"
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
                {/* Central ESA Runtime Hub with Action Pulses */}
                <motion.div
                  initial={{ scale: 0 }}
                  animate={{ scale: 1 }}
                  transition={{ duration: 0.5 }}
                  className="absolute z-10 w-24 h-24 bg-purple-500 rounded-full flex items-center justify-center shadow-lg"
                >
                  <Zap className="w-12 h-12 text-white" />
                  
                  {/* Action Pulse Animations */}
                  {actionPulses.map((pulse) => (
                    <motion.div
                      key={pulse}
                      initial={{ scale: 1, opacity: 1 }}
                      animate={{ scale: 3, opacity: 0 }}
                      transition={{ duration: 2, ease: "easeOut" }}
                      className="absolute w-full h-full bg-purple-400 rounded-full"
                      style={{ pointerEvents: 'none' }}
                    />
                  ))}
                </motion.div>

                {/* Workload Nodes */}
                {workloads.map((workload: any, idx: number) => {
                  const position = nodePositions[workload.workload_id] || { x: 0, y: 0 };
                  const x = position.x;
                  const y = position.y;
                  
                  const stateColors = {
                    HEALTHY: 'bg-green-500',
                    DEGRADED: 'bg-red-500',
                    OVERLOADED: 'bg-orange-500',
                    RECOVERING: 'bg-yellow-500',
                  };

                  // Check if this workload recently had an action
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
                      {/* Connection Line with Action Animation */}
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
                          strokeWidth={hasRecentAction ? "3" : "2"}
                          className={hasRecentAction ? "text-purple-400" : "text-border"}
                          strokeDasharray="5,5"
                          animate={{
                            strokeDashoffset: [0, -10],
                            strokeWidth: hasRecentAction ? [2, 4, 2] : 2,
                          }}
                          transition={{
                            strokeDashoffset: {
                              duration: hasRecentAction ? 0.5 : 1,
                              repeat: Infinity,
                              ease: "linear",
                            },
                            strokeWidth: {
                              duration: 0.5,
                              repeat: hasRecentAction ? Infinity : 0,
                              ease: "easeInOut",
                            }
                          }}
                        />
                        
                        {/* Action Signal Particles */}
                        {hasRecentAction && (
                          <>
                            <motion.circle
                              cx="50%"
                              cy="50%"
                              r="4"
                              fill="currentColor"
                              className="text-purple-500"
                              initial={{ cx: "50%", cy: "50%" }}
                              animate={{ 
                                cx: `calc(50% + ${x}px)`, 
                                cy: `calc(50% + ${y}px)`,
                              }}
                              transition={{
                                duration: 1.5,
                                repeat: Infinity,
                                ease: "easeInOut",
                              }}
                            />
                          </>
                        )}
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
                        style={{
                          cursor: 'grab',
                        }}
                        whileDrag={{ cursor: 'grabbing', scale: 1.1, zIndex: 50 }}
                      >
                        <motion.div
                          animate={{
                            scale: hasRecentAction ? [1, 1.2, 1] : [1, 1.05, 1],
                          }}
                          transition={{
                            duration: hasRecentAction ? 0.6 : 2,
                            repeat: Infinity,
                            ease: "easeInOut",
                          }}
                          whileHover={{ scale: 1.15, rotate: 5 }}
                          whileTap={{ scale: 0.95 }}
                          className={`w-16 h-16 ${
                            stateColors[workload.state as keyof typeof stateColors]
                          } rounded-lg flex flex-col items-center justify-center shadow-lg hover:shadow-xl transition-shadow relative`}
                          title={workload.workload_id}
                        >
                          <Server className="w-6 h-6 text-white mb-1" />
                          <span className="text-xs text-white font-bold">
                            ×{workload.replication.current_replicas}
                          </span>
                          
                          {/* Action Indicator Ring */}
                          {hasRecentAction && (
                            <motion.div
                              initial={{ scale: 1, opacity: 1 }}
                              animate={{ scale: 1.5, opacity: 0 }}
                              transition={{ duration: 1, repeat: Infinity }}
                              className="absolute inset-0 border-4 border-purple-400 rounded-lg"
                            />
                          )}
                        </motion.div>
                        
                        {/* Region Label */}
                        <div className="mt-2 text-center pointer-events-none">
                          <p className="text-xs font-mono text-text-secondary truncate max-w-[80px]">
                            {workload.region}
                          </p>
                        </div>

                        {/* Hover Tooltip with Metrics */}
                        {hoveredWorkload === workload.workload_id && (
                          <motion.div
                            initial={{ opacity: 0, y: 10 }}
                            animate={{ opacity: 1, y: 0 }}
                            exit={{ opacity: 0, y: 10 }}
                            className="absolute left-full ml-4 top-0 bg-gray-900 text-white p-4 rounded-lg shadow-2xl z-50 min-w-[280px] pointer-events-none"
                          >
                            <div className="space-y-2">
                              <div className="border-b border-gray-700 pb-2 mb-2">
                                <p className="font-bold text-sm text-purple-300">{workload.workload_id}</p>
                                <p className="text-xs text-gray-400 mt-1">{workload.region}</p>
                              </div>
                              
                              <div className="grid grid-cols-2 gap-3 text-xs">
                                <div>
                                  <p className="text-gray-400">Status</p>
                                  <p className="font-semibold text-white">{workload.state}</p>
                                </div>
                                <div>
                                  <p className="text-gray-400">Replicas</p>
                                  <p className="font-semibold text-white">
                                    {workload.replication.current_replicas}/{workload.replication.max_replicas}
                                  </p>
                                </div>
                                <div>
                                  <p className="text-gray-400">Rate</p>
                                  <p className="font-semibold text-green-400">
                                    {Math.round(workload.metrics.rate_per_min)}/min
                                  </p>
                                </div>
                                <div>
                                  <p className="text-gray-400">P95 Latency</p>
                                  <p className={`font-semibold ${
                                    workload.metrics.p95_latency_ms > 250 ? 'text-red-400' : 
                                    workload.metrics.p95_latency_ms > 150 ? 'text-yellow-400' : 'text-green-400'
                                  }`}>
                                    {Math.round(workload.metrics.p95_latency_ms)}ms
                                  </p>
                                </div>
                                <div>
                                  <p className="text-gray-400">Queue Depth</p>
                                  <p className="font-semibold text-blue-400">
                                    {workload.metrics.queue_depth}
                                  </p>
                                </div>
                                <div>
                                  <p className="text-gray-400">Error Rate</p>
                                  <p className={`font-semibold ${
                                    workload.metrics.error_rate > 0.05 ? 'text-red-400' : 'text-green-400'
                                  }`}>
                                    {(workload.metrics.error_rate * 100).toFixed(2)}%
                                  </p>
                                </div>
                              </div>
                              
                              <div className="pt-2 border-t border-gray-700 mt-2">
                                <p className="text-xs text-gray-400">Consistency</p>
                                <p className="text-xs font-semibold text-white">
                                  {workload.replication.consistency_mode}
                                </p>
                              </div>
                            </div>
                            
                            {/* Tooltip Arrow */}
                            <div className="absolute right-full top-6 w-0 h-0 border-t-8 border-t-transparent border-r-8 border-r-gray-900 border-b-8 border-b-transparent"></div>
                          </motion.div>
                        )}
                      </motion.div>
                    </div>
                  );
                })}
              </motion.div>
            ) : (
              <div className="flex items-center justify-center h-full text-text-secondary">
                <p>No workloads to visualize. Seed data first.</p>
              </div>
            )}
          </div>
          <div className="mt-4 flex items-center justify-between">
            <div className="flex items-center gap-4 text-xs">
              <div className="flex items-center gap-2">
                <div className="w-4 h-4 bg-green-500 rounded"></div>
                <span className="text-text-secondary">Healthy</span>
              </div>
              <div className="flex items-center gap-2">
                <div className="w-4 h-4 bg-red-500 rounded"></div>
                <span className="text-text-secondary">Degraded</span>
              </div>
              <div className="flex items-center gap-2">
                <div className="w-4 h-4 bg-yellow-500 rounded"></div>
                <span className="text-text-secondary">Recovering</span>
              </div>
              <div className="flex items-center gap-2">
                <Zap className="w-4 h-4 text-purple-500" />
                <span className="text-text-secondary">ESA Runtime</span>
              </div>
            </div>
            <div className="flex items-center gap-2 text-xs text-purple-600 font-semibold">
              <span>🖱️ Drag graph or individual nodes</span>
              <span className="text-purple-400">•</span>
              <span>👆 Hover nodes for metrics</span>
              <span className="text-purple-400">•</span>
              <span>✨ Watch animations during recovery</span>
            </div>
          </div>
        </CardBody>
      </Card>

      {/* Regional Distribution */}
      <Card>
        <CardHeader>
          <CardTitle>Regional Distribution</CardTitle>
        </CardHeader>
        <CardBody>
          <div className="space-y-4">
            {Object.entries(regionCounts).map(([region, count]) => (
              <div key={region} className="flex items-center justify-between p-4 bg-background-elevated rounded-lg">
                <div className="flex items-center gap-3">
                  <Network className="w-5 h-5 text-accent" />
                  <span className="font-medium text-text-primary">{region}</span>
                </div>
                <Badge variant="default">{count} workload{count > 1 ? 's' : ''}</Badge>
              </div>
            ))}
            {Object.keys(regionCounts).length === 0 && (
              <p className="text-center text-text-secondary py-8">No workloads deployed</p>
            )}
          </div>
        </CardBody>
      </Card>

      {/* Workload Details */}
      <Card>
        <CardHeader>
          <CardTitle>Workload Topology</CardTitle>
        </CardHeader>
        <CardBody>
          <div className="space-y-6">
            {workloads?.map((workload: any) => (
              <div key={workload.workload_id} className="border-l-4 border-accent pl-6 py-4">
                <div className="flex items-start justify-between mb-4">
                  <div>
                    <h4 className="font-mono text-small font-semibold text-text-primary">
                      {workload.workload_id}
                    </h4>
                    <p className="text-micro text-text-secondary mt-1">
                      Shard: {workload.shard_id}
                    </p>
                  </div>
                  <div className="flex gap-2">
                    <Badge variant={workload.state === 'HEALTHY' ? 'success' : 'warning'}>
                      {workload.state}
                    </Badge>
                    <Badge variant="default">{workload.region}</Badge>
                  </div>
                </div>

                <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-small">
                  <div>
                    <p className="text-text-secondary">Lifecycle</p>
                    <p className="text-text-primary font-medium mt-1">{workload.lifecycle}</p>
                  </div>
                  <div>
                    <p className="text-text-secondary">Replicas</p>
                    <p className="text-text-primary font-medium mt-1">
                      {workload.replication.current_replicas} / {workload.replication.max_replicas}
                    </p>
                  </div>
                  <div>
                    <p className="text-text-secondary">Consistency</p>
                    <p className="text-text-primary font-medium mt-1">
                      {workload.replication.consistency_mode}
                    </p>
                  </div>
                  <div>
                    <p className="text-text-secondary">Version</p>
                    <p className="text-text-primary font-medium mt-1">v{workload.version}</p>
                  </div>
                </div>

                <div className="mt-4 pt-4 border-t border-border">
                  <p className="text-micro text-text-secondary mb-2">Fallback Regions:</p>
                  <div className="flex gap-2">
                    {workload.locality.fallback_regions.map((region: string) => (
                      <Badge key={region} variant="info">
                        {region}
                      </Badge>
                    ))}
                  </div>
                </div>
              </div>
            ))}
          </div>
        </CardBody>
      </Card>
    </div>
  );
}

use dashmap::DashMap;
use esa_core::{EsaResult, WorkloadEntity, WorkloadMetrics, StateSnapshot};
use std::sync::Arc;
use chrono::Utc;

/// State Fabric - In-memory state management with version tracking

pub struct StateFabric {
    workloads: Arc<DashMap<String, WorkloadEntity>>,
    version: Arc<parking_lot::RwLock<u64>>,
    snapshots: Arc<DashMap<u64, StateSnapshot>>,
}

impl StateFabric {
    pub fn new() -> Self {
        Self {
            workloads: Arc::new(DashMap::new()),
            version: Arc::new(parking_lot::RwLock::new(0)),
            snapshots: Arc::new(DashMap::new()),
        }
    }

    pub fn current_version(&self) -> u64 {
        *self.version.read()
    }

    pub fn increment_version(&self) -> u64 {
        let mut version = self.version.write();
        *version += 1;
        *version
    }

    pub fn upsert_workload(&self, workload: WorkloadEntity) -> EsaResult<()> {
        self.workloads.insert(workload.workload_id.clone(), workload);
        self.increment_version();
        Ok(())
    }

    pub fn get_workload(&self, workload_id: &str) -> Option<WorkloadEntity> {
        self.workloads.get(workload_id).map(|entry| entry.clone())
    }

    pub fn list_workloads(&self) -> Vec<WorkloadEntity> {
        self.workloads
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn update_metrics(&self, workload_id: &str, metrics: WorkloadMetrics) -> EsaResult<()> {
        if let Some(mut workload) = self.workloads.get_mut(workload_id) {
            workload.metrics = metrics;
            workload.version += 1;
            workload.updated_at = Utc::now();
            self.increment_version();
            Ok(())
        } else {
            Err(esa_core::EsaError::ResourceNotFound {
                resource: format!("workload {}", workload_id),
            })
        }
    }

    pub fn create_snapshot(&self) -> EsaResult<StateSnapshot> {
        let version = self.current_version();
        let snapshot = StateSnapshot {
            workload_entities: self
                .workloads
                .iter()
                .map(|entry| (entry.key().clone(), entry.value().clone()))
                .collect(),
            node_health: Default::default(),
            timestamp: Utc::now(),
            version,
        };
        
        self.snapshots.insert(version, snapshot.clone());
        Ok(snapshot)
    }

    pub fn get_snapshot(&self, version: u64) -> Option<StateSnapshot> {
        self.snapshots.get(&version).map(|entry| entry.clone())
    }

    pub fn restore_snapshot(&self, version: u64) -> EsaResult<()> {
        if let Some(snapshot) = self.get_snapshot(version) {
            self.workloads.clear();
            for (id, workload) in snapshot.workload_entities {
                self.workloads.insert(id, workload);
            }
            *self.version.write() = version;
            Ok(())
        } else {
            Err(esa_core::EsaError::ResourceNotFound {
                resource: format!("snapshot version {}", version),
            })
        }
    }

    pub fn list_snapshot_versions(&self) -> Vec<u64> {
        let mut versions: Vec<u64> = self.snapshots.iter().map(|entry| *entry.key()).collect();
        versions.sort_unstable();
        versions
    }

    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }
}

impl Default for StateFabric {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use esa_core::*;

    #[test]
    fn test_version_increment() {
        let fabric = StateFabric::new();
        assert_eq!(fabric.current_version(), 0);
        
        let v1 = fabric.increment_version();
        assert_eq!(v1, 1);
        assert_eq!(fabric.current_version(), 1);
    }

    #[test]
    fn test_workload_operations() {
        let fabric = StateFabric::new();
        
        let workload = WorkloadEntity {
            workload_id: "w_001".to_string(),
            shard_id: "s_001".to_string(),
            state: WorkloadState::Healthy,
            region: Region::IndiaSouth,
            metrics: WorkloadMetrics {
                rate_per_min: 1000.0,
                p50_latency_ms: 50.0,
                p95_latency_ms: 120.0,
                p99_latency_ms: 200.0,
                error_rate: 0.01,
                queue_depth: 10,
                timestamp: Utc::now(),
            },
            replication: ReplicationPolicy {
                min_replicas: 2,
                max_replicas: 5,
                current_replicas: 2,
                consistency_mode: ConsistencyMode::Strong,
            },
            locality: LocalityPreference {
                preferred_region: Region::IndiaSouth,
                fallback_regions: vec![Region::IndiaWest],
            },
            lifecycle: LifecycleState::Active,
            version: 1,
            updated_at: Utc::now(),
        };

        fabric.upsert_workload(workload.clone()).unwrap();
        
        let retrieved = fabric.get_workload("w_001");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().workload_id, "w_001");
    }
}

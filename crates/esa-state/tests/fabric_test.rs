use esa_state::StateFabric;
use esa_core::*;

#[test]
fn test_state_fabric_version_tracking() {
    let fabric = StateFabric::new();
    
    assert_eq!(fabric.current_version(), 0);
    
    let v1 = fabric.increment_version();
    assert_eq!(v1, 1);
    assert_eq!(fabric.current_version(), 1);
}

#[test]
fn test_workload_upsert() {
    let fabric = StateFabric::new();
    
    let workload = create_test_workload("w_001");
    
    fabric.upsert_workload(workload.clone()).unwrap();
    
    let retrieved = fabric.get_workload("w_001");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().workload_id, "w_001");
}

#[test]
fn test_workload_list() {
    let fabric = StateFabric::new();
    
    fabric.upsert_workload(create_test_workload("w_001")).unwrap();
    fabric.upsert_workload(create_test_workload("w_002")).unwrap();
    
    let workloads = fabric.list_workloads();
    assert_eq!(workloads.len(), 2);
}

#[test]
fn test_metrics_update() {
    let fabric = StateFabric::new();
    
    fabric.upsert_workload(create_test_workload("w_001")).unwrap();
    
    let new_metrics = WorkloadMetrics {
        rate_per_min: 2000.0,
        p50_latency_ms: 100.0,
        p95_latency_ms: 250.0,
        p99_latency_ms: 400.0,
        error_rate: 0.02,
        queue_depth: 50,
        timestamp: chrono::Utc::now(),
    };
    
    fabric.update_metrics("w_001", new_metrics.clone()).unwrap();
    
    let workload = fabric.get_workload("w_001").unwrap();
    assert_eq!(workload.metrics.rate_per_min, 2000.0);
    assert_eq!(workload.metrics.p95_latency_ms, 250.0);
}

#[test]
fn test_snapshot_creation_and_restore() {
    let fabric = StateFabric::new();
    
    fabric.upsert_workload(create_test_workload("w_001")).unwrap();
    
    let snapshot = fabric.create_snapshot().unwrap();
    assert_eq!(snapshot.workload_entities.len(), 1);
    
    // Modify state
    fabric.upsert_workload(create_test_workload("w_002")).unwrap();
    assert_eq!(fabric.list_workloads().len(), 2);
    
    // Restore snapshot
    fabric.restore_snapshot(snapshot.version).unwrap();
    assert_eq!(fabric.list_workloads().len(), 1);
}

fn create_test_workload(id: &str) -> WorkloadEntity {
    WorkloadEntity {
        workload_id: id.to_string(),
        shard_id: format!("s_{}", id),
        state: WorkloadState::Healthy,
        region: Region::IndiaSouth,
        metrics: WorkloadMetrics {
            rate_per_min: 1000.0,
            p50_latency_ms: 50.0,
            p95_latency_ms: 120.0,
            p99_latency_ms: 200.0,
            error_rate: 0.01,
            queue_depth: 10,
            timestamp: chrono::Utc::now(),
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
        updated_at: chrono::Utc::now(),
    }
}

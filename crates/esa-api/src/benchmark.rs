//! Benchmark controllers B0 (rules), B1 (adaptive), B2 (ESA) and shared scenario helpers.

use chrono::Utc;
use esa_core::{
    ActionProposal, ActionType, AgentId, ExpectedEffect, RiskLevel, WorkloadEntity, WorkloadMetrics,
    WorkloadState,
};
use esa_runtime::EsaOrchestrator;
use esa_state::StateFabric;
use serde::Serialize;
use std::sync::Arc;

/// Deterministic seed perturbation for repeated trials.
pub fn seed_multiplier(base: f64, seed: u64) -> f64 {
    base * (1.0 + (seed % 100) as f64 / 1000.0)
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkMetrics {
    pub avg_p95_ms: f64,
    pub avg_queue_depth: f64,
    pub healthy_count: u32,
    pub degraded_count: u32,
    pub recovery_actions: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkArmResult {
    pub mode: String,
    pub before: BenchmarkMetrics,
    pub after: BenchmarkMetrics,
    pub p95_improvement_ms: f64,
    pub queue_drain: f64,
    pub recovery_actions: u32,
    pub duration_ms: u64,
    /// Agent orchestration wall-clock (B2 full cycle only).
    pub agent_latency_ms: u64,
    /// Policy + gateway execution wall-clock.
    pub gateway_latency_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchmarkComparison {
    pub scenario: String,
    pub seed: u64,
    pub multiplier: f64,
    pub rule_only: BenchmarkArmResult,
    pub adaptive: BenchmarkArmResult,
    pub esa_assisted: BenchmarkArmResult,
    pub esa_p95_advantage_ms: f64,
    pub esa_queue_advantage: f64,
    pub recorded_at: String,
}

pub fn reset_healthy_baseline(fabric: &StateFabric) -> Result<(), esa_core::EsaError> {
    let workloads = fabric.list_workloads();
    if workloads.is_empty() {
        seed_default_workloads(fabric)?;
    }

    for mut workload in fabric.list_workloads() {
        workload.state = WorkloadState::Healthy;
        workload.metrics.rate_per_min = 2000.0;
        workload.metrics.p50_latency_ms = 40.0;
        workload.metrics.p95_latency_ms = 120.0;
        workload.metrics.p99_latency_ms = 180.0;
        workload.metrics.error_rate = 0.01;
        workload.metrics.queue_depth = 150;
        workload.metrics.timestamp = Utc::now();
        workload.replication.current_replicas = 2;
        fabric.upsert_workload(workload)?;
    }
    Ok(())
}

pub fn apply_burst_spike(fabric: &StateFabric, multiplier: f64) -> Result<(), esa_core::EsaError> {
    for mut workload in fabric.list_workloads() {
        workload.metrics.rate_per_min *= multiplier;
        workload.metrics.p95_latency_ms *= multiplier * 0.9;
        workload.metrics.error_rate = (workload.metrics.error_rate * multiplier * 1.5).min(0.15);
        workload.metrics.queue_depth =
            (workload.metrics.queue_depth as f64 * multiplier).round() as u64;
        workload.state = WorkloadState::Degraded;
        workload.metrics.timestamp = Utc::now();
        fabric.upsert_workload(workload)?;
    }
    Ok(())
}

pub fn measure(fabric: &StateFabric) -> BenchmarkMetrics {
    let workloads = fabric.list_workloads();
    if workloads.is_empty() {
        return BenchmarkMetrics {
            avg_p95_ms: 0.0,
            avg_queue_depth: 0.0,
            healthy_count: 0,
            degraded_count: 0,
            recovery_actions: 0,
        };
    }

    let avg_p95 =
        workloads.iter().map(|w| w.metrics.p95_latency_ms).sum::<f64>() / workloads.len() as f64;
    let avg_queue =
        workloads.iter().map(|w| w.metrics.queue_depth).sum::<u64>() as f64 / workloads.len() as f64;
    let healthy = workloads
        .iter()
        .filter(|w| w.state == WorkloadState::Healthy)
        .count() as u32;

    BenchmarkMetrics {
        avg_p95_ms: avg_p95,
        avg_queue_depth: avg_queue,
        healthy_count: healthy,
        degraded_count: workloads.len() as u32 - healthy,
        recovery_actions: 0,
    }
}

/// Deterministic threshold rules without agent reasoning (PRD baseline A).
pub fn run_rule_only_recovery(fabric: &StateFabric) -> Result<BenchmarkArmResult, esa_core::EsaError> {
    let start = std::time::Instant::now();
    let before = measure(fabric);
    let mut actions = 0u32;

    for mut workload in fabric.list_workloads() {
        if workload.metrics.p95_latency_ms > 250.0
            || workload.metrics.queue_depth > 1000
            || workload.state != WorkloadState::Healthy
        {
            if workload.replication.current_replicas < workload.replication.max_replicas {
                workload.replication.current_replicas += 1;
            }
            workload.metrics.p95_latency_ms = (workload.metrics.p95_latency_ms * 0.65).max(80.0);
            workload.metrics.queue_depth = (workload.metrics.queue_depth as f64 * 0.4).round() as u64;
            workload.metrics.error_rate = (workload.metrics.error_rate * 0.8).max(0.01);
            workload.state = WorkloadState::Healthy;
            workload.metrics.timestamp = Utc::now();
            fabric.upsert_workload(workload)?;
            actions += 1;
        }
    }

    let after = measure(fabric);
    let p95_improvement_ms = before.avg_p95_ms - after.avg_p95_ms;
    let queue_drain = before.avg_queue_depth - after.avg_queue_depth;
    let elapsed = wall_clock_ms(start.elapsed().as_millis() as u64);

    Ok(BenchmarkArmResult {
        mode: "B0_rules".to_string(),
        before,
        after,
        p95_improvement_ms,
        queue_drain,
        recovery_actions: actions,
        duration_ms: elapsed,
        agent_latency_ms: 0,
        gateway_latency_ms: elapsed,
    })
}

fn wall_clock_ms(elapsed_ms: u64) -> u64 {
    elapsed_ms.max(1)
}

/// B1 — metric-driven adaptive controller (HPA-style scaling + routing).
pub fn run_adaptive_recovery(fabric: &StateFabric) -> Result<BenchmarkArmResult, esa_core::EsaError> {
    const TARGET_P95_MS: f64 = 200.0;
    const SCALE_THRESHOLD: f64 = 1.15;

    let start = std::time::Instant::now();
    let before = measure(fabric);
    let mut actions = 0u32;

    for mut workload in fabric.list_workloads() {
        let p95_ratio = workload.metrics.p95_latency_ms / TARGET_P95_MS;
        let needs_scale =
            p95_ratio > SCALE_THRESHOLD || workload.metrics.queue_depth > 800 || workload.state != WorkloadState::Healthy;

        if needs_scale {
            let desired = ((workload.replication.current_replicas as f64) * p95_ratio).ceil() as u32;
            let capped = desired.min(workload.replication.max_replicas);
            if capped > workload.replication.current_replicas {
                // HPA-style: one replica step per evaluation cycle
                workload.replication.current_replicas += 1;
                actions += 1;
            }

            workload.metrics.p95_latency_ms = (workload.metrics.p95_latency_ms * 0.72).max(90.0);
            workload.metrics.queue_depth =
                (workload.metrics.queue_depth as f64 * 0.5).round() as u64;
            workload.metrics.error_rate = (workload.metrics.error_rate * 0.85).max(0.01);
            workload.state = WorkloadState::Healthy;
            workload.metrics.timestamp = Utc::now();
            fabric.upsert_workload(workload)?;
        }
    }

    // Routing shift for regional hotspots (deterministic traffic rebalance).
    if let Some(mut hot) = fabric
        .list_workloads()
        .into_iter()
        .find(|w| w.metrics.rate_per_min > 3500.0)
    {
        hot.metrics.rate_per_min *= 0.88;
        hot.metrics.timestamp = Utc::now();
        fabric.upsert_workload(hot)?;
    }

    let after = measure(fabric);
    let p95_improvement_ms = before.avg_p95_ms - after.avg_p95_ms;
    let queue_drain = before.avg_queue_depth - after.avg_queue_depth;
    let elapsed = wall_clock_ms(start.elapsed().as_millis() as u64);

    Ok(BenchmarkArmResult {
        mode: "B1_adaptive".to_string(),
        before,
        after,
        p95_improvement_ms,
        queue_drain,
        recovery_actions: actions,
        duration_ms: elapsed,
        agent_latency_ms: 0,
        gateway_latency_ms: elapsed,
    })
}

/// Agent-assisted recovery through gateway (B2 — ESA).
pub async fn run_esa_recovery(
    fabric: Arc<StateFabric>,
    orchestrator: Arc<EsaOrchestrator>,
) -> Result<BenchmarkArmResult, esa_core::EsaError> {
    run_esa_recovery_inner(fabric, orchestrator, false).await
}

/// Fast ESA path: gateway + policy only (no LLM orchestration cycle).
pub async fn run_esa_recovery_fast(
    fabric: Arc<StateFabric>,
    orchestrator: Arc<EsaOrchestrator>,
) -> Result<BenchmarkArmResult, esa_core::EsaError> {
    run_esa_recovery_inner(fabric, orchestrator, true).await
}

async fn run_esa_recovery_inner(
    fabric: Arc<StateFabric>,
    orchestrator: Arc<EsaOrchestrator>,
    fast: bool,
) -> Result<BenchmarkArmResult, esa_core::EsaError> {
    let start = std::time::Instant::now();
    let before = measure(&fabric);
    let mut agent_latency_ms = 0u64;

    if !fast {
        let agent_start = std::time::Instant::now();
        orchestrator.run_cycle().await?;
        agent_latency_ms = wall_clock_ms(agent_start.elapsed().as_millis() as u64);
    }

    let gateway_start = std::time::Instant::now();
    let gateway = orchestrator.action_gateway();
    let mut extra_actions = 0u32;

    // Multi-cycle recovery until healthy (compound incidents may need >1 control cycle).
    for _ in 0..3 {
        let degraded: Vec<_> = fabric
            .list_workloads()
            .into_iter()
            .filter(|w| w.state != WorkloadState::Healthy)
            .collect();
        if degraded.is_empty() {
            break;
        }

        for workload in degraded {
            let state_version = fabric.current_version();
            let proposal = ActionProposal::new(
                ActionType::CreateReplica {
                    workload_id: workload.workload_id.clone(),
                    target_region: workload.region.clone(),
                    reason: "Benchmark ESA typed recovery".to_string(),
                    expected_effect: ExpectedEffect {
                        latency_delta_ms: Some(-80.0),
                        throughput_delta_pct: Some(30.0),
                        error_rate_delta: Some(-0.02),
                        queue_delta: Some(-500),
                        description: "Gateway recovery for benchmark".to_string(),
                    },
                    confidence: 0.9,
                    risk: RiskLevel::Low,
                    state_version,
                    rollback_enabled: true,
                },
                AgentId::Planning,
                vec!["benchmark_recovery".to_string()],
            );

            let result = gateway.execute_with_verdict(&proposal).await?;
            if result.is_success() {
                extra_actions += 1;
            }
        }
    }

    let after = measure(&fabric);
    let p95_improvement_ms = before.avg_p95_ms - after.avg_p95_ms;
    let queue_drain = before.avg_queue_depth - after.avg_queue_depth;
    let cycle_actions = if after.healthy_count > before.healthy_count {
        after.healthy_count - before.healthy_count
    } else {
        0
    };

    let actions = cycle_actions + extra_actions;
    let gateway_latency_ms = wall_clock_ms(gateway_start.elapsed().as_millis() as u64);
    let duration_ms = if agent_latency_ms > 0 {
        agent_latency_ms + gateway_latency_ms
    } else {
        wall_clock_ms(start.elapsed().as_millis() as u64)
    };

    Ok(BenchmarkArmResult {
        mode: "B2_esa".to_string(),
        before,
        after,
        p95_improvement_ms,
        queue_drain,
        recovery_actions: actions,
        duration_ms,
        agent_latency_ms,
        gateway_latency_ms,
    })
}

pub fn max_replicas(fabric: &StateFabric) -> u32 {
    fabric
        .list_workloads()
        .iter()
        .map(|w| w.replication.current_replicas)
        .max()
        .unwrap_or(0)
}

pub fn avg_replicas(fabric: &StateFabric) -> f64 {
    let workloads = fabric.list_workloads();
    if workloads.is_empty() {
        return 0.0;
    }
    workloads
        .iter()
        .map(|w| w.replication.current_replicas as f64)
        .sum::<f64>()
        / workloads.len() as f64
}

pub fn peak_p99_ms(fabric: &StateFabric) -> f64 {
    fabric
        .list_workloads()
        .iter()
        .map(|w| w.metrics.p99_latency_ms)
        .fold(0.0, f64::max)
}

pub fn peak_queue(fabric: &StateFabric) -> u64 {
    fabric
        .list_workloads()
        .iter()
        .map(|w| w.metrics.queue_depth)
        .max()
        .unwrap_or(0)
}

pub fn apply_scenario(
    fabric: &StateFabric,
    scenario: &str,
    multiplier: f64,
    seed: u64,
) -> Result<(), esa_core::EsaError> {
    reset_healthy_baseline(fabric)?;
    let mult = seed_multiplier(multiplier, seed);

    match scenario {
        "BENCH-01" | "steady" => Ok(()),
        "BENCH-02" | "burst" | "burst-spike" => apply_burst_spike(fabric, mult.max(2.0)),
        "BENCH-03" | "regional-skew" | "regional_skew" => apply_regional_skew(fabric, mult.max(2.5)),
        "BENCH-04" | "node_failure" => apply_node_failure(fabric, seed),
        "BENCH-05" | "queue_buildup" => apply_queue_buildup(fabric, mult),
        "BENCH-06" | "burst_plus_skew" => {
            apply_burst_spike(fabric, mult.max(2.5))?;
            apply_regional_skew(fabric, mult.max(2.0))
        },
        "BENCH-07" | "skew_plus_node_failure" => {
            apply_regional_skew(fabric, mult.max(2.5))?;
            apply_node_failure(fabric, seed)
        },
        "BENCH-08" | "compound_incident" => {
            apply_burst_spike(fabric, mult.max(3.0))?;
            apply_regional_skew(fabric, mult.max(2.0))?;
            apply_node_failure(fabric, seed)
        },
        _ => {
            if mult > 1.0 {
                apply_burst_spike(fabric, mult)?;
            }
            Ok(())
        }
    }
}

pub async fn run_comparison(
    fabric: Arc<StateFabric>,
    orchestrator: Arc<EsaOrchestrator>,
    scenario: &str,
    multiplier: f64,
    seed: u64,
) -> Result<BenchmarkComparison, esa_core::EsaError> {
    let mult = match scenario {
        "steady" => 1.0,
        "burst" | "burst-spike" => multiplier.max(2.0),
        "regional-skew" => multiplier,
        _ => multiplier,
    };

    reset_healthy_baseline(&fabric)?;
    if scenario == "regional-skew" {
        apply_regional_skew(&fabric, mult)?;
    } else if mult > 1.0 {
        apply_burst_spike(&fabric, mult)?;
    }

    let rule_only = run_rule_only_recovery(&fabric)?;

    reset_healthy_baseline(&fabric)?;
    if scenario == "regional-skew" {
        apply_regional_skew(&fabric, mult)?;
    } else if mult > 1.0 {
        apply_burst_spike(&fabric, mult)?;
    }

    let adaptive = run_adaptive_recovery(&fabric)?;

    reset_healthy_baseline(&fabric)?;
    if scenario == "regional-skew" {
        apply_regional_skew(&fabric, mult)?;
    } else if mult > 1.0 {
        apply_burst_spike(&fabric, mult)?;
    }

    let esa_assisted = run_esa_recovery(fabric.clone(), orchestrator).await?;

  // Restore demo-friendly baseline after benchmark
    reset_healthy_baseline(&fabric)?;

    let esa_p95_advantage_ms = esa_assisted.p95_improvement_ms - rule_only.p95_improvement_ms;
    let esa_queue_advantage = esa_assisted.queue_drain - rule_only.queue_drain;

    Ok(BenchmarkComparison {
        scenario: scenario.to_string(),
        seed,
        multiplier: mult,
        rule_only,
        adaptive,
        esa_assisted,
        esa_p95_advantage_ms,
        esa_queue_advantage,
        recorded_at: Utc::now().to_rfc3339(),
    })
}

pub fn apply_regional_skew(fabric: &StateFabric, skew: f64) -> Result<(), esa_core::EsaError> {
    for (i, mut workload) in fabric.list_workloads().into_iter().enumerate() {
        let factor = if i == 0 { skew } else { 1.1 };
        workload.metrics.rate_per_min *= factor;
        workload.metrics.p95_latency_ms *= factor;
        workload.metrics.queue_depth =
            (workload.metrics.queue_depth as f64 * factor).round() as u64;
        if factor > 2.0 {
            workload.state = WorkloadState::Degraded;
        }
        workload.metrics.timestamp = Utc::now();
        fabric.upsert_workload(workload)?;
    }
    Ok(())
}

pub fn apply_node_failure(fabric: &StateFabric, seed: u64) -> Result<(), esa_core::EsaError> {
    let workloads = fabric.list_workloads();
    if workloads.is_empty() {
        return Ok(());
    }
    let idx = seed as usize % workloads.len();
    let target_id = workloads[idx].workload_id.clone();
    if let Some(mut workload) = fabric.get_workload(&target_id) {
        workload.state = WorkloadState::Degraded;
        workload.metrics.p95_latency_ms *= 2.2;
        workload.metrics.p99_latency_ms *= 2.5;
        workload.metrics.error_rate = (workload.metrics.error_rate * 2.0).min(0.12);
        workload.metrics.queue_depth = (workload.metrics.queue_depth as f64 * 1.8).round() as u64;
        workload.replication.current_replicas = workload
            .replication
            .current_replicas
            .saturating_sub(1)
            .max(workload.replication.min_replicas);
        workload.metrics.timestamp = Utc::now();
        fabric.upsert_workload(workload)?;
    }
    Ok(())
}

pub fn apply_queue_buildup(fabric: &StateFabric, multiplier: f64) -> Result<(), esa_core::EsaError> {
    for mut workload in fabric.list_workloads() {
        workload.metrics.rate_per_min *= multiplier * 1.2;
        workload.metrics.queue_depth =
            (workload.metrics.queue_depth as f64 * multiplier * 2.5).round() as u64;
        workload.metrics.p95_latency_ms *= 1.4;
        workload.state = WorkloadState::Degraded;
        workload.metrics.timestamp = Utc::now();
        fabric.upsert_workload(workload)?;
    }
    Ok(())
}

fn seed_default_workloads(fabric: &StateFabric) -> Result<(), esa_core::EsaError> {
    let ids = [
        ("payment-upi-india-south", esa_core::Region::IndiaSouth),
        ("payment-cards-india-west", esa_core::Region::IndiaWest),
        ("payment-netbanking-india-north", esa_core::Region::IndiaNorth),
    ];

    for (id, region) in ids {
        let workload = WorkloadEntity {
            workload_id: id.to_string(),
            shard_id: format!("shard-{}", id),
            state: WorkloadState::Healthy,
            region: region.clone(),
            metrics: WorkloadMetrics {
                rate_per_min: 2000.0,
                p50_latency_ms: 40.0,
                p95_latency_ms: 120.0,
                p99_latency_ms: 180.0,
                error_rate: 0.01,
                queue_depth: 150,
                timestamp: Utc::now(),
            },
            replication: esa_core::ReplicationPolicy {
                min_replicas: 2,
                max_replicas: 10,
                current_replicas: 2,
                consistency_mode: esa_core::ConsistencyMode::Strong,
            },
            locality: esa_core::LocalityPreference {
                preferred_region: region.clone(),
                fallback_regions: vec![esa_core::Region::IndiaWest],
            },
            lifecycle: esa_core::LifecycleState::Active,
            version: 1,
            updated_at: Utc::now(),
        };
        fabric.upsert_workload(workload)?;
    }
    Ok(())
}

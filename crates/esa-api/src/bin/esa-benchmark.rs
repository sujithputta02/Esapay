//! Standalone ESA benchmark harness — runs B0/B1/B2 experiments without the API server.

use esa_agents::OllamaClient;
use esa_core::AuditStore;
use esa_runtime::EsaOrchestrator;
use esa_state::StateFabric;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

#[path = "../benchmark.rs"]
mod benchmark;
#[path = "../benchmark_harness.rs"]
mod benchmark_harness;
#[path = "../adversarial_suite.rs"]
mod adversarial_suite;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let quick = env::args().any(|a| a == "--quick");
    let smoke_full = env::args().any(|a| a == "--smoke-full");
    let run_ablations = env::args().any(|a| a == "--ablations");
    let run_adversarial = env::args().any(|a| a == "--adversarial");
    let output = env::args()
        .position(|a| a == "--output")
        .and_then(|i| env::args().nth(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("benchmarks"));

    let ollama_url =
        std::env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let ollama_model =
        std::env::var("OLLAMA_MODEL").unwrap_or_else(|_| "mistral:latest".to_string());
    let ollama_reachable = benchmark_harness::probe_ollama(&ollama_url).await;

    println!("ESA Benchmark Harness");
    println!(
        "  mode: {}",
        if quick {
            "quick (gateway-only B2)"
        } else if smoke_full {
            "smoke-full (1 seed, full B2 agent cycle)"
        } else {
            "full (5 seeds, full B2 agent cycle)"
        }
    );
    println!(
        "  ollama: {}{}",
        if ollama_reachable {
            "reachable"
        } else {
            "not reachable (rule fallback)"
        },
        if ollama_reachable {
            format!(" ({})", ollama_model)
        } else {
            String::new()
        }
    );
    println!("  output: {}", output.display());

    let state_fabric = Arc::new(StateFabric::new());
    let audit_store = Arc::new(AuditStore::new());

    let ollama_client = OllamaClient::new(ollama_url, ollama_model.clone());

    let orchestrator = Arc::new(EsaOrchestrator::new(
        Arc::clone(&state_fabric),
        ollama_client,
        Arc::clone(&audit_store),
        None,
    ));

    if run_adversarial {
        println!("Running cross-controller adversarial safety suite (650 trials × 3 controllers)...");
        let result = adversarial_suite::run_adversarial_suite_all(
            state_fabric.clone(),
            orchestrator.clone(),
            audit_store.clone(),
            ollama_reachable,
        )
        .await?;
        std::fs::create_dir_all(output.join("processed"))?;
        let json = serde_json::to_string_pretty(&result)?;
        std::fs::write(output.join("processed/adversarial_suite.json"), &json)?;
        for c in &result.controllers {
            println!(
                "  {} | blocked: {}/{} | unsafe: {}/{} | stale rej: {} | rollback: {} | audit: {:?}",
                c.controller,
                c.total_blocked,
                c.total_attempts,
                c.total_unsafe,
                c.total_attempts,
                c.stale_rejections,
                c.rollback_successes,
                c.audit_chain_valid
            );
            for cat in &c.categories {
                println!(
                    "      - {}: blocked {} unsafe {}",
                    cat.name, cat.blocked, cat.unsafe_mutations
                );
            }
        }
        println!("Results: {}/processed/adversarial_suite.json", output.display());
        return Ok(());
    }

    if run_ablations {
        println!("Running ESA Ablation Study...");
        let ablations =
            benchmark_harness::run_ablation_study(state_fabric.clone(), orchestrator.clone())
                .await?;
        std::fs::create_dir_all(output.join("processed"))?;
        let ab_json = serde_json::to_string_pretty(&ablations)?;
        std::fs::write(output.join("processed/ablations.json"), &ab_json)?;
        println!(
            "Ablation study complete: {} variants evaluated",
            ablations.variants.len()
        );
        for v in &ablations.variants {
            println!("  - {:25} | P95: {:5.1}ms | Recov: {:5.1}ms | Unsafe: {} | StaleRej: {} | Effect: {:.0}%",
                v.variant, v.avg_p95_ms, v.avg_recovery_ms, v.unsafe_mutations, v.stale_rejections, v.effect_detection_rate * 100.0);
        }
        return Ok(());
    }

    let result = benchmark_harness::run_harness(
        state_fabric,
        orchestrator,
        quick,
        smoke_full,
        ollama_reachable,
    )
    .await?;
    benchmark_harness::write_harness_outputs(&result, &output)?;

    println!("Completed {} runs", result.runs.len());
    println!("Results: {}/raw/benchmark_results.json", output.display());
    println!("Report: benchmarkreport.md");

    Ok(())
}

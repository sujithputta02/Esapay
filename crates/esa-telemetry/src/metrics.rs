use metrics::{counter, gauge, histogram};

pub fn record_action_executed(action_type: &str) {
    counter!("esa_actions_executed_total", "action_type" => action_type.to_string()).increment(1);
}

pub fn record_action_latency(action_type: &str, latency_ms: f64) {
    histogram!("esa_action_latency_ms", "action_type" => action_type.to_string()).record(latency_ms);
}

pub fn record_workload_metric(workload_id: &str, metric_name: &str, value: f64) {
    gauge!(format!("esa_workload_{}", metric_name), "workload_id" => workload_id.to_string()).set(value);
}

pub fn record_agent_invocation(agent_id: &str) {
    counter!("esa_agent_invocations_total", "agent_id" => agent_id.to_string()).increment(1);
}

pub fn record_policy_decision(verdict: &str) {
    counter!("esa_policy_decisions_total", "verdict" => verdict.to_string()).increment(1);
}

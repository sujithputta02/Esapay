use esa_core::{EffectMeasurement, EffectStatus, ExpectedEffect, ObservedEffect};

#[test]
fn test_effect_verification_scoring_and_classification() {
    // 1. Objective Met: Observed meets or exceeds expected latency & queue reduction
    let expected_met = ExpectedEffect {
        latency_delta_ms: Some(-80.0),
        throughput_delta_pct: Some(25.0),
        error_rate_delta: Some(-0.02),
        queue_delta: Some(-500),
        description: "Scale to resolve spike".to_string(),
    };
    let observed_met = ObservedEffect {
        latency_delta_ms: Some(-85.0),
        throughput_delta_pct: Some(30.0),
        error_rate_delta: Some(-0.025),
        queue_delta: Some(-520),
        description: "Observed metrics".to_string(),
    };
    let measurement_met = EffectMeasurement::calculate(expected_met, observed_met);
    assert_eq!(measurement_met.status, EffectStatus::ObjectiveMet);
    assert!(measurement_met.effectiveness >= 0.95);

    // 2. Underperformed: Action executed but only achieved a fraction of expected improvement
    let expected_under = ExpectedEffect {
        latency_delta_ms: Some(-80.0),
        throughput_delta_pct: None,
        error_rate_delta: None,
        queue_delta: Some(-500),
        description: "Expect 80ms reduction and 500 queue drain".to_string(),
    };
    let observed_under = ObservedEffect {
        latency_delta_ms: Some(-20.0), // only 25% of expected latency drop
        throughput_delta_pct: None,
        error_rate_delta: None,
        queue_delta: Some(-100), // only 20% of expected queue drop
        description: "Observed partial change".to_string(),
    };
    let measurement_under = EffectMeasurement::calculate(expected_under, observed_under);
    assert_eq!(measurement_under.status, EffectStatus::Failed); // <0.5 is Failed, triggers immediate replanning
    assert!(measurement_under.effectiveness < 0.5);

    // 3. Partially Met
    let expected_part = ExpectedEffect {
        latency_delta_ms: Some(-100.0),
        throughput_delta_pct: None,
        error_rate_delta: None,
        queue_delta: None,
        description: "Expect 100ms drop".to_string(),
    };
    let observed_part = ObservedEffect {
        latency_delta_ms: Some(-80.0), // 80% achieved
        throughput_delta_pct: None,
        error_rate_delta: None,
        queue_delta: None,
        description: "Observed 80ms drop".to_string(),
    };
    let measurement_part = EffectMeasurement::calculate(expected_part, observed_part);
    assert_eq!(measurement_part.status, EffectStatus::PartiallyMet);
    assert_eq!(measurement_part.effectiveness, 0.8);
}

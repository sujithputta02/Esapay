"""
Autonomous ARC-AGI-3 Fluid Reasoning Engine.
Performs end-to-end self-guided perception, hypothesis formulation, verification, and prediction.
"""

from dataclasses import dataclass
from typing import Any, Dict, List, Optional, Tuple
from arc_reasoner.dsl.ast_nodes import ASTNode
from arc_reasoner.exploration.sandbox import ExplorationSandbox
from arc_reasoner.exploration.verifier import HypothesisVerifier, VerificationResult
from arc_reasoner.priors.color import detect_background_color, get_color_histogram
from arc_reasoner.priors.grid import Grid, to_grid
from arc_reasoner.priors.objects import extract_objects
from arc_reasoner.synthesis.enumerator import ProgramSynthesizer
from arc_reasoner.synthesis.induction import analyze_demonstrations
from arc_reasoner.visualizer import Visualizer


@dataclass
class ReasoningOutcome:
    """Complete diagnostic trace of the agent's autonomous problem solving."""
    success: bool
    synthesized_program: Optional[ASTNode]
    program_code: str
    train_accuracy: float
    predicted_test_outputs: List[Grid]
    thought_trace: List[str]


class ARCReasoner:
    """
    Autonomous Fluid Intelligence Engine for ARC-AGI-3.
    Requires no text instructions or guidance. Given raw demonstration grids,
    it independently deduces the hidden rules and applies them to test inputs.
    """

    def __init__(self, verbose: bool = True, max_depth: int = 2):
        self.verbose = verbose
        self.max_depth = max_depth
        self.sandbox = ExplorationSandbox()
        self.synthesizer = ProgramSynthesizer(max_depth=max_depth)

    def _log(self, step: str, message: str, thought_trace: List[str]):
        thought_trace.append(f"{step}: {message}")
        if self.verbose:
            Visualizer.print_thought(step, message)

    def solve(
        self,
        train_pairs: List[Tuple[Grid, Grid]],
        test_inputs: List[Grid]
    ) -> ReasoningOutcome:
        """
        Autonomously solves an ARC task:
        1. Explores demonstration pairs to extract object/geometric priors.
        2. Induces structural/chromatic invariants.
        3. Synthesizes and tests candidate programs in the sandbox.
        4. Verifies candidate programs with 100% strict verification.
        5. Predicts solutions for unseen test grids.
        """
        thought_trace: List[str] = []
        self.sandbox.clear()

        self._log("INGEST", f"Received {len(train_pairs)} demonstration pairs and {len(test_inputs)} test input(s). Zero instructions provided.", thought_trace)

        # 1. Perceptual Analysis (Objectness & Color Priors)
        bg_colors = [detect_background_color(in_g) for in_g, _ in train_pairs]
        dom_bg = bg_colors[0] if bg_colors else 0
        total_objects = sum(len(extract_objects(in_g, background_color=dom_bg)) for in_g, _ in train_pairs)

        self._log(
            "PERCEPTION",
            f"Dominant background color: {dom_bg}. Detected {total_objects} cohesive objects across training demonstrations.",
            thought_trace
        )

        # 2. Inductive Invariant Analysis
        analysis = analyze_demonstrations(train_pairs)
        inv_desc = []
        if analysis.same_dimensions:
            inv_desc.append("Grid dimensions preserved (H_in == H_out, W_in == W_out)")
        elif analysis.is_scale:
            inv_desc.append(f"Grid scaled by factor {analysis.is_scale}x")
        elif analysis.is_cropped:
            inv_desc.append("Output is a tight crop around foreground objects")

        if analysis.colors_added:
            inv_desc.append(f"Colors introduced: {sorted(list(analysis.colors_added))}")
        if analysis.color_mapping:
            inv_desc.append(f"Potential color mapping: {analysis.color_mapping}")

        self._log("INDUCTION", "; ".join(inv_desc) if inv_desc else "Dynamic spatial transformation detected", thought_trace)

        # 3. Active Program Synthesis & Sandbox Hypothesis Testing
        self._log("SYNTHESIS", "Enumerating candidate hypotheses in DSL and testing in sandbox...", thought_trace)
        best_program = self.synthesizer.synthesize(train_pairs)

        if best_program is None:
            self._log("FAILURE", "Could not synthesize a program that satisfies all demonstrations.", thought_trace)
            return ReasoningOutcome(
                success=False,
                synthesized_program=None,
                program_code="None",
                train_accuracy=0.0,
                predicted_test_outputs=[],
                thought_trace=thought_trace
            )

        # 4. Strict Counterexample Verification
        v_result: VerificationResult = HypothesisVerifier.verify(best_program, train_pairs)
        self._log(
            "VERIFICATION",
            f"Selected hypothesis achieved {v_result.accuracy * 100:.1f}% accuracy across demonstrations. Code:\n  {best_program.to_code()}",
            thought_trace
        )

        if not v_result.passed:
            self._log("WARN", f"Program failed verification on example {v_result.counterexample_index}. Partial match.", thought_trace)

        # 5. Prediction on Test Grids
        predicted_outputs: List[Grid] = []
        for idx, t_in in enumerate(test_inputs):
            success, p_out, err = self.sandbox.test_program(best_program, t_in)
            if success and p_out is not None:
                predicted_outputs.append(p_out)
                self._log("PREDICTION", f"Computed output for Test Grid {idx + 1} ({len(p_out)}x{len(p_out[0])})", thought_trace)
            else:
                self._log("ERROR", f"Failed to execute program on Test Grid {idx + 1}: {err}", thought_trace)
                predicted_outputs.append(t_in)  # fallback to input

        return ReasoningOutcome(
            success=v_result.passed,
            synthesized_program=best_program,
            program_code=best_program.to_code(),
            train_accuracy=v_result.accuracy,
            predicted_test_outputs=predicted_outputs,
            thought_trace=thought_trace
        )


def run_demo():
    """Runs an interactive demonstration across the benchmark suite."""
    from arc_reasoner.benchmark import get_benchmark_tasks

    tasks = get_benchmark_tasks()
    reasoner = ARCReasoner(verbose=True)

    print(f"\n========================================================")
    print(f"   ARC-AGI-3 AUTONOMOUS FLUID REASONING ENGINE DEMO   ")
    print(f"========================================================\n")

    total_tasks = len(tasks)
    solved = 0

    for idx, task in enumerate(tasks):
        print(f"\n--------------------------------------------------------")
        print(f" TASK {idx + 1}/{total_tasks}: {task.name} [{task.category}]")
        print(f"--------------------------------------------------------")

        # Show first train pair
        print("\nDemonstration Example 1 (Hidden Rule to Discover):")
        print(Visualizer.render_side_by_side(task.train[0][0], task.train[0][1]))

        # Run Reasoner
        test_inputs = [t_in for t_in, _ in task.test]
        outcome = reasoner.solve(task.train, test_inputs)

        # Verify test output
        expected_test = task.test[0][1]
        predicted_test = outcome.predicted_test_outputs[0] if outcome.predicted_test_outputs else ()

        is_test_correct = (predicted_test == expected_test)
        if is_test_correct:
            solved += 1
            Visualizer.print_success(f"TEST PASSED! Autonomous prediction exactly matches ground truth.")
        else:
            Visualizer.print_failure("TEST FAILED: Prediction does not match ground truth.")

        print("\nTest Input vs Predicted Output:")
        print(Visualizer.render_side_by_side(task.test[0][0], predicted_test, title_left="Test Input", title_right="Predicted Output"))

    print(f"\n========================================================")
    print(f" BENCHMARK SUMMARY: {solved}/{total_tasks} Tasks Solved Autonomously ({solved/total_tasks*100:.1f}%)")
    print(f"========================================================\n")


if __name__ == "__main__":
    run_demo()

"""
Verification and counterexample testing for candidate hypotheses.
"""

from dataclasses import dataclass
from typing import List, Optional, Tuple
from arc_reasoner.dsl.ast_nodes import ASTNode
from arc_reasoner.priors.grid import Grid


@dataclass
class VerificationResult:
    """Outcome of validating a candidate program against demonstrations."""
    passed: bool
    accuracy: float
    counterexample_index: Optional[int]
    expected: Optional[Grid]
    predicted: Optional[Grid]
    error_msg: Optional[str] = None


class HypothesisVerifier:
    """Tests hypotheses across all available demonstration examples."""

    @staticmethod
    def verify(program: ASTNode, examples: List[Tuple[Grid, Grid]]) -> VerificationResult:
        total = len(examples)
        if total == 0:
            return VerificationResult(passed=False, accuracy=0.0, counterexample_index=None, expected=None, predicted=None, error_msg="No examples provided")

        correct = 0
        first_counterexample = None
        expected_grid = None
        predicted_grid = None

        for idx, (in_g, out_g) in enumerate(examples):
            try:
                pred = program.evaluate(in_g)
                if pred == out_g:
                    correct += 1
                else:
                    if first_counterexample is None:
                        first_counterexample = idx
                        expected_grid = out_g
                        predicted_grid = pred
            except Exception as e:
                if first_counterexample is None:
                    first_counterexample = idx
                    expected_grid = out_g
                    predicted_grid = None
                    return VerificationResult(
                        passed=False,
                        accuracy=correct / total,
                        counterexample_index=idx,
                        expected=out_g,
                        predicted=None,
                        error_msg=f"Execution error on example {idx}: {e}"
                    )

        passed = (correct == total)
        return VerificationResult(
            passed=passed,
            accuracy=correct / total,
            counterexample_index=first_counterexample,
            expected=expected_grid,
            predicted=predicted_grid,
        )

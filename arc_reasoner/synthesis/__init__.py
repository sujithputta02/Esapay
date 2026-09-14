"""
Synthesis module exposing ProgramSynthesizer, cost functions, and induction analysis.
"""

from arc_reasoner.synthesis.cost import compute_grid_mismatch_loss, compute_program_score
from arc_reasoner.synthesis.enumerator import ProgramSynthesizer
from arc_reasoner.synthesis.induction import TransformationAnalysis, analyze_demonstrations

__all__ = [
    "ProgramSynthesizer",
    "compute_grid_mismatch_loss",
    "compute_program_score",
    "analyze_demonstrations",
    "TransformationAnalysis",
]

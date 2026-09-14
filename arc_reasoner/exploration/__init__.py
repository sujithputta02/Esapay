"""
Exploration package exposing sandbox, verifier, and curiosity metrics.
"""

from arc_reasoner.exploration.curiosity import compute_entropy_reduction, compute_state_novelty
from arc_reasoner.exploration.sandbox import ExplorationSandbox, SandboxTrial
from arc_reasoner.exploration.verifier import HypothesisVerifier, VerificationResult

__all__ = [
    "ExplorationSandbox",
    "SandboxTrial",
    "HypothesisVerifier",
    "VerificationResult",
    "compute_state_novelty",
    "compute_entropy_reduction",
]

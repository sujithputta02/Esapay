"""
Curiosity-driven exploration and novelty metrics for ARC-AGI-3 environments.
"""

from typing import Dict, List, Set, Tuple
from arc_reasoner.priors.grid import Grid


def compute_state_novelty(visited_states: Set[Grid], current_state: Grid) -> float:
    """
    Computes an intrinsic curiosity bonus based on whether current_state has been seen before.
    Returns 1.0 for completely novel states, decays for frequently seen states.
    """
    if current_state not in visited_states:
        return 1.0
    return 0.0


def compute_entropy_reduction(
    prior_candidates_count: int,
    posterior_candidates_count: int
) -> float:
    """
    Measures information gain from testing an action in the sandbox.
    High reduction in valid candidates means the test was highly informative.
    """
    if prior_candidates_count <= 0:
        return 0.0
    if posterior_candidates_count <= 0:
        return 1.0
    import math
    prior_entropy = math.log2(prior_candidates_count)
    posterior_entropy = math.log2(posterior_candidates_count)
    return max(0.0, prior_entropy - posterior_entropy)

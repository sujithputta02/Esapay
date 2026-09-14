"""
Complexity and cost metrics based on Occam's razor and Minimum Description Length (MDL).
"""

from typing import List, Tuple
from arc_reasoner.dsl.ast_nodes import ASTNode
from arc_reasoner.priors.grid import Grid


def compute_grid_mismatch_loss(pred: Grid, target: Grid) -> float:
    """
    Computes cell-by-cell error between predicted grid and target grid.
    Returns 0.0 for an exact match, or normalized mismatch penalty.
    """
    if not pred or not target:
        return 1.0
    ph, pw = len(pred), len(pred[0])
    th, tw = len(target), len(target[0])
    if ph != th or pw != tw:
        # Extreme penalty for mismatched dimensions
        return 1.0 + abs(ph - th) + abs(pw - tw)

    mismatches = 0
    total = ph * pw
    for r in range(ph):
        for c in range(pw):
            if pred[r][c] != target[r][c]:
                mismatches += 1

    return mismatches / total


def compute_program_score(program: ASTNode, examples: List[Tuple[Grid, Grid]]) -> Tuple[float, bool]:
    """
    Evaluates a program across all demonstration examples.
    Returns (total_score, is_exact_match).
    Lower score is better.
    """
    total_loss = 0.0
    for in_grid, out_grid in examples:
        try:
            pred = program.evaluate(in_grid)
            loss = compute_grid_mismatch_loss(pred, out_grid)
            total_loss += loss
        except Exception:
            total_loss += 10.0

    is_exact = (total_loss == 0.0)
    # MDL score = total training loss * 1000 + structural complexity
    mdl_score = (total_loss * 1000.0) + program.complexity()
    return mdl_score, is_exact

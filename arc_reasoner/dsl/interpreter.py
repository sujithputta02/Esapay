"""
Safe deterministic interpreter for running synthesized programs.
"""

from typing import Any, Optional, Tuple
from arc_reasoner.dsl.ast_nodes import ASTNode
from arc_reasoner.priors.grid import Grid


class ProgramExecutionError(Exception):
    """Raised when program execution produces invalid output or errors."""
    pass


def execute_program(program: ASTNode, grid: Grid) -> Tuple[bool, Optional[Grid], Optional[str]]:
    """
    Safely executes an AST program on a grid.
    
    Returns:
        (success, result_grid, error_message)
    """
    try:
        res = program.evaluate(grid)
        if not isinstance(res, tuple) or not res or not isinstance(res[0], tuple):
            return False, None, "Program returned non-grid object"
        return True, res, None
    except Exception as e:
        return False, None, str(e)

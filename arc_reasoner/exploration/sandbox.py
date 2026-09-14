"""
Interactive sandbox scratchpad for active exploration of ARC environment rules.
"""

from dataclasses import dataclass, field
from typing import Callable, List, Optional, Tuple
from arc_reasoner.dsl.ast_nodes import ASTNode
from arc_reasoner.dsl.interpreter import execute_program
from arc_reasoner.priors.grid import Grid


@dataclass
class SandboxTrial:
    """Record of a trial action or hypothesis tested in the sandbox."""
    trial_id: int
    program_code: str
    input_grid: Grid
    output_grid: Optional[Grid]
    success: bool
    error: Optional[str] = None


class ExplorationSandbox:
    """
    Scratchpad environment where the model tests actions and hypotheses
    before committing to a final solution.
    """

    def __init__(self):
        self.history: List[SandboxTrial] = []
        self._counter = 0

    def test_program(self, program: ASTNode, test_input: Grid) -> Tuple[bool, Optional[Grid], Optional[str]]:
        """Executes a candidate program inside the sandbox and logs the result."""
        self._counter += 1
        success, res_grid, err = execute_program(program, test_input)
        trial = SandboxTrial(
            trial_id=self._counter,
            program_code=program.to_code(),
            input_grid=test_input,
            output_grid=res_grid,
            success=success,
            error=err
        )
        self.history.append(trial)
        return success, res_grid, err

    def clear(self):
        self.history.clear()
        self._counter = 0

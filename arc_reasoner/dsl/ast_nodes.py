"""
Abstract Syntax Tree (AST) representations for synthesized ARC-AGI programs.
"""

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Tuple
from arc_reasoner.priors.grid import Grid


class ASTNode(ABC):
    """Base class for all synthesized program AST nodes."""

    @abstractmethod
    def evaluate(self, grid: Grid) -> Grid:
        """Executes the AST node on an input grid."""
        pass

    @abstractmethod
    def to_code(self) -> str:
        """Returns readable Python-like code representing this program."""
        pass

    @abstractmethod
    def complexity(self) -> int:
        """Computes structural Kolmogorov complexity / description length."""
        pass


@dataclass
class PrimitiveNode(ASTNode):
    """Executes a single primitive function."""
    name: str
    func: Callable[..., Grid]
    args: Tuple[Any, ...] = field(default_factory=tuple)
    kwargs: Dict[str, Any] = field(default_factory=dict)

    def evaluate(self, grid: Grid) -> Grid:
        return self.func(grid, *self.args, **self.kwargs)

    def to_code(self) -> str:
        all_args = [repr(a) for a in self.args]
        all_args.extend(f"{k}={repr(v)}" for k, v in self.kwargs.items())
        args_str = ", ".join(all_args)
        if args_str:
            return f"{self.name}(grid, {args_str})"
        return f"{self.name}(grid)"

    def complexity(self) -> int:
        # Base cost of primitive + cost of arguments
        return 1 + len(self.args) + len(self.kwargs)


@dataclass
class PipelineNode(ASTNode):
    """Chains multiple AST transformations in sequence: step_k(...(step_1(grid)))."""
    steps: List[ASTNode] = field(default_factory=list)

    def evaluate(self, grid: Grid) -> Grid:
        curr = grid
        for step in self.steps:
            curr = step.evaluate(curr)
        return curr

    def to_code(self) -> str:
        if not self.steps:
            return "identity(grid)"
        if len(self.steps) == 1:
            return self.steps[0].to_code()
        lines = ["# Pipeline of transformations:"]
        for i, step in enumerate(self.steps):
            lines.append(f"grid = {step.to_code()}")
        return "\n".join(lines)

    def complexity(self) -> int:
        return sum(s.complexity() for s in self.steps)


@dataclass
class IdentityNode(ASTNode):
    """Returns grid unchanged."""
    def evaluate(self, grid: Grid) -> Grid:
        return grid

    def to_code(self) -> str:
        return "grid"

    def complexity(self) -> int:
        return 0

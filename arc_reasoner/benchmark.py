"""
ARC-AGI Benchmark Suite: Collection of diverse fluid reasoning tasks
testing geometry, topology, gravity, symmetry, and color logic.
"""

from dataclasses import dataclass
from typing import Dict, List, Tuple
from arc_reasoner.priors.grid import Grid, to_grid


@dataclass
class ARCTask:
    """Represents a standard ARC task containing demonstration pairs and test cases."""
    task_id: str
    name: str
    category: str
    train: List[Tuple[Grid, Grid]]  # (input, output) pairs
    test: List[Tuple[Grid, Grid]]   # (input, expected_output) pairs


def get_benchmark_tasks() -> List[ARCTask]:
    """Returns a curated suite of diverse ARC-AGI reasoning tasks."""
    tasks: List[ARCTask] = []

    # Task 1: Topological Hole Filling
    # Rule: Background (0) completely enclosed by blue (1) walls must be filled with yellow (4).
    train_1 = [
        (
            to_grid([
                [0, 0, 0, 0, 0],
                [0, 1, 1, 1, 0],
                [0, 1, 0, 1, 0],
                [0, 1, 1, 1, 0],
                [0, 0, 0, 0, 0],
            ]),
            to_grid([
                [0, 0, 0, 0, 0],
                [0, 1, 1, 1, 0],
                [0, 1, 4, 1, 0],
                [0, 1, 1, 1, 0],
                [0, 0, 0, 0, 0],
            ]),
        ),
        (
            to_grid([
                [0, 0, 0, 0, 0, 0],
                [0, 1, 1, 1, 1, 0],
                [0, 1, 0, 0, 1, 0],
                [0, 1, 1, 1, 1, 0],
                [0, 0, 0, 0, 0, 0],
            ]),
            to_grid([
                [0, 0, 0, 0, 0, 0],
                [0, 1, 1, 1, 1, 0],
                [0, 1, 4, 4, 1, 0],
                [0, 1, 1, 1, 1, 0],
                [0, 0, 0, 0, 0, 0],
            ]),
        ),
    ]
    test_1 = [
        (
            to_grid([
                [0, 0, 0, 0, 0, 0, 0],
                [0, 1, 1, 1, 1, 1, 0],
                [0, 1, 0, 0, 0, 1, 0],
                [0, 1, 1, 1, 1, 1, 0],
                [0, 0, 0, 0, 0, 0, 0],
            ]),
            to_grid([
                [0, 0, 0, 0, 0, 0, 0],
                [0, 1, 1, 1, 1, 1, 0],
                [0, 1, 4, 4, 4, 1, 0],
                [0, 1, 1, 1, 1, 1, 0],
                [0, 0, 0, 0, 0, 0, 0],
            ]),
        )
    ]
    tasks.append(ARCTask("arc_01", "Topological Hole Fill", "Topology", train_1, test_1))

    # Task 2: Downward Gravity
    # Rule: All non-background blocks fall to the bottom of the grid along their column.
    train_2 = [
        (
            to_grid([
                [0, 2, 0, 0],
                [0, 0, 3, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
            ]),
            to_grid([
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 2, 3, 0],
            ]),
        ),
        (
            to_grid([
                [1, 0, 0, 4],
                [2, 0, 0, 0],
                [0, 0, 0, 0],
            ]),
            to_grid([
                [0, 0, 0, 0],
                [1, 0, 0, 0],
                [2, 0, 0, 4],
            ]),
        ),
    ]
    test_2 = [
        (
            to_grid([
                [0, 7, 0, 0, 8],
                [0, 0, 0, 0, 0],
                [0, 6, 0, 0, 0],
                [0, 0, 0, 0, 0],
            ]),
            to_grid([
                [0, 0, 0, 0, 0],
                [0, 0, 0, 0, 0],
                [0, 7, 0, 0, 0],
                [0, 6, 0, 0, 8],
            ]),
        )
    ]
    tasks.append(ARCTask("arc_02", "Downward Gravity Drop", "Physics/Mechanics", train_2, test_2))

    # Task 3: Geometric 90-Degree Clockwise Rotation
    train_3 = [
        (
            to_grid([
                [1, 2],
                [3, 0],
            ]),
            to_grid([
                [3, 1],
                [0, 2],
            ]),
        ),
        (
            to_grid([
                [0, 4, 5],
                [0, 0, 6],
            ]),
            to_grid([
                [0, 0],
                [0, 4],
                [6, 5],
            ]),
        ),
    ]
    test_3 = [
        (
            to_grid([
                [7, 8, 9],
                [1, 0, 2],
            ]),
            to_grid([
                [1, 7],
                [0, 8],
                [2, 9],
            ]),
        )
    ]
    tasks.append(ARCTask("arc_03", "Clockwise Rotation", "Geometry", train_3, test_3))

    # Task 4: Content Bounding Box Extraction / Cropping
    train_4 = [
        (
            to_grid([
                [0, 0, 0, 0, 0],
                [0, 3, 3, 0, 0],
                [0, 3, 3, 0, 0],
                [0, 0, 0, 0, 0],
            ]),
            to_grid([
                [3, 3],
                [3, 3],
            ]),
        ),
        (
            to_grid([
                [0, 0, 0, 0],
                [0, 2, 2, 2],
                [0, 0, 0, 0],
            ]),
            to_grid([
                [2, 2, 2],
            ]),
        ),
    ]
    test_4 = [
        (
            to_grid([
                [0, 0, 0, 0, 0, 0],
                [0, 0, 8, 8, 0, 0],
                [0, 0, 8, 8, 0, 0],
                [0, 0, 8, 8, 0, 0],
                [0, 0, 0, 0, 0, 0],
            ]),
            to_grid([
                [8, 8],
                [8, 8],
                [8, 8],
            ]),
        )
    ]
    tasks.append(ARCTask("arc_04", "Crop to Content", "BoundingBox", train_4, test_4))

    # Task 5: Vertical Reflective Symmetry Completion
    train_5 = [
        (
            to_grid([
                [1, 0, 0, 0],
                [2, 3, 0, 0],
                [0, 4, 0, 0],
            ]),
            to_grid([
                [1, 0, 0, 1],
                [2, 3, 3, 2],
                [0, 4, 4, 0],
            ]),
        ),
        (
            to_grid([
                [0, 6, 0, 0],
                [7, 0, 0, 0],
            ]),
            to_grid([
                [0, 6, 6, 0],
                [7, 0, 0, 7],
            ]),
        ),
    ]
    test_5 = [
        (
            to_grid([
                [8, 9, 0, 0],
                [0, 5, 0, 0],
                [3, 0, 0, 0],
            ]),
            to_grid([
                [8, 9, 9, 8],
                [0, 5, 5, 0],
                [3, 0, 0, 3],
            ]),
        )
    ]
    tasks.append(ARCTask("arc_05", "Vertical Symmetry Completion", "Symmetry", train_5, test_5))

    # Task 6: Color Permutation / Recoloring
    train_6 = [
        (
            to_grid([
                [1, 1, 0],
                [0, 1, 0],
            ]),
            to_grid([
                [2, 2, 0],
                [0, 2, 0],
            ]),
        ),
        (
            to_grid([
                [0, 1, 1],
                [1, 0, 1],
            ]),
            to_grid([
                [0, 2, 2],
                [2, 0, 2],
            ]),
        ),
    ]
    test_6 = [
        (
            to_grid([
                [1, 0, 1, 1],
                [0, 1, 0, 0],
            ]),
            to_grid([
                [2, 0, 2, 2],
                [0, 2, 0, 0],
            ]),
        )
    ]
    tasks.append(ARCTask("arc_06", "Color Transformation", "Chromatic", train_6, test_6))

    return tasks

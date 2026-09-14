"""
Inductive bias analysis: inspects demonstration pairs to infer transformations.
"""

from dataclasses import dataclass
from typing import Dict, List, Optional, Set, Tuple
from arc_reasoner.priors.color import (
    detect_background_color,
    get_color_histogram,
    get_foreground_colors,
)
from arc_reasoner.priors.grid import Grid, crop, get_bounding_box
from arc_reasoner.priors.objects import extract_objects
from arc_reasoner.priors.topology import find_enclosed_holes


@dataclass
class TransformationAnalysis:
    """Summary of spatial, topological, and chromatic deltas across demonstration pairs."""
    same_dimensions: bool
    is_scale: Optional[int]
    is_tiled: Optional[Tuple[int, int]]
    is_cropped: bool
    colors_added: Set[int]
    colors_removed: Set[int]
    color_mapping: Dict[int, int]
    holes_filled_possible: bool
    gravity_possible: bool
    symmetry_possible: bool


def analyze_demonstrations(examples: List[Tuple[Grid, Grid]]) -> TransformationAnalysis:
    """
    Induces structural invariants across all input/output pairs.
    """
    same_dims = True
    scale_factor: Optional[int] = None
    all_colors_added: Set[int] = set()
    all_colors_removed: Set[int] = set()
    inferred_color_map: Dict[int, int] = {}
    is_cropped = True
    holes_filled_possible = True
    gravity_possible = True
    symmetry_possible = True

    for in_grid, out_grid in examples:
        ih, iw = len(in_grid), len(in_grid[0])
        oh, ow = len(out_grid), len(out_grid[0])

        if (ih, iw) != (oh, ow):
            same_dims = False

        # Scale check
        if oh % ih == 0 and ow % iw == 0 and (oh // ih == ow // iw):
            factor = oh // ih
            if scale_factor is None or scale_factor == factor:
                scale_factor = factor
            else:
                scale_factor = None
        else:
            scale_factor = None

        # Crop check
        bg = detect_background_color(in_grid)
        bbox = get_bounding_box(in_grid, background_color=bg)
        if bbox:
            min_r, min_c, max_r, max_c = bbox
            cropped_h = max_r - min_r + 1
            cropped_w = max_c - min_c + 1
            if (cropped_h, cropped_w) != (oh, ow):
                is_cropped = False
        else:
            is_cropped = False

        # Color analysis
        in_hist = get_color_histogram(in_grid)
        out_hist = get_color_histogram(out_grid)
        in_colors = set(in_hist.keys())
        out_colors = set(out_hist.keys())

        all_colors_added.update(out_colors - in_colors)
        all_colors_removed.update(in_colors - out_colors)

        # Direct 1-to-1 color replacement candidate check
        if same_dims and (out_colors - in_colors):
            new_col = list(out_colors - in_colors)[0]
            # See which input color was replaced
            for r in range(ih):
                for c in range(iw):
                    if in_grid[r][c] != out_grid[r][c] and out_grid[r][c] == new_col:
                        inferred_color_map[in_grid[r][c]] = new_col

        # Check hole filling possibility
        holes = find_enclosed_holes(in_grid, bg_color=bg)
        if not holes and (out_colors - in_colors):
            holes_filled_possible = False

    return TransformationAnalysis(
        same_dimensions=same_dims,
        is_scale=scale_factor if not same_dims else None,
        is_tiled=None,
        is_cropped=is_cropped if not same_dims else False,
        colors_added=all_colors_added,
        colors_removed=all_colors_removed,
        color_mapping=inferred_color_map,
        holes_filled_possible=holes_filled_possible,
        gravity_possible=same_dims,
        symmetry_possible=same_dims,
    )

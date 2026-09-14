"""
Domain-Specific Language (DSL) primitive transformations for ARC-AGI.
"""

from typing import Dict, List, Optional, Tuple
from arc_reasoner.priors.color import detect_background_color, get_foreground_colors
from arc_reasoner.priors.grid import (
    Grid,
    crop,
    flip_h,
    flip_v,
    get_bounding_box,
    rotate90,
    rotate180,
    rotate270,
    scale,
    tile,
    to_grid,
    to_mutable,
    transpose,
)
from arc_reasoner.priors.objects import Object, extract_objects
from arc_reasoner.priors.topology import fill_enclosed_holes


def recolor(grid: Grid, old_color: int, new_color: int) -> Grid:
    """Replaces all pixels of old_color with new_color."""
    return tuple(
        tuple(new_color if val == old_color else val for val in row)
        for row in grid
    )


def recolor_all_foreground(grid: Grid, new_color: int, bg_color: Optional[int] = None) -> Grid:
    """Changes every non-background pixel to new_color."""
    if bg_color is None:
        bg_color = detect_background_color(grid)
    return tuple(
        tuple(new_color if val != bg_color else val for val in row)
        for row in grid
    )


def crop_to_content(grid: Grid, bg_color: Optional[int] = None) -> Grid:
    """Crops the grid tightly around all non-background content."""
    if bg_color is None:
        bg_color = detect_background_color(grid)
    bbox = get_bounding_box(grid, background_color=bg_color)
    if bbox is None:
        return grid
    min_r, min_c, max_r, max_c = bbox
    return crop(grid, min_r, min_c, max_r, max_c)


def rotate_cw(grid: Grid) -> Grid:
    return rotate90(grid)


def rotate_ccw(grid: Grid) -> Grid:
    return rotate270(grid)


def rotate_half(grid: Grid) -> Grid:
    return rotate180(grid)


def reflect_h(grid: Grid) -> Grid:
    return flip_h(grid)


def reflect_v(grid: Grid) -> Grid:
    return flip_v(grid)


def fill_holes(grid: Grid, fill_color: int, bg_color: Optional[int] = None) -> Grid:
    """Fills enclosed hollows with fill_color."""
    if bg_color is None:
        bg_color = detect_background_color(grid)
    return fill_enclosed_holes(grid, fill_color=fill_color, bg_color=bg_color)


def apply_gravity(grid: Grid, direction: str = "down", bg_color: Optional[int] = None) -> Grid:
    """
    Applies 1D gravity to each column (or row), moving foreground blocks towards the boundary.
    """
    if bg_color is None:
        bg_color = detect_background_color(grid)

    h, w = len(grid), len(grid[0])
    res = to_mutable(grid)

    if direction == "down":
        for c in range(w):
            col_vals = [grid[r][c] for r in range(h) if grid[r][c] != bg_color]
            num_empty = h - len(col_vals)
            for r in range(num_empty):
                res[r][c] = bg_color
            for i, val in enumerate(col_vals):
                res[num_empty + i][c] = val
    elif direction == "up":
        for c in range(w):
            col_vals = [grid[r][c] for r in range(h) if grid[r][c] != bg_color]
            for i, val in enumerate(col_vals):
                res[i][c] = val
            for r in range(len(col_vals), h):
                res[r][c] = bg_color
    elif direction == "right":
        for r in range(h):
            row_vals = [grid[r][c] for c in range(w) if grid[r][c] != bg_color]
            num_empty = w - len(row_vals)
            for c in range(num_empty):
                res[r][c] = bg_color
            for i, val in enumerate(row_vals):
                res[r][num_empty + i] = val
    elif direction == "left":
        for r in range(h):
            row_vals = [grid[r][c] for c in range(w) if grid[r][c] != bg_color]
            for i, val in enumerate(row_vals):
                res[r][i] = val
            for c in range(len(row_vals), w):
                res[r][c] = bg_color

    return to_grid(res)


def extract_largest_object_grid(grid: Grid, bg_color: Optional[int] = None) -> Grid:
    """Extracts the largest connected object and returns it cropped."""
    if bg_color is None:
        bg_color = detect_background_color(grid)
    objs = extract_objects(grid, background_color=bg_color)
    if not objs:
        return grid
    largest = max(objs, key=lambda o: o.area)
    return largest.to_subgrid(background=bg_color)


def extract_smallest_object_grid(grid: Grid, bg_color: Optional[int] = None) -> Grid:
    """Extracts the smallest connected object and returns it cropped."""
    if bg_color is None:
        bg_color = detect_background_color(grid)
    objs = extract_objects(grid, background_color=bg_color)
    if not objs:
        return grid
    smallest = min(objs, key=lambda o: o.area)
    return smallest.to_subgrid(background=bg_color)


def filter_keep_color(grid: Grid, target_color: int, bg_color: Optional[int] = None) -> Grid:
    """Zeros out all pixels that are not target_color."""
    if bg_color is None:
        bg_color = detect_background_color(grid)
    return tuple(
        tuple(val if val == target_color else bg_color for val in row)
        for row in grid
    )


def complete_symmetry(grid: Grid, axis: str = "vertical") -> Grid:
    """
    Fills in missing pixels by assuming reflective symmetry.
    axis='vertical' reflects left half onto right half (or union).
    axis='horizontal' reflects top half onto bottom half (or union).
    """
    h, w = len(grid), len(grid[0])
    res = to_mutable(grid)

    if axis == "vertical":
        for r in range(h):
            for c in range(w // 2):
                sym_c = w - 1 - c
                v1, v2 = grid[r][c], grid[r][sym_c]
                if v1 != 0 and v2 == 0:
                    res[r][sym_c] = v1
                elif v2 != 0 and v1 == 0:
                    res[r][c] = v2
    elif axis == "horizontal":
        for r in range(h // 2):
            sym_r = h - 1 - r
            for c in range(w):
                v1, v2 = grid[r][c], grid[sym_r][c]
                if v1 != 0 and v2 == 0:
                    res[sym_r][c] = v1
                elif v2 != 0 and v1 == 0:
                    res[r][c] = v2

    return to_grid(res)

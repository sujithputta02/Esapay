"""
Topological priors: enclosed regions, holes, containment, perimeter, and boundaries.
"""

from typing import List, Optional, Set, Tuple
from arc_reasoner.priors.grid import Grid, to_grid, to_mutable


def find_enclosed_holes(
    grid: Grid,
    bg_color: int = 0
) -> Set[Tuple[int, int]]:
    """
    Finds background pixels that are completely enclosed by foreground objects (holes).
    Uses flood fill starting from all outer boundary background cells.
    Any background cell NOT reached by this exterior flood fill is enclosed.
    """
    h, w = len(grid), len(grid[0])
    exterior: Set[Tuple[int, int]] = set()
    queue: List[Tuple[int, int]] = []

    # Initialize queue with all boundary cells that have bg_color
    for r in range(h):
        for c in (0, w - 1):
            if grid[r][c] == bg_color and (r, c) not in exterior:
                exterior.add((r, c))
                queue.append((r, c))
    for c in range(w):
        for r in (0, h - 1):
            if grid[r][c] == bg_color and (r, c) not in exterior:
                exterior.add((r, c))
                queue.append((r, c))

    # 4-way flood fill from exterior
    neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1)]
    while queue:
        curr_r, curr_c = queue.pop(0)
        for dr, dc in neighbors:
            nr, nc = curr_r + dr, curr_c + dc
            if 0 <= nr < h and 0 <= nc < w:
                if grid[nr][nc] == bg_color and (nr, nc) not in exterior:
                    exterior.add((nr, nc))
                    queue.append((nr, nc))

    # All background cells not reached from exterior are enclosed holes
    enclosed: Set[Tuple[int, int]] = set()
    for r in range(h):
        for c in range(w):
            if grid[r][c] == bg_color and (r, c) not in exterior:
                enclosed.add((r, c))

    return enclosed


def fill_enclosed_holes(
    grid: Grid,
    fill_color: int,
    bg_color: int = 0
) -> Grid:
    """Fills all enclosed background regions (holes) with fill_color."""
    holes = find_enclosed_holes(grid, bg_color=bg_color)
    if not holes:
        return grid
    res = to_mutable(grid)
    for r, c in holes:
        res[r][c] = fill_color
    return to_grid(res)


def extract_perimeter(grid: Grid, bg_color: int = 0) -> Set[Tuple[int, int]]:
    """Returns coordinates of foreground pixels that touch the background or grid border."""
    h, w = len(grid), len(grid[0])
    perimeter: Set[Tuple[int, int]] = set()
    neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1)]

    for r in range(h):
        for c in range(w):
            if grid[r][c] == bg_color:
                continue
            is_edge = False
            # Check grid edge
            if r == 0 or r == h - 1 or c == 0 or c == w - 1:
                is_edge = True
            else:
                for dr, dc in neighbors:
                    nr, nc = r + dr, c + dc
                    if grid[nr][nc] == bg_color:
                        is_edge = True
                        break
            if is_edge:
                perimeter.add((r, c))

    return perimeter

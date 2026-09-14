"""
Grid representations, geometric transformations, and coordinate manipulations.
"""

from typing import Any, List, Optional, Tuple, Union

Grid = Tuple[Tuple[int, ...], ...]
MutableGrid = List[List[int]]


def to_grid(data: Any) -> Grid:
    """Converts a 2D iterable into an immutable Grid (tuple of tuples)."""
    return tuple(tuple(int(x) for x in row) for row in data)


def to_mutable(grid: Grid) -> MutableGrid:
    """Converts an immutable Grid into a mutable 2D list."""
    return [list(row) for row in grid]


def create_empty_grid(h: int, w: int, fill: int = 0) -> Grid:
    """Creates a grid of height h, width w filled with fill value."""
    return tuple(tuple(fill for _ in range(w)) for _ in range(h))


def rotate90(grid: Grid) -> Grid:
    """Rotates grid 90 degrees clockwise."""
    h, w = len(grid), len(grid[0])
    return tuple(tuple(grid[h - 1 - r][c] for r in range(h)) for c in range(w))


def rotate180(grid: Grid) -> Grid:
    """Rotates grid 180 degrees."""
    return tuple(tuple(reversed(row)) for row in reversed(grid))


def rotate270(grid: Grid) -> Grid:
    """Rotates grid 270 degrees clockwise (90 counter-clockwise)."""
    h, w = len(grid), len(grid[0])
    return tuple(tuple(grid[r][w - 1 - c] for r in range(h)) for c in range(w))


def flip_v(grid: Grid) -> Grid:
    """Reflects grid vertically (upside down)."""
    return tuple(reversed(grid))


def flip_h(grid: Grid) -> Grid:
    """Reflects grid horizontally (left-to-right mirror)."""
    return tuple(tuple(reversed(row)) for row in grid)


def transpose(grid: Grid) -> Grid:
    """Transposes rows and columns (reflection across main diagonal)."""
    h, w = len(grid), len(grid[0])
    return tuple(tuple(grid[r][c] for r in range(h)) for c in range(w))


def crop(grid: Grid, r1: int, c1: int, r2: int, c2: int) -> Grid:
    """Crops grid to bounding box [r1:r2+1, c1:c2+1] inclusive."""
    h, w = len(grid), len(grid[0])
    r1 = max(0, min(r1, h - 1))
    r2 = max(0, min(r2, h - 1))
    c1 = max(0, min(c1, w - 1))
    c2 = max(0, min(c2, w - 1))
    if r1 > r2 or c1 > c2:
        return ()
    return tuple(tuple(grid[r][c] for c in range(c1, c2 + 1)) for r in range(r1, r2 + 1))


def pad(grid: Grid, top: int, bottom: int, left: int, right: int, color: int = 0) -> Grid:
    """Pads grid with color border."""
    h, w = len(grid), len(grid[0])
    new_h = h + top + bottom
    new_w = w + left + right
    res = [[color] * new_w for _ in range(new_h)]
    for r in range(h):
        for c in range(w):
            res[r + top][c + left] = grid[r][c]
    return to_grid(res)


def scale(grid: Grid, factor: int) -> Grid:
    """Kronecker scale: magnifies each pixel into a factor x factor block."""
    if factor <= 1:
        return grid
    h, w = len(grid), len(grid[0])
    res = []
    for r in range(h):
        scaled_row = []
        for c in range(w):
            scaled_row.extend([grid[r][c]] * factor)
        for _ in range(factor):
            res.append(tuple(scaled_row))
    return tuple(res)


def tile(grid: Grid, repeat_r: int, repeat_c: int) -> Grid:
    """Repeats the grid repeat_r times vertically and repeat_c horizontally."""
    if repeat_r <= 0 or repeat_c <= 0:
        return ()
    tiled_rows = []
    for row in grid:
        tiled_rows.append(row * repeat_c)
    return tuple(tiled_rows * repeat_r)


def overlay(base: Grid, top: Grid, offset_r: int = 0, offset_c: int = 0, transparent_color: Optional[int] = 0) -> Grid:
    """Overlays top grid onto base grid at offset, skipping transparent_color pixels."""
    res = to_mutable(base)
    bh, bw = len(base), len(base[0])
    th, tw = len(top), len(top[0])
    
    for tr in range(th):
        br = tr + offset_r
        if 0 <= br < bh:
            for tc in range(tw):
                bc = tc + offset_c
                if 0 <= bc < bw:
                    val = top[tr][tc]
                    if transparent_color is None or val != transparent_color:
                        res[br][bc] = val
    return to_grid(res)


def get_bounding_box(grid: Grid, background_color: int = 0) -> Optional[Tuple[int, int, int, int]]:
    """Returns (min_r, min_c, max_r, max_c) for all non-background pixels."""
    min_r, min_c = float('inf'), float('inf')
    max_r, max_c = float('-inf'), float('-inf')
    found = False
    
    for r, row in enumerate(grid):
        for c, val in enumerate(row):
            if val != background_color:
                found = True
                if r < min_r: min_r = r
                if r > max_r: max_r = r
                if c < min_c: min_c = c
                if c > max_c: max_c = c
                
    if not found:
        return None
    return int(min_r), int(min_c), int(max_r), int(max_c)

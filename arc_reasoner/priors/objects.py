"""
Objectness priors: connected component extraction, bounding boxes, cohesion, and filtering.
"""

from dataclasses import dataclass
from typing import Dict, List, Optional, Set, Tuple
from arc_reasoner.priors.grid import Grid, to_grid, to_mutable


@dataclass(frozen=True)
class Object:
    """Represents a coherent visual object in an ARC grid."""
    pixels: Tuple[Tuple[int, int], ...]  # Tuple of (row, col)
    colors: Tuple[int, ...]              # Colors of the pixels
    color: int                           # Dominant color
    min_r: int
    min_c: int
    max_r: int
    max_c: int

    @property
    def height(self) -> int:
        return self.max_r - self.min_r + 1

    @property
    def width(self) -> int:
        return self.max_c - self.min_c + 1

    @property
    def area(self) -> int:
        return len(self.pixels)

    @property
    def centroid(self) -> Tuple[float, float]:
        if not self.pixels:
            return (0.0, 0.0)
        avg_r = sum(r for r, _ in self.pixels) / len(self.pixels)
        avg_c = sum(c for _, c in self.pixels) / len(self.pixels)
        return (avg_r, avg_c)

    def to_subgrid(self, background: int = 0) -> Grid:
        """Renders the object in its own cropped subgrid."""
        h, w = self.height, self.width
        matrix = [[background] * w for _ in range(h)]
        for (r, c), col in zip(self.pixels, self.colors):
            matrix[r - self.min_r][c - self.min_c] = col
        return to_grid(matrix)


def extract_objects(
    grid: Grid,
    background_color: int = 0,
    connectivity: int = 4,
    monochrome: bool = True
) -> List[Object]:
    """
    Extracts coherent objects from a grid using connected components.
    
    Args:
        grid: Input 2D grid
        background_color: Color considered background / empty
        connectivity: 4 or 8 neighbor connectivity
        monochrome: If True, objects only group adjacent pixels of the SAME color.
                    If False, groups all adjacent non-background pixels.
    """
    h, w = len(grid), len(grid[0])
    visited: Set[Tuple[int, int]] = set()
    objects: List[Object] = []

    if connectivity == 8:
        neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (-1, 1), (1, -1), (1, 1)]
    else:
        neighbors = [(-1, 0), (1, 0), (0, -1), (0, 1)]

    for r in range(h):
        for c in range(w):
            val = grid[r][c]
            if val == background_color or (r, c) in visited:
                continue

            # Flood fill new component
            queue = [(r, c)]
            visited.add((r, c))
            component_pixels: List[Tuple[int, int]] = []
            component_colors: List[int] = []

            while queue:
                curr_r, curr_c = queue.pop(0)
                component_pixels.append((curr_r, curr_c))
                component_colors.append(grid[curr_r][curr_c])

                for dr, dc in neighbors:
                    nr, nc = curr_r + dr, curr_c + dc
                    if 0 <= nr < h and 0 <= nc < w and (nr, nc) not in visited:
                        n_val = grid[nr][nc]
                        if n_val != background_color:
                            if not monochrome or n_val == val:
                                visited.add((nr, nc))
                                queue.append((nr, nc))

            min_r = min(p[0] for p in component_pixels)
            max_r = max(p[0] for p in component_pixels)
            min_c = min(p[1] for p in component_pixels)
            max_c = max(p[1] for p in component_pixels)

            # Dominant color
            from collections import Counter
            dom_color = Counter(component_colors).most_common(1)[0][0]

            objects.append(
                Object(
                    pixels=tuple(component_pixels),
                    colors=tuple(component_colors),
                    color=dom_color,
                    min_r=min_r,
                    min_c=min_c,
                    max_r=max_r,
                    max_c=max_c,
                )
            )

    return objects

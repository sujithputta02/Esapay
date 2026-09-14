"""
Color priors and palette definitions for ARC-AGI.
"""

from typing import Dict, List, Tuple
from collections import Counter

# Standard ARC-AGI 10-color palette mapping
ARC_COLORS = {
    0: ("Black", "#000000", "\033[40m"),
    1: ("Blue", "#1E88E5", "\033[44m"),
    2: ("Red", "#E53935", "\033[41m"),
    3: ("Green", "#43A047", "\033[42m"),
    4: ("Yellow", "#FDD835", "\033[43m"),
    5: ("Grey", "#8E8E8E", "\033[100m"),
    6: ("Magenta", "#D81B60", "\033[45m"),
    7: ("Orange", "#FB8C00", "\033[48;5;208m"),
    8: ("Teal", "#00ACC1", "\033[46m"),
    9: ("Maroon", "#8E24AA", "\033[48;5;88m"),
}

Grid = Tuple[Tuple[int, ...], ...]


def get_color_histogram(grid: Grid) -> Dict[int, int]:
    """Returns frequency count of each color in the grid."""
    counter: Counter[int] = Counter()
    for row in grid:
        counter.update(row)
    return dict(counter)


def detect_background_color(grid: Grid) -> int:
    """
    Detects the dominant background color.
    Prioritizes 0 (Black) if present on borders/dominant, otherwise returns most frequent border color.
    """
    h, w = len(grid), len(grid[0])
    border_colors: List[int] = []
    
    # Top & bottom rows
    border_colors.extend(grid[0])
    if h > 1:
        border_colors.extend(grid[-1])
    # Left & right columns (excluding corners already counted)
    for r in range(1, h - 1):
        border_colors.append(grid[r][0])
        if w > 1:
            border_colors.append(grid[r][-1])
            
    if not border_colors:
        return grid[0][0]

    border_counter = Counter(border_colors)
    # If 0 (black) is significantly present on border, it's virtually always background
    if border_counter.get(0, 0) >= len(border_colors) * 0.25:
        return 0
    return border_counter.most_common(1)[0][0]


def get_foreground_colors(grid: Grid, bg_color: int = None) -> List[int]:
    """Returns all colors present excluding the background color."""
    if bg_color is None:
        bg_color = detect_background_color(grid)
    hist = get_color_histogram(grid)
    return [c for c in hist.keys() if c != bg_color]

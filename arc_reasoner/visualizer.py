"""
Visual terminal rendering engine for ARC-AGI grids and thought traces.
Uses standard ANSI 256 colors for vivid terminal visualization.
"""

from typing import List, Optional, Tuple
from arc_reasoner.priors.color import ARC_COLORS, detect_background_color
from arc_reasoner.priors.grid import Grid
from arc_reasoner.priors.objects import Object

RESET = "\033[0m"
BOLD = "\033[1m"
DIM = "\033[2m"
CYAN = "\033[36m"
GREEN = "\033[32m"
YELLOW = "\033[33m"
RED = "\033[31m"
MAGENTA = "\033[35m"


class Visualizer:
    """Renders ARC-AGI grids and thought processes to terminal."""

    @staticmethod
    def color_block(val: int) -> str:
        """Returns colored cell representation for ARC color value."""
        color_info = ARC_COLORS.get(val, ("Unknown", "#ffffff", "\033[47m"))
        bg_code = color_info[2]
        # Two-character block with contrast text
        text_color = "\033[30m" if val in (0, 4, 5, 7, 8) else "\033[37m"
        return f"{bg_code}{text_color}{val:2d}{RESET}"

    @classmethod
    def render_grid(cls, grid: Grid, title: Optional[str] = None) -> str:
        """Renders a single grid to an ANSI color string."""
        if not grid:
            return "(empty grid)"
        lines = []
        if title:
            h, w = len(grid), len(grid[0])
            lines.append(f"{BOLD}{title}{RESET} {DIM}({h}x{w}){RESET}")

        for row in grid:
            row_str = "".join(cls.color_block(c) for c in row)
            lines.append(row_str)
        return "\n".join(lines)

    @classmethod
    def render_side_by_side(
        cls,
        grid_left: Grid,
        grid_right: Grid,
        title_left: str = "Input",
        title_right: str = "Output",
        sep: str = "   ->   "
    ) -> str:
        """Renders two grids side-by-side."""
        lh, lw = (len(grid_left), len(grid_left[0])) if grid_left else (0, 0)
        rh, rw = (len(grid_right), len(grid_right[0])) if grid_right else (0, 0)
        max_h = max(lh, rh)

        header = f"{BOLD}{title_left:^{lw * 2}}{RESET}{sep}{BOLD}{title_right:^{rw * 2}}{RESET}"
        lines = [header]

        for r in range(max_h):
            if r < lh:
                left_row = "".join(cls.color_block(c) for c in grid_left[r])
            else:
                left_row = "  " * lw

            if r < rh:
                right_row = "".join(cls.color_block(c) for c in grid_right[r])
            else:
                right_row = "  " * rw

            lines.append(f"{left_row}{sep}{right_row}")

        return "\n".join(lines)

    @classmethod
    def print_thought(cls, step: str, details: str):
        """Prints an autonomous agent thought log."""
        print(f"\n{CYAN}{BOLD}[THINKING]{RESET} {YELLOW}{step}:{RESET} {details}")

    @classmethod
    def print_success(cls, message: str):
        print(f"\n{GREEN}{BOLD}✔ {message}{RESET}")

    @classmethod
    def print_failure(cls, message: str):
        print(f"\n{RED}{BOLD}✘ {message}{RESET}")

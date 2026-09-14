"""
Priors module exposing grid geometry, objectness, topology, and color representations.
"""

from arc_reasoner.priors.color import (
    ARC_COLORS,
    detect_background_color,
    get_color_histogram,
    get_foreground_colors,
)
from arc_reasoner.priors.grid import (
    Grid,
    create_empty_grid,
    crop,
    flip_h,
    flip_v,
    get_bounding_box,
    overlay,
    pad,
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
from arc_reasoner.priors.topology import extract_perimeter, fill_enclosed_holes, find_enclosed_holes

__all__ = [
    "Grid",
    "ARC_COLORS",
    "to_grid",
    "to_mutable",
    "create_empty_grid",
    "rotate90",
    "rotate180",
    "rotate270",
    "flip_v",
    "flip_h",
    "transpose",
    "crop",
    "pad",
    "scale",
    "tile",
    "overlay",
    "get_bounding_box",
    "detect_background_color",
    "get_color_histogram",
    "get_foreground_colors",
    "Object",
    "extract_objects",
    "find_enclosed_holes",
    "fill_enclosed_holes",
    "extract_perimeter",
]

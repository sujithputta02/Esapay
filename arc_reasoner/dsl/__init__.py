"""
DSL package exposing primitives, AST nodes, and interpreter.
"""

from arc_reasoner.dsl.ast_nodes import (
    ASTNode,
    IdentityNode,
    PipelineNode,
    PrimitiveNode,
)
from arc_reasoner.dsl.interpreter import execute_program
from arc_reasoner.dsl.primitives import (
    apply_gravity,
    complete_symmetry,
    crop_to_content,
    extract_largest_object_grid,
    extract_smallest_object_grid,
    fill_holes,
    filter_keep_color,
    recolor,
    recolor_all_foreground,
    reflect_h,
    reflect_v,
    rotate_ccw,
    rotate_cw,
    rotate_half,
)

__all__ = [
    "ASTNode",
    "PrimitiveNode",
    "PipelineNode",
    "IdentityNode",
    "execute_program",
    "recolor",
    "recolor_all_foreground",
    "crop_to_content",
    "rotate_cw",
    "rotate_ccw",
    "rotate_half",
    "reflect_h",
    "reflect_v",
    "fill_holes",
    "apply_gravity",
    "extract_largest_object_grid",
    "extract_smallest_object_grid",
    "filter_keep_color",
    "complete_symmetry",
]

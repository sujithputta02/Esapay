"""
Guided enumerator and beam search for synthesizing ARC programs.
"""

from typing import List, Optional, Tuple
from arc_reasoner.dsl.ast_nodes import ASTNode, IdentityNode, PipelineNode, PrimitiveNode
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
from arc_reasoner.priors.color import detect_background_color, get_color_histogram
from arc_reasoner.priors.grid import Grid, scale, tile
from arc_reasoner.synthesis.cost import compute_program_score
from arc_reasoner.synthesis.induction import analyze_demonstrations


class ProgramSynthesizer:
    """Synthesizes candidate programs to explain demonstration input/output pairs."""

    def __init__(self, max_depth: int = 2):
        self.max_depth = max_depth

    def generate_candidate_primitives(self, examples: List[Tuple[Grid, Grid]]) -> List[ASTNode]:
        """Generates domain-relevant primitive candidate nodes based on problem analysis."""
        analysis = analyze_demonstrations(examples)
        candidates: List[ASTNode] = [IdentityNode()]

        # Colors observed
        all_colors = set()
        for in_g, out_g in examples:
            all_colors.update(get_color_histogram(in_g).keys())
            all_colors.update(get_color_histogram(out_g).keys())

        # 1. Geometric transforms
        candidates.extend([
            PrimitiveNode("rotate_cw", rotate_cw),
            PrimitiveNode("rotate_ccw", rotate_ccw),
            PrimitiveNode("rotate_half", rotate_half),
            PrimitiveNode("reflect_h", reflect_h),
            PrimitiveNode("reflect_v", reflect_v),
        ])

        # 2. Crop & Object Extraction
        candidates.extend([
            PrimitiveNode("crop_to_content", crop_to_content),
            PrimitiveNode("extract_largest_object_grid", extract_largest_object_grid),
            PrimitiveNode("extract_smallest_object_grid", extract_smallest_object_grid),
        ])

        # 3. Scaling / Tiling
        if analysis.is_scale:
            k = analysis.is_scale
            candidates.append(
                PrimitiveNode("scale", scale, args=(k,))
            )
        for k in (2, 3):
            candidates.append(PrimitiveNode("scale", scale, args=(k,)))
            candidates.append(PrimitiveNode("tile", tile, args=(k, k)))

        # 4. Color transformations
        for c in all_colors:
            candidates.append(
                PrimitiveNode("recolor_all_foreground", recolor_all_foreground, args=(c,))
            )
            candidates.append(
                PrimitiveNode("filter_keep_color", filter_keep_color, args=(c,))
            )
            # Hole filling with color c
            candidates.append(
                PrimitiveNode("fill_holes", fill_holes, args=(c,))
            )

        # Pairwise recoloring based on color delta analysis
        if analysis.color_mapping:
            for old_c, new_c in analysis.color_mapping.items():
                candidates.append(
                    PrimitiveNode("recolor", recolor, args=(old_c, new_c))
                )
        else:
            for old_c in all_colors:
                for new_c in all_colors:
                    if old_c != new_c:
                        candidates.append(
                            PrimitiveNode("recolor", recolor, args=(old_c, new_c))
                        )

        # 5. Physics & Symmetry
        for direction in ("down", "up", "left", "right"):
            candidates.append(
                PrimitiveNode("apply_gravity", apply_gravity, args=(direction,))
            )
        for axis in ("vertical", "horizontal"):
            candidates.append(
                PrimitiveNode("complete_symmetry", complete_symmetry, args=(axis,))
            )

        return candidates

    def synthesize(self, examples: List[Tuple[Grid, Grid]]) -> Optional[ASTNode]:
        """
        Searches for a program that satisfies 100% of demonstration examples.
        First tests depth-1 primitives, then depth-2 pipelines.
        """
        candidates = self.generate_candidate_primitives(examples)

        best_program: Optional[ASTNode] = None
        best_score = float("inf")

        # Pass 1: Single primitive candidates
        for node in candidates:
            score, is_exact = compute_program_score(node, examples)
            if is_exact:
                return node
            if score < best_score:
                best_score = score
                best_program = node

        if self.max_depth < 2:
            return best_program

        # Pass 2: Two-stage pipelines (chaining 2 primitives)
        # Filter top single candidates to avoid combinatorial explosion
        scored_candidates = []
        for node in candidates:
            score, _ = compute_program_score(node, examples)
            scored_candidates.append((score, node))
        scored_candidates.sort(key=lambda x: x[0])
        top_candidates = [n for _, n in scored_candidates[:25]]

        for step1 in top_candidates:
            for step2 in top_candidates:
                # Avoid redundant identities
                if isinstance(step1, IdentityNode) or isinstance(step2, IdentityNode):
                    continue
                pipe = PipelineNode(steps=[step1, step2])
                score, is_exact = compute_program_score(pipe, examples)
                if is_exact:
                    return pipe
                if score < best_score:
                    best_score = score
                    best_program = pipe

        return best_program

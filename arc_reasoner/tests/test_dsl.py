"""
Unit tests for DSL primitives, AST nodes, and interpreter execution.
"""

import unittest
from arc_reasoner.dsl.ast_nodes import PipelineNode, PrimitiveNode
from arc_reasoner.dsl.interpreter import execute_program
from arc_reasoner.dsl.primitives import (
    apply_gravity,
    complete_symmetry,
    crop_to_content,
    fill_holes,
    recolor,
)
from arc_reasoner.priors.grid import to_grid


class TestDSL(unittest.TestCase):

    def test_recolor_primitive(self):
        grid = to_grid([[1, 2], [1, 3]])
        res = recolor(grid, 1, 9)
        self.assertEqual(res, to_grid([[9, 2], [9, 3]]))

    def test_gravity_down(self):
        grid = to_grid([
            [1, 0, 2],
            [0, 0, 0],
            [0, 0, 0],
        ])
        res = apply_gravity(grid, direction="down", bg_color=0)
        self.assertEqual(res, to_grid([
            [0, 0, 0],
            [0, 0, 0],
            [1, 0, 2],
        ]))

    def test_complete_vertical_symmetry(self):
        # Left half has content, right half empty -> mirror to right half
        grid = to_grid([
            [1, 0, 0, 0],
            [2, 3, 0, 0],
        ])
        res = complete_symmetry(grid, axis="vertical")
        self.assertEqual(res, to_grid([
            [1, 0, 0, 1],
            [2, 3, 3, 2],
        ]))

    def test_ast_pipeline_execution(self):
        step1 = PrimitiveNode("crop", crop_to_content)
        step2 = PrimitiveNode("recolor", recolor, args=(1, 4))
        pipeline = PipelineNode(steps=[step1, step2])

        grid = to_grid([
            [0, 0, 0, 0],
            [0, 1, 1, 0],
            [0, 0, 0, 0],
        ])
        success, res, err = execute_program(pipeline, grid)
        self.assertTrue(success)
        self.assertIsNone(err)
        self.assertEqual(res, to_grid([[4, 4]]))


if __name__ == "__main__":
    unittest.main()

"""
Unit tests for program synthesis, induction analysis, and cost functions.
"""

import unittest
from arc_reasoner.dsl.ast_nodes import PrimitiveNode
from arc_reasoner.dsl.primitives import rotate_cw
from arc_reasoner.priors.grid import to_grid
from arc_reasoner.synthesis.cost import compute_grid_mismatch_loss, compute_program_score
from arc_reasoner.synthesis.enumerator import ProgramSynthesizer
from arc_reasoner.synthesis.induction import analyze_demonstrations


class TestSynthesis(unittest.TestCase):

    def test_mismatch_loss(self):
        g1 = to_grid([[1, 2], [3, 4]])
        g2 = to_grid([[1, 2], [3, 4]])
        g3 = to_grid([[1, 2], [3, 9]])
        self.assertEqual(compute_grid_mismatch_loss(g1, g2), 0.0)
        self.assertAlmostEqual(compute_grid_mismatch_loss(g1, g3), 0.25)

    def test_synthesizer_rotates(self):
        train = [
            (to_grid([[1, 2], [3, 4]]), to_grid([[3, 1], [4, 2]])),
            (to_grid([[5, 6], [7, 8]]), to_grid([[7, 5], [8, 6]])),
        ]
        synthesizer = ProgramSynthesizer(max_depth=1)
        program = synthesizer.synthesize(train)
        self.assertIsNotNone(program)
        assert program is not None
        score, is_exact = compute_program_score(program, train)
        self.assertTrue(is_exact)

    def test_induction_analysis(self):
        train = [
            (to_grid([[1, 1], [1, 1]]), to_grid([[2, 2], [2, 2]])),
        ]
        analysis = analyze_demonstrations(train)
        self.assertTrue(analysis.same_dimensions)
        self.assertIn(2, analysis.colors_added)
        self.assertIn(1, analysis.colors_removed)


if __name__ == "__main__":
    unittest.main()

"""
Unit tests for Core Knowledge Priors: Grid geometry, Objectness, Topology, Color.
"""

import unittest
from arc_reasoner.priors.color import detect_background_color, get_color_histogram
from arc_reasoner.priors.grid import (
    crop,
    flip_h,
    flip_v,
    get_bounding_box,
    rotate90,
    rotate180,
    rotate270,
    scale,
    tile,
    to_grid,
)
from arc_reasoner.priors.objects import extract_objects
from arc_reasoner.priors.topology import fill_enclosed_holes, find_enclosed_holes


class TestPriors(unittest.TestCase):

    def test_grid_rotations(self):
        grid = to_grid([
            [1, 2],
            [3, 4],
        ])
        rot90 = rotate90(grid)
        self.assertEqual(rot90, to_grid([
            [3, 1],
            [4, 2],
        ]))
        rot180 = rotate180(grid)
        self.assertEqual(rot180, to_grid([
            [4, 3],
            [2, 1],
        ]))
        rot270 = rotate270(grid)
        self.assertEqual(rot270, to_grid([
            [2, 4],
            [1, 3],
        ]))

    def test_grid_reflections(self):
        grid = to_grid([
            [1, 2],
            [3, 4],
        ])
        self.assertEqual(flip_h(grid), to_grid([
            [2, 1],
            [4, 3],
        ]))
        self.assertEqual(flip_v(grid), to_grid([
            [3, 4],
            [1, 2],
        ]))

    def test_crop_and_bbox(self):
        grid = to_grid([
            [0, 0, 0, 0],
            [0, 5, 5, 0],
            [0, 5, 5, 0],
            [0, 0, 0, 0],
        ])
        bbox = get_bounding_box(grid, background_color=0)
        self.assertEqual(bbox, (1, 1, 2, 2))
        cropped = crop(grid, *bbox)
        self.assertEqual(cropped, to_grid([
            [5, 5],
            [5, 5],
        ]))

    def test_connected_component_extraction(self):
        grid = to_grid([
            [1, 1, 0, 2],
            [1, 0, 0, 2],
            [0, 0, 0, 0],
            [3, 3, 3, 0],
        ])
        objs = extract_objects(grid, background_color=0, connectivity=4)
        self.assertEqual(len(objs), 3)
        colors = {o.color for o in objs}
        self.assertEqual(colors, {1, 2, 3})

    def test_topology_holes(self):
        # Ring of 1s enclosing 0s in the center
        grid = to_grid([
            [0, 0, 0, 0, 0],
            [0, 1, 1, 1, 0],
            [0, 1, 0, 1, 0],
            [0, 1, 1, 1, 0],
            [0, 0, 0, 0, 0],
        ])
        holes = find_enclosed_holes(grid, bg_color=0)
        self.assertEqual(holes, {(2, 2)})

        filled = fill_enclosed_holes(grid, fill_color=4, bg_color=0)
        self.assertEqual(filled[2][2], 4)
        self.assertEqual(filled[0][0], 0)  # Exterior remains background


if __name__ == "__main__":
    unittest.main()

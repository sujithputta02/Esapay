"""
End-to-end integration tests: verifies the ARCReasoner solves diverse benchmark tasks autonomously.
"""

import unittest
from arc_reasoner.benchmark import get_benchmark_tasks
from arc_reasoner.engine import ARCReasoner


class TestEndToEnd(unittest.TestCase):

    def setUp(self):
        self.reasoner = ARCReasoner(verbose=False, max_depth=2)

    def test_all_benchmark_tasks(self):
        tasks = get_benchmark_tasks()
        self.assertGreater(len(tasks), 0)

        for task in tasks:
            with self.subTest(task=task.name):
                test_inputs = [t_in for t_in, _ in task.test]
                outcome = self.reasoner.solve(task.train, test_inputs)

                # The reasoner must succeed on the training demonstrations
                self.assertTrue(
                    outcome.success,
                    f"Task {task.name} failed training verification with accuracy {outcome.train_accuracy}"
                )

                # The test prediction must match ground truth
                predicted = outcome.predicted_test_outputs[0]
                expected = task.test[0][1]
                self.assertEqual(
                    predicted,
                    expected,
                    f"Task {task.name} failed test prediction: Expected {expected}, Got {predicted}"
                )


if __name__ == "__main__":
    unittest.main()

"""Self-checks of the Python image path (the Rust side is checked against
golden files in crates/vision/tests/parity.rs)."""
import unittest

import numpy as np

from facetilt import boxfilter, labels


class BoxFilterTest(unittest.TestCase):
    def test_constant_and_rounding(self):
        y = np.full((144, 176), 77, np.uint8)
        self.assertTrue((boxfilter.box_downsample(y) == 77).all())
        y = np.zeros((64, 64), np.uint8)
        y[0, 0] = 1  # (1+2)//4 = 0
        y[0, 2] = 2  # (2+2)//4 = 1
        y[0, 4:6] = 255
        y[1, 4], y[1, 5] = 255, 254  # (1019+2)//4 = 255
        self.assertEqual(boxfilter.box_downsample(y)[0, :3].tolist(), [0, 1, 255])

    def test_centre_crop(self):
        # 100x70: k=2, crop 64 at (18, 3) - same as the Rust unit test.
        y = np.full((70, 100), 10, np.uint8)
        y[3:67, 18:82] = 200
        self.assertTrue((boxfilter.box_downsample(y) == 200).all())

    def test_orientation_matches_rust_convention(self):
        t = np.zeros((32, 32), np.uint8)
        t[0, 0] = 255
        pos = lambda a: tuple(int(v) for v in np.argwhere(a == 255)[0])
        self.assertEqual(pos(boxfilter.to_mirror_view(t, 90, False)), (0, 31))
        self.assertEqual(pos(boxfilter.to_mirror_view(t, 180, False)), (31, 31))
        self.assertEqual(pos(boxfilter.to_mirror_view(t, 270, False)), (31, 0))
        t = np.zeros((32, 32), np.uint8)
        t[0, 5] = 255
        self.assertEqual(pos(boxfilter.to_mirror_view(t, 270, True)), (26, 31))

    def test_standardize(self):
        self.assertIsNone(boxfilter.standardize(np.zeros((32, 32), np.uint8)))
        x = boxfilter.standardize((np.arange(1024) % 256).astype(np.uint8).reshape(32, 32))
        self.assertAlmostEqual(float(x.mean()), 0.0, places=5)
        self.assertAlmostEqual(float(x.std()), 1.0, places=4)

    def test_rotation_labels(self):
        # Head top moves left when the image is rotated counter-clockwise.
        self.assertEqual(labels.ROT90_K_TO_LABEL[1], labels.ROTATED_LEFT)
        self.assertEqual(labels.ROT90_K_TO_LABEL[3], labels.ROTATED_RIGHT)
        self.assertEqual(labels.NAMES[labels.NO_FACE], "no_face")


if __name__ == "__main__":
    unittest.main()

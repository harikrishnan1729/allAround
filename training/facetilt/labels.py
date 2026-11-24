"""Class indices. MUST match `HeadClass` in crates/decision/src/lib.rs.

All classes describe the face as it appears in the mirror-view image in the
device's natural orientation (see crates/vision/src/orient.rs).
"""
UPRIGHT = 0        # head top -> image top
ROTATED_LEFT = 1   # face rotated 90 deg counter-clockwise: head top -> image left
ROTATED_RIGHT = 2  # face rotated 90 deg clockwise: head top -> image right
UPSIDE_DOWN = 3    # head top -> image bottom
NO_FACE = 4        # no face / unsure

NAMES = ["upright", "rotated_left", "rotated_right", "upside_down", "no_face"]
NUM_CLASSES = 5

# np.rot90(img, k) rotates counter-clockwise by 90*k degrees.
# Upright face rotated CCW by 90 -> head top points left -> ROTATED_LEFT.
ROT90_K_TO_LABEL = {0: UPRIGHT, 1: ROTATED_LEFT, 2: UPSIDE_DOWN, 3: ROTATED_RIGHT}

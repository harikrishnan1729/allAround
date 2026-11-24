package dev.facetilt

import android.content.pm.ActivityInfo
import dev.facetilt.rotation.RotationMapping
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RotationMappingTest {
    // android.jar constants are inlined at compile time, so these work in local JVM tests.
    @Test fun naturalPortraitPhone() {
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT, RotationMapping.screenOrientationFor(0, true))
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE, RotationMapping.screenOrientationFor(1, true))
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_REVERSE_PORTRAIT, RotationMapping.screenOrientationFor(2, true))
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE, RotationMapping.screenOrientationFor(3, true))
    }

    @Test fun naturalLandscapeTablet() {
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE, RotationMapping.screenOrientationFor(0, false))
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_REVERSE_PORTRAIT, RotationMapping.screenOrientationFor(1, false))
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE, RotationMapping.screenOrientationFor(2, false))
        assertEquals(ActivityInfo.SCREEN_ORIENTATION_PORTRAIT, RotationMapping.screenOrientationFor(3, false))
    }

    @Test fun naturalOrientationDetection() {
        assertTrue(RotationMapping.isNaturalPortrait(0, 1080, 2400))
        assertTrue(RotationMapping.isNaturalPortrait(1, 2400, 1080))
        assertFalse(RotationMapping.isNaturalPortrait(0, 2560, 1600))
        assertFalse(RotationMapping.isNaturalPortrait(3, 1600, 2560))
    }
}

package dev.facetilt

import android.Manifest
import android.content.pm.PackageManager
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.core.content.ContextCompat
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import kotlinx.coroutines.runBlocking
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Permission flow on a real device/emulator:
 * consent given → master switch → camera rationale → system dialog → DENY → graceful
 * "camera not allowed" screen (FaceTilt stays off) → grant → flow continues past the camera step.
 *
 * Run on a fresh install (the camera permission must not be granted yet):
 *   ./gradlew connectedDebugAndroidTest
 */
@RunWith(AndroidJUnit4::class)
class PermissionFlowInstrumentedTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val context = instrumentation.targetContext

    @Before
    fun setUp() {
        // Revoking a runtime permission kills the process, so we can only run from a clean state.
        assumeTrue(
            "camera already granted; reinstall the app to run this test",
            ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) != PackageManager.PERMISSION_GRANTED,
        )
        runBlocking {
            val repo = (context.applicationContext as FaceTiltApp).settings
            repo.deleteAll()
            repo.setConsent(true)
        }
    }

    @Test
    fun denyingCameraLeavesFaceTiltOffAndGrantingContinues() {
        compose.waitUntil(5_000) { compose.onAllNodesWithTagExists("master_switch") }
        compose.onNodeWithTag("master_switch").performClick()

        compose.onNodeWithTag("camera_rationale").assertIsDisplayed()
        compose.onNodeWithTag("camera_request").performScrollTo().performClick()

        // System permission dialog (permission controller ids are stable on Android 10–15).
        val deny = device.wait(
            Until.findObject(By.res("com.android.permissioncontroller", "permission_deny_button")),
            5_000,
        ) ?: device.wait(Until.findObject(By.res("com.android.permissioncontroller", "permission_deny_and_dont_ask_again_button")), 2_000)
        checkNotNull(deny) { "permission dialog not shown" }.click()

        compose.waitUntil(5_000) { compose.onAllNodesWithTagExists("camera_denied") }
        compose.onNodeWithTag("camera_denied").assertIsDisplayed()

        // Graceful denial: the user can leave and FaceTilt stays off.
        compose.onNodeWithTag("camera_denied_cancel").performScrollTo().performClick()
        compose.waitUntil(5_000) { compose.onAllNodesWithTagExists("home") }
        runBlocking {
            val s = (context.applicationContext as FaceTiltApp).settings.current()
            check(!s.enabled) { "FaceTilt must stay disabled after a camera denial" }
        }

        // Grant (as if from system settings) and retry: the flow moves past the camera step.
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.CAMERA)
        compose.onNodeWithTag("master_switch").performClick()
        compose.waitUntil(5_000) {
            compose.onAllNodesWithTagExists("notification_rationale") ||
                compose.onAllNodesWithTagExists("rotation_permission") ||
                compose.onAllNodesWithTagExists("home")
        }
    }
}

private fun androidx.compose.ui.test.junit4.AndroidComposeTestRule<*, *>.onAllNodesWithTagExists(tag: String): Boolean =
    onAllNodes(androidx.compose.ui.test.hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty()

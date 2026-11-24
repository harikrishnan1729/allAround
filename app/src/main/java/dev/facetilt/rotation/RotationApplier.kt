package dev.facetilt.rotation

import android.content.ContentResolver
import android.content.Context
import android.content.pm.ActivityInfo
import android.graphics.PixelFormat
import android.hardware.display.DisplayManager
import android.os.Build
import android.provider.Settings
import android.view.Display
import android.view.Gravity
import android.view.Surface
import android.view.View
import android.view.WindowManager

/**
 * Applies a `Surface.ROTATION_*` value to the default display.
 *
 * Two operating modes, chosen at [activate] time by `takeControl`:
 * * **Normal** (`takeControl = true`): FaceTilt is the auto-rotator the whole time — it holds
 *   rotation control continuously and [apply]s every decision.
 * * **Assist** (`takeControl = false`): the system's own auto-rotate keeps running; FaceTilt
 *   only [seizeControl]s briefly to evaluate a tilt, [apply]s (pins) portrait when the user is
 *   lying down, and [releaseControl]s to hand auto-rotate back when they are upright again.
 */
interface RotationApplier {
    /**
     * Prepares the applier. `takeControl = true` takes rotation control now (normal mode);
     * `false` leaves the system's auto-rotate running until [seizeControl] (assist mode).
     * Returns false if the required permission is missing.
     */
    fun activate(currentRotation: Int, takeControl: Boolean): Boolean

    /** Sets the shown orientation, taking control first if necessary. */
    fun apply(rotation: Int)

    /** Assist mode: take rotation control from the system, keeping [currentRotation] shown. */
    fun seizeControl(currentRotation: Int)

    /** Assist mode: hand rotation control back to the system's own auto-rotate (stay ready). */
    fun releaseControl()

    /** Gives control back to the system permanently and restores the user's settings. */
    fun release()
}

/** Pure mapping helpers (unit-tested in RotationMappingTest). */
object RotationMapping {
    /**
     * `ActivityInfo.SCREEN_ORIENTATION_*` for a `Surface.ROTATION_*` value, following AOSP
     * `DisplayRotation` with `config_reverseDefaultRotation = false` (the default on almost all
     * devices). Devices that set that overlay swap 90/270 on natural-landscape hardware; the
     * [SystemSettingApplier] does not have this limitation.
     */
    fun screenOrientationFor(rotation: Int, naturalPortrait: Boolean): Int = if (naturalPortrait) {
        when (rotation) {
            Surface.ROTATION_90 -> ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
            Surface.ROTATION_180 -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_PORTRAIT
            Surface.ROTATION_270 -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE
            else -> ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
        }
    } else {
        when (rotation) {
            Surface.ROTATION_90 -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_PORTRAIT
            Surface.ROTATION_180 -> ActivityInfo.SCREEN_ORIENTATION_REVERSE_LANDSCAPE
            Surface.ROTATION_270 -> ActivityInfo.SCREEN_ORIENTATION_PORTRAIT
            else -> ActivityInfo.SCREEN_ORIENTATION_LANDSCAPE
        }
    }

    /** Whether the display is portrait in its natural (ROTATION_0) orientation. */
    fun isNaturalPortrait(currentRotation: Int, width: Int, height: Int): Boolean {
        val rotated = currentRotation == Surface.ROTATION_90 || currentRotation == Surface.ROTATION_270
        return if (rotated) width > height else height >= width
    }
}

/**
 * Applier using `Settings.System.USER_ROTATION` (needs WRITE_SETTINGS). While it holds control it
 * sets the system's gravity auto-rotate off and becomes the auto-rotator; apps that request a
 * fixed orientation still get it. The user's original values are restored on [release] (and on
 * next app start if the process was killed).
 */
class SystemSettingApplier(
    private val context: Context,
    private val onSaveOriginal: (accelerometer: Int, user: Int) -> Unit,
) : RotationApplier {
    private val resolver: ContentResolver get() = context.contentResolver
    private var originalAccel = -1

    private fun saveOriginal() {
        originalAccel = Settings.System.getInt(resolver, Settings.System.ACCELEROMETER_ROTATION, 1)
        val originalUser = Settings.System.getInt(resolver, Settings.System.USER_ROTATION, Surface.ROTATION_0)
        onSaveOriginal(originalAccel, originalUser)
    }

    override fun activate(currentRotation: Int, takeControl: Boolean): Boolean {
        if (!Settings.System.canWrite(context)) return false
        saveOriginal()
        if (takeControl) {
            Settings.System.putInt(resolver, Settings.System.USER_ROTATION, currentRotation)
            Settings.System.putInt(resolver, Settings.System.ACCELEROMETER_ROTATION, 0)
        }
        // Assist mode: leave the system's auto-rotate exactly as the user has it.
        return true
    }

    override fun apply(rotation: Int) {
        if (!Settings.System.canWrite(context)) return
        Settings.System.putInt(resolver, Settings.System.ACCELEROMETER_ROTATION, 0)
        Settings.System.putInt(resolver, Settings.System.USER_ROTATION, rotation)
    }

    override fun seizeControl(currentRotation: Int) {
        if (!Settings.System.canWrite(context)) return
        Settings.System.putInt(resolver, Settings.System.USER_ROTATION, currentRotation)
        Settings.System.putInt(resolver, Settings.System.ACCELEROMETER_ROTATION, 0)
    }

    override fun releaseControl() {
        if (!Settings.System.canWrite(context)) return
        // Re-enable the system's own auto-rotate; it immediately follows gravity.
        Settings.System.putInt(resolver, Settings.System.ACCELEROMETER_ROTATION, 1)
    }

    override fun release() {
        if (originalAccel >= 0 && Settings.System.canWrite(context)) {
            Settings.System.putInt(resolver, Settings.System.ACCELEROMETER_ROTATION, originalAccel)
        }
        originalAccel = -1
    }

    companion object {
        /** Restores values saved by a previous process that died while active. */
        fun restoreSaved(context: Context, accelerometer: Int, user: Int): Boolean {
            if (accelerometer < 0 || !Settings.System.canWrite(context)) return false
            if (user >= 0) Settings.System.putInt(context.contentResolver, Settings.System.USER_ROTATION, user)
            Settings.System.putInt(context.contentResolver, Settings.System.ACCELEROMETER_ROTATION, accelerometer)
            return true
        }
    }
}

/**
 * Applier using a 1×1 transparent, non-touchable `TYPE_APPLICATION_OVERLAY` window whose
 * `screenOrientation` pins the display orientation (needs SYSTEM_ALERT_WINDOW). Because the
 * overlay sits above apps, its orientation also overrides apps that request their own. In assist
 * mode, removing the overlay hands rotation back to whatever the user's own system setting is.
 */
class OverlayApplier(private val context: Context) : RotationApplier {
    private var view: View? = null
    private var params: WindowManager.LayoutParams? = null
    private var naturalPortrait = true

    private fun windowContext(): Context {
        val display = context.getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)
        // API 30+: overlays must be added from a window context (StrictMode IncorrectContextUseViolation).
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            context.createDisplayContext(display).createWindowContext(WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY, null)
        } else {
            context
        }
    }

    private fun realSize(): Pair<Int, Int> =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            val b = context.getSystemService(WindowManager::class.java).maximumWindowMetrics.bounds
            b.width() to b.height()
        } else {
            val p = android.graphics.Point()
            @Suppress("DEPRECATION")
            context.getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY).getRealSize(p)
            p.x to p.y
        }

    /** Adds the overlay pinned to [rotation] if it is not up yet; returns false on failure. */
    private fun addOverlay(rotation: Int): Boolean {
        if (view != null) {
            apply(rotation)
            return true
        }
        if (!Settings.canDrawOverlays(context)) return false
        val ctx = windowContext()
        val (w, h) = realSize()
        naturalPortrait = RotationMapping.isNaturalPortrait(rotation, w, h)
        val lp = WindowManager.LayoutParams(
            1, 1,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCHABLE or
                WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
            PixelFormat.TRANSPARENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            alpha = 0f
            screenOrientation = RotationMapping.screenOrientationFor(rotation, naturalPortrait)
        }
        val v = View(ctx)
        return try {
            ctx.getSystemService(WindowManager::class.java).addView(v, lp)
            view = v
            params = lp
            true
        } catch (e: RuntimeException) {
            false
        }
    }

    private fun removeOverlay() {
        val v = view ?: return
        try {
            v.context.getSystemService(WindowManager::class.java).removeView(v)
        } catch (_: RuntimeException) {
        }
        view = null
        params = null
    }

    override fun activate(currentRotation: Int, takeControl: Boolean): Boolean {
        if (!Settings.canDrawOverlays(context)) return false
        // Assist mode: don't add the overlay until we actually need to hold an orientation.
        return if (takeControl) addOverlay(currentRotation) else true
    }

    override fun apply(rotation: Int) {
        if (view == null) {
            addOverlay(rotation)
            return
        }
        val v = view ?: return
        val lp = params ?: return
        lp.screenOrientation = RotationMapping.screenOrientationFor(rotation, naturalPortrait)
        try {
            v.context.getSystemService(WindowManager::class.java).updateViewLayout(v, lp)
        } catch (_: RuntimeException) {
        }
    }

    override fun seizeControl(currentRotation: Int) {
        addOverlay(currentRotation)
    }

    override fun releaseControl() {
        removeOverlay()
    }

    override fun release() {
        removeOverlay()
    }
}

package dev.facetilt.service

import android.Manifest
import android.app.KeyguardManager
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.content.res.Configuration
import android.database.ContentObserver
import android.hardware.camera2.CameraAccessException
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraManager
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import android.provider.Settings
import android.view.Display
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import dev.facetilt.BuildConfig
import dev.facetilt.FaceTiltApp
import dev.facetilt.MainActivity
import dev.facetilt.R
import dev.facetilt.data.ApplierMode
import dev.facetilt.data.Settings as AppSettings
import dev.facetilt.engine.DecisionListener
import dev.facetilt.engine.EngineConfig
import dev.facetilt.engine.EngineStats
import dev.facetilt.engine.NativeBridge
import dev.facetilt.rotation.OverlayApplier
import dev.facetilt.rotation.RotationApplier
import dev.facetilt.rotation.SystemSettingApplier
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors

/**
 * Foreground service of type `camera`. Runs only while the user has FaceTilt enabled.
 *
 * The native engine (sensors + camera) runs only while the screen is on and unlocked, the
 * user is in the foreground, and no external presentation display is connected. While the
 * screen is off this service does nothing at all: no sensors, no camera, no wake lock, no timer.
 *
 * Why the service is not stopped on screen-off: Android 12+ forbids starting a foreground
 * service from the background, and Android 14+ additionally requires a camera-type service to
 * be started while the app is visible. A service stopped at screen-off could therefore not be
 * restarted at screen-on without the user opening the app every time.
 */
class FaceTiltService : LifecycleService(), DecisionListener {

    enum class Status { Off, Active, WaitingForScreen, PausedExternalDisplay, PausedOtherUser, NoCameraPermission, EngineUnavailable }

    companion object {
        const val ACTION_PAUSE = "dev.facetilt.action.PAUSE"
        private const val CHANNEL_ID = "facetilt_status"
        private const val NOTIF_ID = 1

        private val _status = MutableStateFlow(Status.Off)
        val status: StateFlow<Status> = _status
        val isRunning: Boolean get() = _status.value != Status.Off

        fun ensureChannel(context: Context) {
            val nm = context.getSystemService(NotificationManager::class.java)
            val ch = NotificationChannel(CHANNEL_ID, context.getString(R.string.notif_channel_name), NotificationManager.IMPORTANCE_MIN)
            ch.description = context.getString(R.string.notif_channel_desc)
            ch.setShowBadge(false)
            nm.createNotificationChannel(ch)
        }
    }

    private val main = Handler(Looper.getMainLooper())
    /** Native start/stop block (thread joins); keep them off the main thread, strictly ordered. */
    private val worker: ExecutorService = Executors.newSingleThreadExecutor { r -> Thread(r, "facetilt-ctl") }

    private lateinit var appSettings: AppSettings
    private var initialized = false
    private var applier: RotationApplier? = null
    private var userForeground = true
    private var receiversRegistered = false

    // Accessed only on `worker`.
    private var nativeRunning = false
    private var countedOpens = 0L
    private var orientationOverride = -1

    // ------------------------------------------------------------------ lifecycle

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        if (intent?.action == ACTION_PAUSE) {
            val repo = (application as FaceTiltApp).settings
            lifecycleScope.launch {
                repo.setEnabled(false)
                stopSelf()
            }
            return START_NOT_STICKY
        }
        if (!startInForeground()) {
            stopSelf()
            return START_NOT_STICKY
        }
        if (!initialized) {
            initialized = true
            lifecycleScope.launch {
                appSettings = (application as FaceTiltApp).settings.current()
                setup()
            }
        }
        // Not sticky: if the system kills us, FaceTilt stays off until the user turns it on
        // (a camera-type service cannot legally restart itself from the background anyway).
        return START_NOT_STICKY
    }

    private fun startInForeground(): Boolean {
        ensureChannel(this)
        return try {
            // The `camera` service type exists from Android 11 (API 30), together with the
            // while-in-use camera restriction it relaxes. On Android 10 the manifest attribute
            // is unknown and passing the type would throw; no type is needed there.
            val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) ServiceInfo.FOREGROUND_SERVICE_TYPE_CAMERA else 0
            ServiceCompat.startForeground(this, NOTIF_ID, buildNotification(), type)
            true
        } catch (e: SecurityException) {
            // Android 14+: missing CAMERA permission or not started while the app was visible.
            false
        } catch (e: IllegalStateException) {
            // Android 12+: ForegroundServiceStartNotAllowedException (started from background).
            false
        }
    }

    private fun setup() {
        applier = when (appSettings.applierMode) {
            ApplierMode.SystemSetting -> SystemSettingApplier(this) { a, u ->
                lifecycleScope.launch { (application as FaceTiltApp).settings.saveOriginalRotation(a, u) }
            }
            ApplierMode.Overlay -> OverlayApplier(this)
        }
        if (applier?.activate(currentRotation(), takeControl = !currentSettings().assistWhenLying) != true) {
            // The rotation permission was revoked: nothing useful can be done.
            applier = null
            stopSelf()
            return
        }
        registerListeners()
        reevaluate()
        // Apply engine settings changed in the app (threshold, upside-down, 2nd verification)
        // without restarting the service.
        lifecycleScope.launch {
            (application as FaceTiltApp).settings.settings.collect { s ->
                if (s == appSettings) return@collect
                val assistChanged = s.assistWhenLying != appSettings.assistWhenLying
                appSettings = s
                // A rotation-method change restarts the whole service from the UI, so it is not
                // handled here (that would spawn a second settings collector).
                if (assistChanged) {
                    // Reset the applier's control baseline: normal mode holds control
                    // continuously, assist mode leaves the system's auto-rotate on until a tilt.
                    applier?.release()
                    applier?.activate(currentRotation(), takeControl = !s.assistWhenLying)
                }
                val cfg = engineConfig()
                worker.execute { if (nativeRunning) NativeBridge.nativeSetConfig(cfg.copy(sensorOrientationOverride = orientationOverride).toArray()) }
            }
        }
    }

    override fun onDestroy() {
        unregisterListeners()
        worker.execute { stopNative() }
        worker.shutdown()
        applier?.release()
        applier = null
        if (currentSettings().applierMode == ApplierMode.SystemSetting) {
            val repo = (application as FaceTiltApp).settings
            FaceTiltApp.appScope.launch { repo.clearOriginalRotation() }
        }
        _status.value = Status.Off
        super.onDestroy()
    }

    private fun currentSettings(): AppSettings = if (::appSettings.isInitialized) appSettings else AppSettings()

    // ------------------------------------------------------------------ run conditions

    private val screenReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.action) {
                Intent.ACTION_USER_BACKGROUND -> userForeground = false
                Intent.ACTION_USER_FOREGROUND -> userForeground = true
            }
            reevaluate()
        }
    }

    private val displayListener = object : DisplayManager.DisplayListener {
        override fun onDisplayAdded(displayId: Int) = reevaluate()
        override fun onDisplayRemoved(displayId: Int) = reevaluate()
        override fun onDisplayChanged(displayId: Int) {
            if (displayId != Display.DEFAULT_DISPLAY) return // never act on external displays
            val r = currentRotation()
            worker.execute { if (nativeRunning) NativeBridge.nativeSetDisplayRotation(r) }
        }
    }

    private val autoRotateObserver = object : ContentObserver(main) {
        override fun onChange(selfChange: Boolean) {
            // SystemSetting mode keeps ACCELEROMETER_ROTATION at 0. If the user switches the
            // system auto-rotate back on, they want the standard behaviour: step aside.
            val v = Settings.System.getInt(contentResolver, Settings.System.ACCELEROMETER_ROTATION, 0)
            if (v == 1) {
                val repo = (application as FaceTiltApp).settings
                lifecycleScope.launch {
                    repo.setEnabled(false)
                    stopSelf()
                }
            }
        }
    }

    private fun registerListeners() {
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_ON)
            addAction(Intent.ACTION_SCREEN_OFF)
            addAction(Intent.ACTION_USER_PRESENT)
            addAction(Intent.ACTION_USER_BACKGROUND)
            addAction(Intent.ACTION_USER_FOREGROUND)
        }
        // System broadcasts are delivered to non-exported receivers.
        ContextCompat.registerReceiver(this, screenReceiver, filter, ContextCompat.RECEIVER_NOT_EXPORTED)
        getSystemService(DisplayManager::class.java).registerDisplayListener(displayListener, main)
        if (currentSettings().applierMode == ApplierMode.SystemSetting) {
            contentResolver.registerContentObserver(Settings.System.getUriFor(Settings.System.ACCELEROMETER_ROTATION), false, autoRotateObserver)
        }
        receiversRegistered = true
    }

    private fun unregisterListeners() {
        if (!receiversRegistered) return
        receiversRegistered = false
        unregisterReceiver(screenReceiver)
        getSystemService(DisplayManager::class.java).unregisterDisplayListener(displayListener)
        contentResolver.unregisterContentObserver(autoRotateObserver)
    }

    private fun hasCamera(): Boolean =
        ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED

    private fun externalDisplayConnected(): Boolean =
        getSystemService(DisplayManager::class.java).getDisplays(DisplayManager.DISPLAY_CATEGORY_PRESENTATION).isNotEmpty()

    private fun screenUsable(): Boolean {
        val interactive = getSystemService(PowerManager::class.java).isInteractive
        val locked = getSystemService(KeyguardManager::class.java).isKeyguardLocked
        return interactive && !locked
    }

    private fun desiredStatus(): Status = when {
        !hasCamera() -> Status.NoCameraPermission
        !userForeground -> Status.PausedOtherUser
        externalDisplayConnected() -> Status.PausedExternalDisplay
        !screenUsable() -> Status.WaitingForScreen
        else -> Status.Active
    }

    private fun reevaluate() {
        if (applier == null) return
        val want = desiredStatus()
        val cfg = engineConfig()
        val rotation = currentRotation()
        worker.execute {
            if (want == Status.Active && !nativeRunning) {
                if (!startNative(cfg, rotation)) {
                    _status.value = Status.EngineUnavailable
                    return@execute
                }
            } else if (want != Status.Active && nativeRunning) {
                stopNative()
            }
            _status.value = want
        }
    }

    // ------------------------------------------------------------------ native engine (worker thread)

    private fun engineConfig() = EngineConfig(
        confidenceThreshold = currentSettings().confidenceThreshold,
        allowUpsideDown = currentSettings().allowUpsideDown,
        debugTrace = BuildConfig.DEBUG,
        secondVerification = currentSettings().secondVerification,
        assistMode = currentSettings().assistWhenLying,
    )

    private fun startNative(cfg: EngineConfig, rotation: Int): Boolean {
        if (!NativeBridge.ensureLoaded()) return false
        if (!NativeBridge.nativeStart(this, packageName, rotation, cfg.toArray())) return false
        nativeRunning = true
        countedOpens = 0
        applyCameraOrientation(cfg)
        return true
    }

    private fun stopNative() {
        if (!nativeRunning) return
        NativeBridge.nativeStop()
        nativeRunning = false
        countActivations()
    }

    /**
     * Android 12L+ foldables can change a camera's effective SENSOR_ORIENTATION with the device
     * state; the Java CameraCharacteristics API applies that mapping (API 32+), the NDK does not.
     * On older versions both report the same static value, so this is a no-op there.
     */
    private fun applyCameraOrientation(cfg: EngineConfig) {
        val id = NativeBridge.nativeFrontCameraId() ?: return
        val orientation = try {
            getSystemService(CameraManager::class.java).getCameraCharacteristics(id).get(CameraCharacteristics.SENSOR_ORIENTATION)
        } catch (e: CameraAccessException) {
            null
        } catch (e: IllegalArgumentException) {
            null
        } ?: return
        orientationOverride = orientation
        NativeBridge.nativeSetConfig(cfg.copy(sensorOrientationOverride = orientation).toArray())
    }

    private fun countActivations() {
        val stats = EngineStats.fromArray(NativeBridge.nativeGetStats()) ?: return
        val delta = stats.cameraOpens - countedOpens
        if (delta > 0) {
            countedOpens = stats.cameraOpens
            val repo = (application as FaceTiltApp).settings
            FaceTiltApp.appScope.launch { repo.addActivations(delta.toInt()) }
        }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        // Fold / unfold: refresh the camera orientation and the run conditions.
        val cfg = engineConfig()
        worker.execute { if (nativeRunning) applyCameraOrientation(cfg) }
        reevaluate()
    }

    // ------------------------------------------------------------------ decisions

    /** Called on the native engine thread. */
    override fun onOrientationDecision(rotation: Int, reason: Int) {
        main.post {
            val t0 = SystemClock.elapsedRealtimeNanos()
            applier?.apply(rotation)
            val us = (SystemClock.elapsedRealtimeNanos() - t0) / 1000
            NativeBridge.nativeReportApplied(us)
            worker.execute { if (nativeRunning) countActivations() }
        }
    }

    /**
     * Assist mode only (called on the native engine thread): [seize] true = take rotation
     * control from the system before it can flip; false = hand the system's auto-rotate back.
     */
    override fun onAssistControl(seize: Boolean) {
        main.post {
            if (seize) applier?.seizeControl(currentRotation()) else applier?.releaseControl()
        }
    }

    private fun currentRotation(): Int =
        getSystemService(DisplayManager::class.java).getDisplay(Display.DEFAULT_DISPLAY)?.rotation ?: 0

    // ------------------------------------------------------------------ notification

    private fun buildNotification(): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val pause = PendingIntent.getService(
            this, 1, Intent(this, FaceTiltService::class.java).setAction(ACTION_PAUSE),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_tile)
            .setContentTitle(getString(R.string.notif_title))
            .setContentText(getString(R.string.notif_text))
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setOngoing(true)
            .setSilent(true)
            .setShowWhen(false)
            .setContentIntent(open)
            .addAction(0, getString(R.string.notif_pause), pause)
            .setForegroundServiceBehavior(
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE
                else NotificationCompat.FOREGROUND_SERVICE_DEFAULT,
            )
            .build()
    }
}

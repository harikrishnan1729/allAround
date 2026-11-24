package dev.facetilt.ui

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.ContextCompat
import androidx.core.net.toUri
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.facetilt.boot.BootReceiver
import dev.facetilt.data.ApplierMode
import dev.facetilt.data.SettingsRepository
import dev.facetilt.service.FaceTiltService
import dev.facetilt.service.ServiceController
import kotlinx.coroutines.launch

enum class Screen { Home, Privacy, Settings, Debug }

private fun granted(ctx: Context, perm: String) =
    ContextCompat.checkSelfPermission(ctx, perm) == PackageManager.PERMISSION_GRANTED

@Composable
fun FaceTiltRoot(
    repo: SettingsRepository,
    enableRequested: Boolean,
    onEnableRequestHandled: (finishActivity: Boolean) -> Unit,
) {
    val context = LocalContext.current
    val activity = context as? Activity
    val scope = rememberCoroutineScope()
    val settings by repo.settings.collectAsStateWithLifecycle(initialValue = null)
    val status by FaceTiltService.status.collectAsStateWithLifecycle()

    var resumeTick by remember { mutableIntStateOf(0) }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { resumeTick++ }

    var cameraAnswer by rememberSaveable { mutableStateOf(CameraPermission.NotAsked) }
    var notificationsDecided by rememberSaveable { mutableStateOf(false) }
    var setupActive by rememberSaveable { mutableStateOf(false) }
    var consentDismissed by rememberSaveable { mutableStateOf(false) }
    var screen by rememberSaveable { mutableStateOf(Screen.Home) }
    var message by remember { mutableStateOf<String?>(null) }

    val cameraLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        val rationale = activity?.shouldShowRequestPermissionRationale(Manifest.permission.CAMERA) ?: false
        cameraAnswer = PermissionFlow.afterCameraResult(ok, rationale)
    }
    val notificationLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        notificationsDecided = true
    }

    LaunchedEffect(enableRequested) {
        if (enableRequested) setupActive = true
    }

    val s = settings
    if (s == null) {
        Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
        return
    }

    // Re-read permissions whenever we come back from a system settings screen (resumeTick).
    val permState = remember(s, resumeTick, cameraAnswer, notificationsDecided) {
        PermissionState(
        consent = s.consentGiven,
        camera = if (granted(context, Manifest.permission.CAMERA)) CameraPermission.Granted else cameraAnswer,
        notificationsRequired = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU,
        notificationsDecided = notificationsDecided ||
            (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && granted(context, Manifest.permission.POST_NOTIFICATIONS)),
        applierMode = s.applierMode,
        canWriteSettings = Settings.System.canWrite(context),
        canDrawOverlays = Settings.canDrawOverlays(context),
        )
    }

    fun cancelSetup() {
        setupActive = false
        if (enableRequested) onEnableRequestHandled(false)
    }

    // ---------------------------------------------------------------- onboarding / consent
    if (!s.consentGiven && (setupActive || !consentDismissed)) {
        ConsentScreen(
            onAccept = { scope.launch { repo.setConsent(true) } },
            onDecline = {
                consentDismissed = true
                cancelSetup()
            },
        )
        return
    }

    // ---------------------------------------------------------------- enable flow
    if (setupActive) {
        BackHandler { cancelSetup() }
        when (PermissionFlow.next(permState)) {
            SetupStep.Consent -> Unit // handled above
            SetupStep.CameraRationale -> CameraRationaleScreen(
                onContinue = { cameraLauncher.launch(Manifest.permission.CAMERA) },
                onCancel = ::cancelSetup,
            )
            SetupStep.CameraDenied -> CameraDeniedScreen(
                permanentlyDenied = permState.camera == CameraPermission.PermanentlyDenied,
                onRetry = { cameraLauncher.launch(Manifest.permission.CAMERA) },
                onOpenSettings = { openAppSettings(context) },
                onCancel = ::cancelSetup,
            )
            SetupStep.NotificationRationale -> NotificationRationaleScreen(
                onAllow = {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                        notificationLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                    } else {
                        notificationsDecided = true
                    }
                },
                onSkip = { notificationsDecided = true },
            )
            SetupStep.RotationPermission -> RotationPermissionScreen(
                mode = s.applierMode,
                onGrant = { openRotationPermission(context, s.applierMode) },
                onSwitchMode = {
                    val other = if (s.applierMode == ApplierMode.SystemSetting) ApplierMode.Overlay else ApplierMode.SystemSetting
                    scope.launch { repo.setApplierMode(other) }
                },
                onCancel = ::cancelSetup,
            )
            SetupStep.Ready -> LaunchedEffect(Unit) {
                repo.setEnabled(true)
                val result = ServiceController.start(context)
                message = when (result) {
                    ServiceController.StartResult.Started -> null
                    ServiceController.StartResult.MissingPermission -> "Couldn't start. Check the camera permission."
                    ServiceController.StartResult.NotAllowedFromBackground -> "Open the app and try again."
                }
                if (result != ServiceController.StartResult.Started) repo.setEnabled(false)
                setupActive = false
                if (enableRequested) onEnableRequestHandled(result == ServiceController.StartResult.Started)
            }
        }
        return
    }

    // ---------------------------------------------------------------- main screens
    if (screen != Screen.Home) BackHandler { screen = Screen.Home }
    when (screen) {
        Screen.Home -> HomeScreen(
            settings = s,
            status = status,
            message = message,
            onToggle = { on ->
                message = null
                if (on) {
                    setupActive = true
                } else {
                    scope.launch { repo.setEnabled(false) }
                    ServiceController.stop(context)
                }
            },
            onOpen = { screen = it },
        )
        Screen.Privacy -> PrivacyScreen(
            settings = s,
            permState = permState,
            onBack = { screen = Screen.Home },
            onDeleteAll = {
                ServiceController.stop(context)
                BootReceiver.setEnabled(context, false)
                scope.launch { repo.deleteAll() }
                cameraAnswer = CameraPermission.NotAsked
                notificationsDecided = false
                consentDismissed = false
                screen = Screen.Home
            },
        )
        Screen.Settings -> SettingsScreen(
            settings = s,
            onBack = { screen = Screen.Home },
            onApplierMode = { m ->
                scope.launch {
                    repo.setApplierMode(m)
                    if (FaceTiltService.isRunning) {
                        // Re-start so the new applier takes over (we are visible: allowed).
                        ServiceController.stop(context)
                        setupActive = true
                    }
                }
            },
            onThreshold = { v -> scope.launch { repo.setConfidenceThreshold(v) } },
            onUpsideDown = { v -> scope.launch { repo.setAllowUpsideDown(v) } },
            onSecondVerification = { v -> scope.launch { repo.setSecondVerification(v) } },
            onAssistWhenLying = { v -> scope.launch { repo.setAssistWhenLying(v) } },
            onStartOnBoot = { v ->
                BootReceiver.setEnabled(context, v)
                scope.launch { repo.setStartOnBoot(v) }
            },
        )
        Screen.Debug -> DebugScreen(onBack = { screen = Screen.Home })
    }
}

private fun openAppSettings(context: Context) {
    context.startActivity(
        Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", context.packageName, null))
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
    )
}

private fun openRotationPermission(context: Context, mode: ApplierMode) {
    val action = when (mode) {
        ApplierMode.SystemSetting -> Settings.ACTION_MANAGE_WRITE_SETTINGS
        ApplierMode.Overlay -> Settings.ACTION_MANAGE_OVERLAY_PERMISSION
    }
    context.startActivity(Intent(action, "package:${context.packageName}".toUri()).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
}

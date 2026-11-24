package dev.facetilt.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import dev.facetilt.BuildConfig
import dev.facetilt.data.ApplierMode
import dev.facetilt.data.Settings
import dev.facetilt.engine.DecisionReason
import dev.facetilt.engine.EngineStats
import dev.facetilt.engine.NativeBridge
import dev.facetilt.service.FaceTiltService
import java.time.LocalDate
import java.util.Locale

@Composable
private fun Page(title: String, tag: String, onBack: (() -> Unit)? = null, content: @Composable () -> Unit) {
    Column(
        Modifier
            .fillMaxSize()
            .safeDrawingPadding()
            .verticalScroll(rememberScrollState())
            .padding(20.dp)
            .testTag(tag),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (onBack != null) TextButton(onClick = onBack) { Text("‹ Back") }
            Text(title, style = MaterialTheme.typography.headlineSmall)
        }
        content()
    }
}

@Composable
private fun Para(text: String) = Text(text, style = MaterialTheme.typography.bodyLarge)

@Composable
private fun Bullet(text: String) = Text("•  $text", style = MaterialTheme.typography.bodyMedium)

// ------------------------------------------------------------------------------- onboarding

@Composable
fun ConsentScreen(onAccept: () -> Unit, onDecline: () -> Unit) = Page("Before you start", "consent") {
    Para("Keeps your screen upright for your face, so lying on your side doesn't flip it sideways.")
    Bullet("The front camera opens for a split second when you rotate the phone. Android's camera indicator will show.")
    Bullet("Only a tiny black-and-white image is used, for milliseconds, then erased. Nothing is saved or sent, and the app has no internet access.")
    Bullet("It can't recognise who you are. It's off until you turn it on.")
    Button(onClick = onAccept, modifier = Modifier.fillMaxWidth().testTag("consent_accept")) { Text("Continue") }
    OutlinedButton(onClick = onDecline, modifier = Modifier.fillMaxWidth().testTag("consent_decline")) { Text("Not now") }
}

@Composable
fun CameraRationaleScreen(onContinue: () -> Unit, onCancel: () -> Unit) = Page("Camera access", "camera_rationale") {
    Para("Needed to see which way your head is turned when you rotate the phone. Used for a split second, never continuously.")
    Button(onClick = onContinue, modifier = Modifier.fillMaxWidth().testTag("camera_request")) { Text("Continue") }
    OutlinedButton(onClick = onCancel, modifier = Modifier.fillMaxWidth()) { Text("Not now") }
}

@Composable
fun CameraDeniedScreen(permanentlyDenied: Boolean, onRetry: () -> Unit, onOpenSettings: () -> Unit, onCancel: () -> Unit) =
    Page("Camera not allowed", "camera_denied") {
        Para("Without the camera this stays off and your phone uses normal auto-rotate.")
        if (permanentlyDenied) {
            Button(onClick = onOpenSettings, modifier = Modifier.fillMaxWidth().testTag("camera_open_settings")) { Text("Open app settings") }
        } else {
            Button(onClick = onRetry, modifier = Modifier.fillMaxWidth().testTag("camera_retry")) { Text("Try again") }
        }
        OutlinedButton(onClick = onCancel, modifier = Modifier.fillMaxWidth().testTag("camera_denied_cancel")) { Text("Keep it off") }
    }

@Composable
fun NotificationRationaleScreen(onAllow: () -> Unit, onSkip: () -> Unit) = Page("Notification", "notification_rationale") {
    Para("Android shows a small, silent notification while the camera may be used. It has a Pause button.")
    Button(onClick = onAllow, modifier = Modifier.fillMaxWidth().testTag("notification_allow")) { Text("Allow") }
    OutlinedButton(onClick = onSkip, modifier = Modifier.fillMaxWidth().testTag("notification_skip")) { Text("Skip") }
}

@Composable
fun RotationPermissionScreen(mode: ApplierMode, onGrant: () -> Unit, onSwitchMode: () -> Unit, onCancel: () -> Unit) =
    Page("One more permission", "rotation_permission") {
        Para(
            when (mode) {
                ApplierMode.SystemSetting -> "To rotate the screen, allow \"Modify system settings\"."
                ApplierMode.Overlay -> "To rotate the screen, allow \"Display over other apps\"."
            },
        )
        Button(onClick = onGrant, modifier = Modifier.fillMaxWidth().testTag("rotation_grant")) { Text("Continue") }
        TextButton(onClick = onSwitchMode, modifier = Modifier.fillMaxWidth()) {
            Text(if (mode == ApplierMode.SystemSetting) "Use overlay method instead" else "Use system setting instead")
        }
        TextButton(onClick = onCancel, modifier = Modifier.fillMaxWidth()) { Text("Not now") }
    }

// ------------------------------------------------------------------------------- main

private fun statusText(enabled: Boolean, status: FaceTiltService.Status): String = when (status) {
    FaceTiltService.Status.Active, FaceTiltService.Status.WaitingForScreen -> "On"
    FaceTiltService.Status.PausedExternalDisplay -> "Paused: external display"
    FaceTiltService.Status.PausedOtherUser -> "Paused: another user"
    FaceTiltService.Status.NoCameraPermission -> "Camera permission needed"
    FaceTiltService.Status.EngineUnavailable -> "Not supported on this device"
    FaceTiltService.Status.Off -> if (enabled) "Starting\u2026" else "Off"
}

@Composable
fun HomeScreen(
    settings: Settings,
    status: FaceTiltService.Status,
    message: String?,
    onToggle: (Boolean) -> Unit,
    onOpen: (Screen) -> Unit,
) = Page("allAround", "home") {
    Card(Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text("Face detection", style = MaterialTheme.typography.titleMedium)
                Text(statusText(settings.enabled, status), style = MaterialTheme.typography.bodyMedium)
            }
            Switch(
                checked = settings.enabled && status != FaceTiltService.Status.Off,
                onCheckedChange = onToggle,
                modifier = Modifier.testTag("master_switch"),
            )
        }
    }
    if (message != null) Text(message, color = MaterialTheme.colorScheme.error)
    Para("Rotates the screen only when your head turns with the phone.")
    OutlinedButton(onClick = { onOpen(Screen.Settings) }, modifier = Modifier.fillMaxWidth()) { Text("Settings") }
    OutlinedButton(onClick = { onOpen(Screen.Privacy) }, modifier = Modifier.fillMaxWidth().testTag("open_privacy")) { Text("Privacy") }
    if (BuildConfig.DEBUG) {
        TextButton(onClick = { onOpen(Screen.Debug) }, modifier = Modifier.fillMaxWidth()) { Text("Debug") }
    }
}

@Composable
fun PrivacyScreen(settings: Settings, permState: PermissionState, onBack: () -> Unit, onDeleteAll: () -> Unit) =
    Page("Privacy", "privacy", onBack) {
        val ctx = LocalContext.current
        fun g(p: String) = ContextCompat.checkSelfPermission(ctx, p) == PackageManager.PERMISSION_GRANTED
        fun yn(b: Boolean) = if (b) "allowed" else "not allowed"
        Para("Camera frames are shrunk to a tiny black-and-white image in memory, used once, then erased. Nothing is saved or sent, and the app has no internet permission.")
        Bullet("Camera: ${yn(g(Manifest.permission.CAMERA))}")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            Bullet("Notifications: ${yn(g(Manifest.permission.POST_NOTIFICATIONS))}")
        }
        Bullet("Modify system settings: ${yn(permState.canWriteSettings)}")
        Bullet("Display over other apps: ${yn(permState.canDrawOverlays)}")
        Text("Camera uses today: ${settings.activations.countFor(LocalDate.now())}", modifier = Modifier.testTag("activations_today"))
        var confirm by remember { mutableStateOf(false) }
        Button(onClick = { confirm = true }, modifier = Modifier.fillMaxWidth().testTag("delete_all")) { Text("Delete all data") }
        if (confirm) {
            AlertDialog(
                onDismissRequest = { confirm = false },
                title = { Text("Delete all data?") },
                text = { Text("This turns face detection off and erases your settings.") },
                confirmButton = { TextButton(onClick = { confirm = false; onDeleteAll() }) { Text("Delete") } },
                dismissButton = { TextButton(onClick = { confirm = false }) { Text("Cancel") } },
            )
        }
    }

@Composable
private fun ToggleRow(title: String, subtitle: String?, checked: Boolean, tag: String?, onChange: (Boolean) -> Unit) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodySmall)
        }
        Switch(checked = checked, onCheckedChange = onChange, modifier = if (tag != null) Modifier.testTag(tag) else Modifier)
    }
}

@Composable
fun SettingsScreen(
    settings: Settings,
    onBack: () -> Unit,
    onApplierMode: (ApplierMode) -> Unit,
    onThreshold: (Float) -> Unit,
    onUpsideDown: (Boolean) -> Unit,
    onStartOnBoot: (Boolean) -> Unit,
    onSecondVerification: (Boolean) -> Unit,
    onAssistWhenLying: (Boolean) -> Unit,
) = Page("Settings", "settings", onBack) {
    ToggleRow(
        "Only intervene when lying down",
        "Normal auto-rotate stays on. This only holds portrait when you're lying on your side.",
        settings.assistWhenLying, "assist_when_lying", onAssistWhenLying,
    )
    ToggleRow(
        "2nd verification",
        "After turning to landscape, checks once more and switches back if you're lying down.",
        settings.secondVerification, "second_verification", onSecondVerification,
    )
    ToggleRow("Allow upside-down", null, settings.allowUpsideDown, null, onUpsideDown)

    var advanced by remember { mutableStateOf(false) }
    TextButton(onClick = { advanced = !advanced }) { Text(if (advanced) "Advanced \u25B4" else "Advanced \u25BE") }
    if (advanced) {
        Text("Rotation method", style = MaterialTheme.typography.titleSmall)
        for ((mode, label) in listOf(
            ApplierMode.SystemSetting to "System setting (recommended)",
            ApplierMode.Overlay to "Overlay window",
        )) {
            Row(
                Modifier.fillMaxWidth().selectable(selected = settings.applierMode == mode, onClick = { onApplierMode(mode) }),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = settings.applierMode == mode, onClick = { onApplierMode(mode) })
                Text(label)
            }
        }
        var threshold by remember(settings.confidenceThreshold) { mutableFloatStateOf(settings.confidenceThreshold) }
        Text("Face confidence: ${String.format(Locale.US, "%.2f", threshold)}", style = MaterialTheme.typography.titleSmall)
        Slider(value = threshold, onValueChange = { threshold = it }, onValueChangeFinished = { onThreshold(threshold) }, valueRange = 0.5f..0.95f)
        ToggleRow("Start after reboot", "Shows a notification to tap after restarting.", settings.startOnBoot, null, onStartOnBoot)
    }
}

@Composable
fun DebugScreen(onBack: () -> Unit) = Page("Debug: latency & battery", "debug", onBack) {
    var stats by remember { mutableStateOf<EngineStats?>(null) }
    fun refresh() {
        stats = if (NativeBridge.ensureLoaded()) EngineStats.fromArray(NativeBridge.nativeGetStats()) else null
    }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { refresh() }
    Button(onClick = ::refresh, modifier = Modifier.fillMaxWidth()) { Text("Refresh") }
    val s = stats
    if (s == null) {
        Para("Native engine not loaded yet. Turn face detection on and rotate the phone.")
        return@Page
    }
    fun ms(v: Long) = if (v < 0) "–" else "$v ms"
    fun us(v: Long) = if (v < 0) "–" else String.format(Locale.US, "%.2f ms", v / 1000.0)
    Text("Last activation (from tilt onset)", fontWeight = FontWeight.Bold)
    Bullet("camera open + session ready: ${ms(s.lastOpenMs)}")
    Bullet("classified frame: ${ms(s.lastFrameMs)}")
    Bullet("inference: ${us(s.lastInferenceUs)}")
    Bullet("decision: ${ms(s.lastDecisionMs)}  (target < 250 ms)")
    Bullet("apply (Kotlin): ${us(s.lastApplyUs)}")
    Bullet("reason: ${DecisionReason.fromCode(s.lastReason.toInt())?.label ?: "–"}")
    Text("Counters since the engine started (${s.uptimeMs / 1000} s)", fontWeight = FontWeight.Bold)
    Bullet(String.format(Locale.US, "camera-on: %.2f s per hour (%d ms total)", s.cameraOnSecondsPerHour, s.cameraOnMs))
    Bullet(String.format(Locale.US, "activations: %.1f per hour (%d opens)", s.activationsPerHour, s.cameraOpens))
    Bullet("inference p50 / p95: ${us(s.inferenceP50Us)} / ${us(s.inferenceP95Us)}")
    Bullet("evaluations ${s.evaluations}, changed ${s.decisionsChanged}, kept by face ${s.decisionsKeptByFace}, fallbacks ${s.fallbacks}, aborted ${s.aborts}")
    Bullet("2nd verifications ${s.verifications}, switched back to portrait ${s.reverts}")
    Text("Engine state survives only while the screen is on; counters reset when it restarts.", style = MaterialTheme.typography.bodySmall)
}

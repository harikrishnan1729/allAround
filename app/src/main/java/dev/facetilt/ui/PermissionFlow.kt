package dev.facetilt.ui

import dev.facetilt.data.ApplierMode

enum class CameraPermission { Granted, NotAsked, Denied, PermanentlyDenied }

/** Snapshot of everything the enable flow depends on. */
data class PermissionState(
    val consent: Boolean,
    val camera: CameraPermission,
    /** Android 13+ only; always false below. */
    val notificationsRequired: Boolean,
    /** Granted, or the user answered (allow or skip). */
    val notificationsDecided: Boolean,
    val applierMode: ApplierMode,
    val canWriteSettings: Boolean,
    val canDrawOverlays: Boolean,
)

enum class SetupStep { Consent, CameraRationale, CameraDenied, NotificationRationale, RotationPermission, Ready }

/**
 * Which screen the enable flow shows next. Pure function (unit-tested). Every step has a way
 * out: declining anything leaves FaceTilt off and the phone on normal auto-rotate.
 */
object PermissionFlow {
    fun next(s: PermissionState): SetupStep = when {
        !s.consent -> SetupStep.Consent
        s.camera == CameraPermission.NotAsked -> SetupStep.CameraRationale
        s.camera == CameraPermission.Denied || s.camera == CameraPermission.PermanentlyDenied -> SetupStep.CameraDenied
        s.notificationsRequired && !s.notificationsDecided -> SetupStep.NotificationRationale
        s.applierMode == ApplierMode.SystemSetting && !s.canWriteSettings -> SetupStep.RotationPermission
        s.applierMode == ApplierMode.Overlay && !s.canDrawOverlays -> SetupStep.RotationPermission
        else -> SetupStep.Ready
    }

    /**
     * Interprets a camera permission result. Android cannot tell "never asked" from
     * "permanently denied" up front; after a request, `shouldShowRationale == false` with a
     * denial means the system will not show the dialog again.
     */
    fun afterCameraResult(granted: Boolean, shouldShowRationale: Boolean): CameraPermission = when {
        granted -> CameraPermission.Granted
        shouldShowRationale -> CameraPermission.Denied
        else -> CameraPermission.PermanentlyDenied
    }
}

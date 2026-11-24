package dev.facetilt

import dev.facetilt.data.ApplierMode
import dev.facetilt.ui.CameraPermission
import dev.facetilt.ui.PermissionFlow
import dev.facetilt.ui.PermissionState
import dev.facetilt.ui.SetupStep
import org.junit.Assert.assertEquals
import org.junit.Test

class PermissionFlowTest {
    private val base = PermissionState(
        consent = true,
        camera = CameraPermission.Granted,
        notificationsRequired = true,
        notificationsDecided = true,
        applierMode = ApplierMode.SystemSetting,
        canWriteSettings = true,
        canDrawOverlays = false,
    )

    @Test fun consentComesFirst() =
        assertEquals(SetupStep.Consent, PermissionFlow.next(base.copy(consent = false, camera = CameraPermission.NotAsked)))

    @Test fun cameraRationaleBeforeRequest() =
        assertEquals(SetupStep.CameraRationale, PermissionFlow.next(base.copy(camera = CameraPermission.NotAsked)))

    @Test fun deniedCameraShowsGracefulScreen() {
        assertEquals(SetupStep.CameraDenied, PermissionFlow.next(base.copy(camera = CameraPermission.Denied)))
        assertEquals(SetupStep.CameraDenied, PermissionFlow.next(base.copy(camera = CameraPermission.PermanentlyDenied)))
    }

    @Test fun notificationsOnlyOnAndroid13Plus() {
        assertEquals(SetupStep.NotificationRationale, PermissionFlow.next(base.copy(notificationsDecided = false)))
        assertEquals(SetupStep.Ready, PermissionFlow.next(base.copy(notificationsRequired = false, notificationsDecided = false)))
    }

    @Test fun rotationPermissionDependsOnMode() {
        assertEquals(SetupStep.RotationPermission, PermissionFlow.next(base.copy(canWriteSettings = false)))
        assertEquals(SetupStep.RotationPermission, PermissionFlow.next(base.copy(applierMode = ApplierMode.Overlay)))
        assertEquals(SetupStep.Ready, PermissionFlow.next(base.copy(applierMode = ApplierMode.Overlay, canDrawOverlays = true, canWriteSettings = false)))
    }

    @Test fun readyWhenEverythingGranted() = assertEquals(SetupStep.Ready, PermissionFlow.next(base))

    @Test fun cameraResultInterpretation() {
        assertEquals(CameraPermission.Granted, PermissionFlow.afterCameraResult(true, false))
        assertEquals(CameraPermission.Denied, PermissionFlow.afterCameraResult(false, true))
        assertEquals(CameraPermission.PermanentlyDenied, PermissionFlow.afterCameraResult(false, false))
    }
}

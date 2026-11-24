package dev.facetilt.service

import android.content.Context
import android.content.Intent
import androidx.core.content.ContextCompat

/** Starting/stopping the camera-type foreground service, across Android versions. */
object ServiceController {
    sealed interface StartResult {
        data object Started : StartResult

        /** Android 12+: not allowed from the background; must be started while visible. */
        data object NotAllowedFromBackground : StartResult

        /** Android 14+: missing CAMERA / FOREGROUND_SERVICE_CAMERA eligibility. */
        data object MissingPermission : StartResult
    }

    /** Call only while an activity of ours is visible (UI or tile trampoline). */
    fun start(context: Context): StartResult = try {
        ContextCompat.startForegroundService(context, Intent(context, FaceTiltService::class.java))
        StartResult.Started
    } catch (e: SecurityException) {
        StartResult.MissingPermission
    } catch (e: IllegalStateException) {
        // API 31+: ForegroundServiceStartNotAllowedException (a subclass of IllegalStateException).
        // API 29-30: plain IllegalStateException when the app is in the background.
        StartResult.NotAllowedFromBackground
    }

    /** Stopping is always allowed. */
    fun stop(context: Context) {
        context.stopService(Intent(context, FaceTiltService::class.java))
    }
}

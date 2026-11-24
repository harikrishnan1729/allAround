package dev.facetilt

import android.app.Application
import dev.facetilt.data.SettingsRepository
import dev.facetilt.rotation.SystemSettingApplier
import dev.facetilt.service.FaceTiltService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

class FaceTiltApp : Application() {
    lateinit var settings: SettingsRepository
        private set

    override fun onCreate() {
        super.onCreate()
        settings = SettingsRepository(this)
        FaceTiltService.ensureChannel(this)
        // If a previous process died while FaceTilt had replaced the system auto-rotate
        // setting, give the user's original setting back.
        appScope.launch {
            val s = settings.current()
            if (!FaceTiltService.isRunning && s.savedAccelerometerRotation >= 0) {
                if (SystemSettingApplier.restoreSaved(this@FaceTiltApp, s.savedAccelerometerRotation, s.savedUserRotation)) {
                    settings.clearOriginalRotation()
                }
            }
        }
    }

    companion object {
        val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    }
}

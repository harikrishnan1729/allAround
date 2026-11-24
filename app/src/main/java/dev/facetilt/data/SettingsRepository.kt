package dev.facetilt.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.emptyPreferences
import androidx.datastore.preferences.core.floatPreferencesKey
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import java.time.LocalDate

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "facetilt_settings")

/** How the decided rotation is applied to the screen. */
enum class ApplierMode { SystemSetting, Overlay }

/**
 * Everything FaceTilt stores, all of it local (backup is disabled in the manifest):
 * consent flag, settings, a per-day activation counter, and the user's original rotation
 * settings while FaceTilt has replaced them. No image data, no identifiers.
 */
data class Settings(
    val consentGiven: Boolean = false,
    val enabled: Boolean = false,
    val applierMode: ApplierMode = ApplierMode.SystemSetting,
    val confidenceThreshold: Float = 0.7f,
    val allowUpsideDown: Boolean = false,
    val startOnBoot: Boolean = false,
    val secondVerification: Boolean = false,
    val assistWhenLying: Boolean = false,
    val activations: DailyCount = DailyCount("", 0),
    val savedAccelerometerRotation: Int = -1,
    val savedUserRotation: Int = -1,
)

class SettingsRepository(private val context: Context) {
    private object Keys {
        val consent = booleanPreferencesKey("consent_given")
        val enabled = booleanPreferencesKey("enabled")
        val applier = stringPreferencesKey("applier_mode")
        val threshold = floatPreferencesKey("confidence_threshold")
        val upsideDown = booleanPreferencesKey("allow_upside_down")
        val boot = booleanPreferencesKey("start_on_boot")
        val secondVerification = booleanPreferencesKey("second_verification")
        val assistWhenLying = booleanPreferencesKey("assist_when_lying")
        val actDay = stringPreferencesKey("activations_day")
        val actCount = intPreferencesKey("activations_count")
        val savedAccel = intPreferencesKey("saved_accelerometer_rotation")
        val savedUser = intPreferencesKey("saved_user_rotation")
    }

    val settings: Flow<Settings> = context.dataStore.data
        .catch { emit(emptyPreferences()) }
        .map { p ->
            Settings(
                consentGiven = p[Keys.consent] ?: false,
                enabled = p[Keys.enabled] ?: false,
                applierMode = p[Keys.applier]?.let { runCatching { ApplierMode.valueOf(it) }.getOrNull() } ?: ApplierMode.SystemSetting,
                confidenceThreshold = p[Keys.threshold] ?: 0.7f,
                allowUpsideDown = p[Keys.upsideDown] ?: false,
                startOnBoot = p[Keys.boot] ?: false,
                secondVerification = p[Keys.secondVerification] ?: false,
                assistWhenLying = p[Keys.assistWhenLying] ?: false,
                activations = DailyCount(p[Keys.actDay] ?: "", p[Keys.actCount] ?: 0),
                savedAccelerometerRotation = p[Keys.savedAccel] ?: -1,
                savedUserRotation = p[Keys.savedUser] ?: -1,
            )
        }

    suspend fun current(): Settings = settings.first()

    suspend fun setConsent(v: Boolean) = context.dataStore.edit { it[Keys.consent] = v }
    suspend fun setEnabled(v: Boolean) = context.dataStore.edit { it[Keys.enabled] = v }
    suspend fun setApplierMode(m: ApplierMode) = context.dataStore.edit { it[Keys.applier] = m.name }
    suspend fun setConfidenceThreshold(v: Float) = context.dataStore.edit { it[Keys.threshold] = v.coerceIn(0.5f, 0.99f) }
    suspend fun setAllowUpsideDown(v: Boolean) = context.dataStore.edit { it[Keys.upsideDown] = v }
    suspend fun setStartOnBoot(v: Boolean) = context.dataStore.edit { it[Keys.boot] = v }
    suspend fun setSecondVerification(v: Boolean) = context.dataStore.edit { it[Keys.secondVerification] = v }
    suspend fun setAssistWhenLying(v: Boolean) = context.dataStore.edit { it[Keys.assistWhenLying] = v }

    suspend fun saveOriginalRotation(accelerometer: Int, user: Int) = context.dataStore.edit {
        it[Keys.savedAccel] = accelerometer
        it[Keys.savedUser] = user
    }

    suspend fun clearOriginalRotation() = context.dataStore.edit {
        it.remove(Keys.savedAccel)
        it.remove(Keys.savedUser)
    }

    suspend fun addActivations(delta: Int, today: LocalDate = LocalDate.now()) {
        if (delta <= 0) return
        context.dataStore.edit {
            val next = DailyCount(it[Keys.actDay] ?: "", it[Keys.actCount] ?: 0).add(today, delta)
            it[Keys.actDay] = next.day
            it[Keys.actCount] = next.count
        }
    }

    /** "Delete all data": wipes every stored value. FaceTilt is off by default afterwards. */
    suspend fun deleteAll() = context.dataStore.edit { it.clear() }
}

/** Activation counter that resets each local day. Pure; unit-tested. */
data class DailyCount(val day: String, val count: Int) {
    fun add(today: LocalDate, delta: Int): DailyCount {
        val key = today.toString()
        return if (key == day) DailyCount(day, count + delta) else DailyCount(key, delta)
    }

    fun countFor(today: LocalDate): Int = if (today.toString() == day) count else 0
}

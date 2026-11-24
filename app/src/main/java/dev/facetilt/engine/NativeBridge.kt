package dev.facetilt.engine

/**
 * The entire Kotlin ↔ Rust surface. Everything latency-critical (sensors, camera, downsampling,
 * inference, decision) runs in `libfacetilt.so`; Kotlin only receives a rotation and a reason.
 * No image data ever crosses this boundary.
 */
object NativeBridge {
    @Volatile
    private var loaded: Boolean? = null

    /** Loads the native library once. Returns false if it is missing (e.g. unsupported ABI). */
    fun ensureLoaded(): Boolean {
        loaded?.let { return it }
        val ok = try {
            System.loadLibrary("facetilt")
            true
        } catch (e: UnsatisfiedLinkError) {
            false
        }
        loaded = ok
        return ok
    }

    @JvmStatic external fun nativeStart(listener: DecisionListener, packageName: String, rotation: Int, config: FloatArray): Boolean
    @JvmStatic external fun nativeStop()
    @JvmStatic external fun nativeSetConfig(config: FloatArray): Boolean
    @JvmStatic external fun nativeSetDisplayRotation(rotation: Int)
    @JvmStatic external fun nativeReportApplied(applyMicros: Long)
    @JvmStatic external fun nativeGetStats(): LongArray?
    @JvmStatic external fun nativeFrontCameraId(): String?
}

/** Called on the native engine thread; implementations must hop to their own thread. */
interface DecisionListener {
    /** [rotation] is a `Surface.ROTATION_*` value, [reason] a [DecisionReason] code. */
    fun onOrientationDecision(rotation: Int, reason: Int)

    /**
     * Assist mode only: [seize] true = take rotation control from the system
     * (disable its auto-rotate so it cannot flip before FaceTilt decides);
     * false = hand control back (re-enable the system auto-rotate). No-op in
     * normal mode, where it is never called.
     */
    fun onAssistControl(seize: Boolean) {}
}

/** Mirrors `facetilt_decision::Reason` (crates/decision/src/lib.rs). */
enum class DecisionReason(val code: Int, val label: String) {
    GravityUnchanged(0, "No change"),
    FaceConfirmedRotation(1, "Face confirmed rotation"),
    FaceKeptOrientation(2, "Face kept orientation"),
    FallbackNoFace(3, "No face → gravity"),
    FallbackLowConfidence(4, "Low confidence → gravity"),
    FallbackCameraUnavailable(5, "Camera unavailable → gravity"),
    FallbackAmbiguous(6, "Ambiguous face → gravity"),
    Cooldown(7, "Cooldown"),
    UpsideDownDisallowed(8, "Upside-down disabled"),
    SecondCheckReverted(9, "2nd verification → back to portrait"),
    SecondCheckConfirmed(10, "2nd verification confirmed landscape"),
    SecondCheckInconclusive(11, "2nd verification inconclusive"),
    ;

    companion object {
        fun fromCode(code: Int): DecisionReason? = entries.firstOrNull { it.code == code }
    }
}

/**
 * Tunables sent to Rust. Layout must match `crates/jni_bridge/src/config.rs` (v3, 15 floats);
 * Rust validates and clamps every value.
 */
data class EngineConfig(
    val confidenceThreshold: Float = 0.7f,
    val onsetDeg: Float = 15f,
    val stableMs: Int = 120,
    val settleTimeoutMs: Int = 1500,
    val minOpenIntervalMs: Int = 800,
    val cooldownMs: Int = 600,
    val hysteresisDeg: Float = 10f,
    val flatTiltDeg: Float = 70f,
    val allowUpsideDown: Boolean = false,
    val useProximity: Boolean = true,
    /** Java-side SENSOR_ORIENTATION (foldable-aware on API 32+), or -1 to use the NDK value. */
    val sensorOrientationOverride: Int = -1,
    val debugTrace: Boolean = false,
    /** Look again once the phone rests in landscape; switch back if the user is lying down. */
    val secondVerification: Boolean = false,
    /** Only take rotation control to hold portrait when lying; otherwise leave system auto-rotate on. */
    val assistMode: Boolean = false,
) {
    fun toArray(): FloatArray = floatArrayOf(
        VERSION,
        confidenceThreshold,
        onsetDeg,
        stableMs.toFloat(),
        settleTimeoutMs.toFloat(),
        minOpenIntervalMs.toFloat(),
        cooldownMs.toFloat(),
        hysteresisDeg,
        flatTiltDeg,
        if (allowUpsideDown) 1f else 0f,
        if (useProximity) 1f else 0f,
        sensorOrientationOverride.toFloat(),
        if (debugTrace) 1f else 0f,
        if (secondVerification) 1f else 0f,
        if (assistMode) 1f else 0f,
    )

    companion object {
        const val VERSION = 3f
        const val LEN = 15
    }
}

/** Decoded `long[]` from [NativeBridge.nativeGetStats] (layout: `StatsSnapshot::to_array`). */
data class EngineStats(
    val uptimeMs: Long,
    val cameraOnMs: Long,
    val cameraOpens: Long,
    val evaluations: Long,
    val decisionsChanged: Long,
    val decisionsKeptByFace: Long,
    val fallbacks: Long,
    val aborts: Long,
    val inferenceP50Us: Long,
    val inferenceP95Us: Long,
    val lastOpenMs: Long,
    val lastFrameMs: Long,
    val lastInferenceUs: Long,
    val lastDecisionMs: Long,
    val lastApplyUs: Long,
    val lastReason: Long,
    val verifications: Long,
    val reverts: Long,
) {
    private val hours: Double get() = (uptimeMs / 3_600_000.0).coerceAtLeast(1e-9)
    val cameraOnSecondsPerHour: Double get() = cameraOnMs / 1000.0 / hours
    val activationsPerHour: Double get() = cameraOpens / hours

    companion object {
        const val LEN = 18

        fun fromArray(a: LongArray?): EngineStats? {
            if (a == null || a.size != LEN) return null
            return EngineStats(
                a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8], a[9], a[10], a[11], a[12], a[13], a[14], a[15], a[16], a[17],
            )
        }
    }
}

package dev.facetilt

import dev.facetilt.data.DailyCount
import dev.facetilt.engine.DecisionReason
import dev.facetilt.engine.EngineConfig
import dev.facetilt.engine.EngineStats
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import java.time.LocalDate

/** Pins the JNI data layouts shared with Rust (crates/jni_bridge/src/config.rs, metrics.rs). */
class NativeContractTest {
    @Test fun configLayoutMatchesRustDefaults() {
        // Same array as `defaults()` in crates/jni_bridge/src/config.rs tests.
        val expected = floatArrayOf(3f, 0.7f, 15f, 120f, 1500f, 800f, 600f, 10f, 70f, 0f, 1f, -1f, 0f, 0f, 0f)
        assertArrayEquals(expected, EngineConfig().toArray(), 0f)
        assertEquals(EngineConfig.LEN, EngineConfig().toArray().size)
        assertEquals(1f, EngineConfig(secondVerification = true).toArray()[13], 0f)
        assertEquals(1f, EngineConfig(assistMode = true).toArray()[14], 0f)
    }

    @Test fun reasonCodesMatchRust() {
        // facetilt_decision::Reason discriminants.
        val codes = DecisionReason.entries.map { it.code }
        assertEquals((0..11).toList(), codes)
        assertEquals(DecisionReason.FaceKeptOrientation, DecisionReason.fromCode(2))
        assertNull(DecisionReason.fromCode(99))
    }

    @Test fun statsDecoding() {
        val a = LongArray(18) { it.toLong() }
        val s = EngineStats.fromArray(a)!!
        assertEquals(1L, s.cameraOnMs)
        assertEquals(15L, s.lastReason)
        assertEquals(17L, s.reverts)
        assertNull(EngineStats.fromArray(LongArray(3)))
        val hour = EngineStats.fromArray(longArrayOf(3_600_000, 1500, 30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0))!!
        assertEquals(1.5, hour.cameraOnSecondsPerHour, 1e-9)
        assertEquals(30.0, hour.activationsPerHour, 1e-9)
    }

    @Test fun dailyCounterResetsAtMidnight() {
        val d1 = LocalDate.of(2026, 11, 20)
        val c = DailyCount("", 0).add(d1, 3).add(d1, 2)
        assertEquals(5, c.countFor(d1))
        assertEquals(0, c.countFor(d1.plusDays(1)))
        val next = c.add(d1.plusDays(1), 1)
        assertEquals(1, next.countFor(d1.plusDays(1)))
    }
}

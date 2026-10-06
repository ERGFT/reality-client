package com.ergft.realityclient

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.nio.file.Files

class PendingVpnTest {
    @Test
    fun pendingVpnRequestSurvivesActivityRecreationAndIsConsumedOnce() {
        val request = PendingVpnStart("/data/user/0/app/files/reality-pending-vpn-id.json", "/data/user/0/app/files", true)
        PendingVpnStartStore.put(request)

        assertEquals(request, PendingVpnStartStore.take())
        assertNull(PendingVpnStartStore.take())
    }

    @Test
    fun newerPendingVpnRequestReplacesOlderRequest() {
        PendingVpnStartStore.put(PendingVpnStart("/old/reality-pending-vpn-old.json", "/old", true))
        val current = PendingVpnStart("/new/reality-pending-vpn-new.json", "/new", false)
        PendingVpnStartStore.put(current)

        assertEquals(current, PendingVpnStartStore.take())
    }

    @Test
    fun stagedConfigIsConsumedAndRemovedBeforeVpnStarts() {
        withTempDirectory { directory ->
            val original = "{\"secret\":\"large config\"}"
            val staged = PendingVpnConfig.stage(directory, original)

            assertTrue(staged.name.startsWith("reality-pending-vpn-"))
            assertEquals(original, PendingVpnConfig.consume(directory, staged.absolutePath))
            assertFalse(staged.exists())
        }
    }

    @Test
    fun multiMegabyteConfigTravelsThroughAPathRatherThanPendingIntentData() {
        withTempDirectory { directory ->
            val original = "{\"payload\":\"${"x".repeat(2 * 1024 * 1024)}\"}"
            val staged = PendingVpnConfig.stage(directory, original)
            val request = PendingVpnStart(staged.absolutePath, directory.absolutePath, false)

            assertTrue(request.configPath.length < 256)
            assertFalse(request.configPath.contains("payload"))
            assertEquals(original, PendingVpnConfig.consume(directory, request.configPath))
        }
    }

    @Test
    fun stagedConfigRejectsPathsOutsidePrivateAppDirectory() {
        withTempDirectory { directory ->
            withTempDirectory { other ->
                val staged = PendingVpnConfig.stage(other, "{}")
                try {
                    PendingVpnConfig.consume(directory, staged.absolutePath)
                    fail("Expected a path outside the app directory to be rejected")
                } catch (_: IllegalArgumentException) {
                    assertTrue(staged.exists())
                } finally {
                    PendingVpnConfig.erase(other, staged.absolutePath)
                }
            }
        }
    }

    @Test
    fun staleConfigCleanupKeepsOnlyTheCurrentPermissionRequest() {
        withTempDirectory { directory ->
            val current = PendingVpnConfig.stage(directory, "current")
            val stale = PendingVpnConfig.stage(directory, "stale")

            PendingVpnConfig.eraseStale(directory, current.absolutePath)

            assertTrue(current.exists())
            assertFalse(stale.exists())
            PendingVpnConfig.erase(directory, current.absolutePath)
        }
    }

    @Test
    fun staleConfigCleanupLeavesUnrelatedAppFilesUntouched() {
        withTempDirectory { directory ->
            val unrelated = File(directory, "settings.json").apply { writeText("keep") }
            val stale = PendingVpnConfig.stage(directory, "erase")

            PendingVpnConfig.eraseStale(directory)

            assertFalse(stale.exists())
            assertTrue(unrelated.exists())
            assertEquals("keep", unrelated.readText())
        }
    }

    private fun withTempDirectory(block: (File) -> Unit) {
        val directory = Files.createTempDirectory("reality-client-test-").toFile()
        try {
            block(directory)
        } finally {
            directory.deleteRecursively()
        }
    }
}

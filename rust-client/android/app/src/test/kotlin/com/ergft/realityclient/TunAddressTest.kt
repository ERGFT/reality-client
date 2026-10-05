package com.ergft.realityclient

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test
import java.io.File
import java.nio.file.Files

class TunAddressTest {
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
    fun derivesIpv4DnsAddressInsideTunnelSubnet() {
        val cidr = TunAddress.parseCidr("172.19.0.1/30")
        assertArrayEquals(
            byteArrayOf(172.toByte(), 19, 0, 2),
            TunAddress.dnsPeerAddress(cidr.address, cidr.prefix).address,
        )
    }

    @Test
    fun derivesIpv6DnsAddressInsideTunnelSubnet() {
        val cidr = TunAddress.parseCidr("fd00::1/126")
        assertArrayEquals(
            byteArrayOf(
                0xfd.toByte(), 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 2,
            ),
            TunAddress.dnsPeerAddress(cidr.address, cidr.prefix).address,
        )
    }

    @Test
    fun rejectsHostnamesAndScopedAddressesWithoutDnsLookup() {
        assertRejected { TunAddress.parseCidr("vpn.example/24") }
        assertRejected { TunAddress.parseCidr("fe80::1%wlan0/64") }
        assertRejected { TunAddress.parseCidr("0.0.0.0/24") }
        assertRejected { TunAddress.parseCidr("224.0.0.1/24") }
        assertRejected { TunAddress.parseCidr("ff02::1/64") }
    }

    @Test
    fun rejectsInvalidPrefixAndDnsAddressOutsideSubnet() {
        assertRejected { TunAddress.parseCidr("192.0.2.1/33") }
        assertRejected { TunAddress.parseCidr("192.0.2.0/30") }
        assertRejected { TunAddress.parseCidr("192.0.2.3/30") }
    }

    @Test
    fun selectsUsablePeerInsteadOfIpv4BroadcastAtSubnetBoundary() {
        val cidr = TunAddress.parseCidr("192.0.2.2/30")
        assertArrayEquals(
            byteArrayOf(192.toByte(), 0, 2, 1),
            TunAddress.dnsPeerAddress(cidr.address, cidr.prefix).address,
        )
    }

    @Test
    fun findsPeerOnEitherSideOfPointToPointSubnet() {
        val first = TunAddress.parseCidr("192.0.2.0/31")
        val last = TunAddress.parseCidr("192.0.2.1/31")
        assertArrayEquals(
            byteArrayOf(192.toByte(), 0, 2, 1),
            TunAddress.dnsPeerAddress(first.address, first.prefix).address,
        )
        assertArrayEquals(
            byteArrayOf(192.toByte(), 0, 2, 0),
            TunAddress.dnsPeerAddress(last.address, last.prefix).address,
        )
    }

    private fun assertRejected(block: () -> Unit) {
        try {
            block()
            fail("Expected invalid TUN address input to be rejected")
        } catch (_: IllegalArgumentException) {
            // Expected validation failure.
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

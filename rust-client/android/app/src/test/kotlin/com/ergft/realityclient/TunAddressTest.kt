package com.ergft.realityclient

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.fail
import org.junit.Test

class TunAddressTest {
    @Test
    fun pendingVpnRequestSurvivesActivityRecreationAndIsConsumedOnce() {
        val request = PendingVpnStart("{\"inbounds\":[]}", "/data/user/0/app/files", true)
        PendingVpnStartStore.put(request)

        assertEquals(request, PendingVpnStartStore.take())
        assertNull(PendingVpnStartStore.take())
    }

    @Test
    fun newerPendingVpnRequestReplacesOlderRequest() {
        PendingVpnStartStore.put(PendingVpnStart("old", "/old", true))
        val current = PendingVpnStart("new", "/new", false)
        PendingVpnStartStore.put(current)

        assertEquals(current, PendingVpnStartStore.take())
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
}

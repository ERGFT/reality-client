package com.ergft.realityclient

import java.net.InetAddress

internal data class TunCidr(val address: InetAddress, val prefix: Int)

internal object TunAddress {
    fun parseCidr(value: String): TunCidr {
        val parts = value.split('/')
        require(parts.size == 2) { "Некорректный адрес TUN" }
        val address = parseNumericAddress(parts[0])
        val prefix = parts[1].toIntOrNull()
            ?: throw IllegalArgumentException("Некорректный префикс адреса TUN")
        val max = if (address.address.size == 4) 32 else 128
        require(prefix in 0..max) { "Некорректный префикс адреса TUN" }
        require(!address.isAnyLocalAddress() && !address.isMulticastAddress()) {
            "Адрес TUN должен быть адресом узла, а не unspecified или multicast"
        }
        if (address.address.size == 4 && prefix < 31) {
            require(isIpv4HostAddress(address.address, prefix)) {
                "IPv4-адрес TUN не должен совпадать с network или broadcast адресом"
            }
        }
        return TunCidr(address, prefix)
    }

    fun dnsPeerAddress(address: InetAddress, prefix: Int): InetAddress {
        val current = address.address
        val next = stepAddress(current, increment = true)
        if (next != null && isDnsPeerCandidate(next, current, prefix)) {
            return InetAddress.getByAddress(next)
        }
        val previous = stepAddress(current, increment = false)
        if (previous != null && isDnsPeerCandidate(previous, current, prefix)) {
            return InetAddress.getByAddress(previous)
        }
        throw IllegalArgumentException("У адреса TUN нет свободного адреса DNS в своей подсети")
    }

    private fun isDnsPeerCandidate(candidate: ByteArray, current: ByteArray, prefix: Int): Boolean {
        if (candidate.size != current.size || candidate.contentEquals(current)) return false
        val prefixBytes = prefix / 8
        val prefixBits = prefix % 8
        for (index in 0 until prefixBytes) {
            if (candidate[index] != current[index]) return false
        }
        if (prefixBits > 0) {
            val mask = (0xff shl (8 - prefixBits)) and 0xff
            if ((candidate[prefixBytes].toInt() and mask) != (current[prefixBytes].toInt() and mask)) return false
        }
        if (candidate.size == 4 && prefix < 31 && !isIpv4HostAddress(candidate, prefix)) {
            return false
        }
        val peer = InetAddress.getByAddress(candidate)
        return !peer.isAnyLocalAddress() && !peer.isMulticastAddress()
    }

    private fun isIpv4HostAddress(address: ByteArray, prefix: Int): Boolean {
        val prefixBytes = prefix / 8
        val prefixBits = prefix % 8
        var isNetwork = true
        var isBroadcast = true
        for (index in address.indices) {
            val hostMask = when {
                index < prefixBytes -> 0
                index == prefixBytes && prefixBits > 0 -> (1 shl (8 - prefixBits)) - 1
                else -> 0xff
            }
            val hostBits = address[index].toInt() and hostMask
            if (hostBits != 0) isNetwork = false
            if (hostBits != hostMask) isBroadcast = false
        }
        return !isNetwork && !isBroadcast
    }

    private fun stepAddress(address: ByteArray, increment: Boolean): ByteArray? {
        val result = address.copyOf()
        for (index in result.indices.reversed()) {
            val value = result[index].toInt() and 0xff
            if (increment) {
                result[index] = (value + 1).toByte()
                if (value != 0xff) return result
            } else {
                result[index] = (value - 1).toByte()
                if (value != 0) return result
            }
        }
        return null
    }

    private fun parseNumericAddress(value: String): InetAddress {
        require(value.isNotEmpty() && '%' !in value) {
            "Адрес TUN должен быть числовым IPv4 или IPv6-адресом без зоны интерфейса"
        }
        if (':' in value) {
            require(value.all {
                it == ':' || it == '.' || it in '0'..'9' || it.lowercaseChar() in 'a'..'f'
            }) { "Адрес TUN должен быть числовым IPv4 или IPv6-адресом" }
            return try {
                InetAddress.getByName(value)
            } catch (_: java.net.UnknownHostException) {
                throw IllegalArgumentException("Некорректный IPv6-адрес TUN")
            }
        }

        val octets = value.split('.')
        require(octets.size == 4 && octets.all {
            it.isNotEmpty() && it.all { digit -> digit in '0'..'9' }
        }) { "Адрес TUN должен быть числовым IPv4 или IPv6-адресом" }
        val bytes = ByteArray(4)
        for (index in octets.indices) {
            val octet = octets[index].toIntOrNull()
            require(octet != null && octet in 0..255) { "Некорректный IPv4-адрес TUN" }
            bytes[index] = octet.toByte()
        }
        return InetAddress.getByAddress(bytes)
    }
}

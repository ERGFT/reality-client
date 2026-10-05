package com.ergft.realityclient

internal fun singleTunInboundIndex(inboundTypes: List<String>): Int {
    val tunIndices = inboundTypes.indices.filter { inboundTypes[it] == "tun" }
    return when (tunIndices.size) {
        0 -> throw IllegalArgumentException("Для Android требуется входящий тип tun")
        1 -> tunIndices.single()
        else -> throw IllegalArgumentException("Android поддерживает ровно один входящий тип tun")
    }
}

internal fun effectiveTunAddressCidrs(configuredAddresses: List<String>): List<String> {
    val parsed = configuredAddresses.map(TunAddress::parseCidr)
    val ipv4 = parsed.firstOrNull { it.address.address.size == 4 }
        ?: TunAddress.parseCidr("172.19.0.1/30")
    val ipv6 = parsed.firstOrNull { it.address.address.size == 16 }
        ?: TunAddress.parseCidr("fdfe:dcba:9876::1/126")
    require(ipv4.prefix <= 30) { "IPv4-адрес TUN должен иметь префикс не больше /30" }
    return listOf("${ipv4.address.hostAddress}/${ipv4.prefix}", "${ipv6.address.hostAddress}/${ipv6.prefix}")
}

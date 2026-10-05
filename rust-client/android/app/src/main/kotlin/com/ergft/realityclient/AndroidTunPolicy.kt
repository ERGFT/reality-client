package com.ergft.realityclient

internal fun singleTunInboundIndex(inboundTypes: List<String>): Int {
    val tunIndices = inboundTypes.indices.filter { inboundTypes[it] == "tun" }
    return when (tunIndices.size) {
        0 -> throw IllegalArgumentException("Для Android требуется входящий тип tun")
        1 -> tunIndices.single()
        else -> throw IllegalArgumentException("Android поддерживает ровно один входящий тип tun")
    }
}

private val unsupportedAndroidTunFields = setOf(
    "route_address",
    "inet4_route_address",
    "inet6_route_address",
    "route_address_set",
    "inet4_route_address_set",
    "inet6_route_address_set",
    "route_exclude_address",
    "inet4_route_exclude_address",
    "inet6_route_exclude_address",
    "route_exclude_address_set",
    "inet4_route_exclude_address_set",
    "inet6_route_exclude_address_set",
    "auto_redirect",
    "include_interface",
    "exclude_interface",
    "include_uid",
    "exclude_uid",
    "exclude_package",
    "include_android_user",
    "loopback_address",
    "iproute2_table_index",
    "iproute2_rule_index",
)

internal fun validateAndroidTunOptions(
    configuredFields: Set<String>,
    autoRoute: Boolean,
    strictRoute: Boolean,
) {
    val unsupported = configuredFields.intersect(unsupportedAndroidTunFields).firstOrNull()
    require(unsupported == null) {
        "Android пока не поддерживает параметр TUN $unsupported"
    }
    require(autoRoute) {
        "Android пока не поддерживает отключение автоматических TUN-маршрутов"
    }
    require(!strictRoute) {
        "Android пока не поддерживает strict_route для TUN"
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

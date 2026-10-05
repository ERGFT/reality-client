package com.ergft.realityclient

internal fun singleTunInboundIndex(inboundTypes: List<String>): Int {
    val tunIndices = inboundTypes.indices.filter { inboundTypes[it] == "tun" }
    return when (tunIndices.size) {
        0 -> throw IllegalArgumentException("Для Android требуется входящий тип tun")
        1 -> tunIndices.single()
        else -> throw IllegalArgumentException("Android поддерживает ровно один входящий тип tun")
    }
}

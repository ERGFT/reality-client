package com.ergft.realityclient

import java.io.File
import java.io.RandomAccessFile
import java.nio.charset.StandardCharsets

internal object PendingVpnConfig {
    private const val PREFIX = "reality-pending-vpn-"
    private const val SUFFIX = ".json"

    fun stage(filesDir: File, config: String): File {
        val root = filesDir.canonicalFile
        val target = File(root, "$PREFIX${java.util.UUID.randomUUID()}$SUFFIX")
        target.createNewFile().also { created ->
            check(created) { "Не удалось создать временную конфигурацию VPN" }
        }
        try {
            target.writeText(config, StandardCharsets.UTF_8)
            return target
        } catch (problem: Exception) {
            eraseAndDelete(target)
            throw problem
        }
    }

    fun consume(filesDir: File, path: String): String {
        val file = checkedFile(filesDir, path)
        val config = file.readText(StandardCharsets.UTF_8)
        eraseAndDelete(file)
        return config
    }

    fun erase(filesDir: File, path: String) {
        val file = checkedFile(filesDir, path)
        eraseAndDelete(file)
    }

    fun eraseStale(filesDir: File, keepPath: String? = null) {
        val root = filesDir.canonicalFile
        val keep = keepPath?.let { checkedFile(root, it) }
        root.listFiles()?.forEach { file ->
            val canonical = file.canonicalFile
            if (file.name.startsWith(PREFIX) && file.name.endsWith(SUFFIX) &&
                canonical.name.startsWith(PREFIX) && canonical.name.endsWith(SUFFIX) &&
                canonical.parentFile == root && canonical != keep
            ) {
                eraseAndDelete(canonical)
            }
        }
    }

    private fun checkedFile(filesDir: File, path: String): File {
        val root = filesDir.canonicalFile
        val file = File(path).canonicalFile
        require(file.parentFile == root && file.name.startsWith(PREFIX) &&
            file.name.endsWith(SUFFIX) && file.isFile
        ) { "Путь временной конфигурации VPN недопустим" }
        return file
    }

    private fun eraseAndDelete(file: File) {
        if (file.exists()) {
            RandomAccessFile(file, "rw").use { output ->
                val zeros = ByteArray(4096)
                var remaining = output.length()
                output.seek(0)
                while (remaining > 0) {
                    val count = minOf(remaining, zeros.size.toLong()).toInt()
                    output.write(zeros, 0, count)
                    remaining -= count
                }
                output.fd.sync()
            }
            check(file.delete()) { "Не удалось удалить временную конфигурацию VPN" }
        }
    }
}

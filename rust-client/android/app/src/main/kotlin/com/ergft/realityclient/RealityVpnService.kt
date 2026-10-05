package com.ergft.realityclient

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Intent
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.net.VpnService
import android.os.Build
import android.os.ParcelFileDescriptor
import org.json.JSONObject

class RealityVpnService : VpnService() {
    companion object {
        const val ACTION_START = "com.ergft.realityclient.START"
        const val ACTION_STOP = "com.ergft.realityclient.STOP"
        const val EXTRA_CONFIG = "config"
        const val EXTRA_BASE_DIR = "base_dir"
        const val EXTRA_REMOVE_PROFILE_SECRET = "remove_profile_secret"
        private const val CHANNEL_ID = "reality_vpn"
        private const val NOTIFICATION_ID = 41

        init {
            System.loadLibrary("reality_client_rs")
        }
    }

    private var tunnel: ParcelFileDescriptor? = null
    private var running = false

    private external fun nativeStart(
        config: String,
        nativeLibraryDir: String,
        baseDir: String,
        tunFd: Int,
        removeProfileSecret: Boolean,
    ): String

    private external fun nativeStop(): String
    private external fun nativeVpnStartFailed(removeProfileSecret: Boolean, error: String)

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_STOP -> {
                stopTunnel(startId)
                return START_NOT_STICKY
            }
            ACTION_START -> {
                startForeground(NOTIFICATION_ID, notification("Подключение…"))
                val config = intent.getStringExtra(EXTRA_CONFIG)
                val baseDir = intent.getStringExtra(EXTRA_BASE_DIR)
                val removeProfileSecret = intent.getBooleanExtra(EXTRA_REMOVE_PROFILE_SECRET, false)
                if (config.isNullOrBlank() || baseDir.isNullOrBlank()) {
                    nativeVpnStartFailed(
                        removeProfileSecret,
                        "Android не передал конфигурацию или папку данных VPN.",
                    )
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelf(startId)
                    return START_NOT_STICKY
                }
                try {
                    startTunnel(config, baseDir, removeProfileSecret)
                } catch (problem: Exception) {
                    val stopProblem = try {
                        nativeStop().takeIf { it.isNotEmpty() }
                    } catch (_: Exception) {
                        null
                    }
                    val message = listOfNotNull(
                        problem.localizedMessage ?: "Не удалось запустить Android VPN",
                        stopProblem,
                    ).joinToString(" ")
                    nativeVpnStartFailed(
                        removeProfileSecret,
                        message,
                    )
                    stopTunnel(startId)
                    return START_NOT_STICKY
                }
            }
        }
        return if (running) START_STICKY else START_NOT_STICKY
    }

    private fun startTunnel(config: String, baseDir: String, removeProfileSecret: Boolean) {
        if (running) return
        val connectivity = getSystemService(ConnectivityManager::class.java)
        @Suppress("DEPRECATION")
        val otherVpnIsActive = connectivity.allNetworks.any { network ->
            connectivity.getNetworkCapabilities(network)
                ?.hasTransport(NetworkCapabilities.TRANSPORT_VPN) == true
        }
        if (otherVpnIsActive) {
            throw IllegalStateException("Уже работает другое VPN-приложение. Reality Client остановлен, действующий VPN не затронут.")
        }
        val root = JSONObject(config)
        val inbounds = root.optJSONArray("inbounds")
            ?: throw IllegalArgumentException("В конфигурации отсутствуют inbounds")
        val tun = (0 until inbounds.length())
            .asSequence()
            .map { inbounds.getJSONObject(it) }
            .firstOrNull { it.optString("type") == "tun" }
            ?: throw IllegalArgumentException("Для Android требуется входящий тип tun")
        if (!tun.optBoolean("dns_hijack", true) || root.optJSONObject("dns") == null) {
            throw IllegalArgumentException("Android требует DNS-модуль и перехват DNS-запросов в TUN")
        }

        // The core receives a ready descriptor, so it cannot configure Android routes itself.
        if (tun.has("route_exclude_address") || tun.has("route_exclude_address_set") ||
            tun.has("route_address") || tun.has("route_address_set") ||
            tun.has("include_package") || tun.has("exclude_package") ||
            !tun.optBoolean("auto_route", true) || tun.optBoolean("strict_route", false)
        ) {
            throw IllegalArgumentException("Android пока не поддерживает пользовательские TUN-маршруты и kill switch из JSON")
        }
        val builder = Builder()
            .setSession("Reality Client")
            .setMtu(tun.optInt("mtu", 1500))

        val addresses = tun.optJSONArray("address")
            ?: throw IllegalArgumentException("В TUN-конфигурации отсутствует address")
        var hasIpv4 = false
        var hasIpv6 = false
        var hasDnsAddress = false
        for (i in 0 until addresses.length()) {
            val (address, prefix) = TunAddress.parseCidr(addresses.getString(i))
            builder.addAddress(address, prefix)
            if (address.address.size == 4) {
                hasIpv4 = true
                if (prefix < 32) {
                    builder.addDnsServer(TunAddress.dnsPeerAddress(address, prefix))
                    hasDnsAddress = true
                }
            } else {
                hasIpv6 = true
                if (prefix < 128) {
                    builder.addDnsServer(TunAddress.dnsPeerAddress(address, prefix))
                    hasDnsAddress = true
                }
            }
        }
        require(hasIpv4 || hasIpv6) { "В TUN-конфигурации нет IP-адресов" }
        require(hasDnsAddress) { "Для Android DNS требуется адрес TUN с доступным адресом DNS внутри подсети" }
        if (hasIpv4) builder.addRoute("0.0.0.0", 0)
        if (hasIpv6) builder.addRoute("::", 0)

        val established = builder.establish()
            ?: throw IllegalStateException("Android не выдал TUN-дескриптор")
        tunnel = established
        val fd = established.detachFd()
        val error = nativeStart(
            config,
            applicationInfo.nativeLibraryDir,
            baseDir,
            fd,
            removeProfileSecret,
        )
        if (error.isNotEmpty()) {
            // The FFI contract transfers ownership of the detached FD to rc_start.
            tunnel = null
            throw IllegalStateException(error)
        }
        tunnel = null
        running = true
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, notification("VPN подключён"))
    }

    private fun stopTunnel(startId: Int? = null) {
        if (running) {
            nativeStop()
            running = false
        }
        tunnel?.close()
        tunnel = null
        stopForeground(STOP_FOREGROUND_REMOVE)
        if (startId == null) stopSelf() else stopSelf(startId)
    }

    override fun onRevoke() {
        stopTunnel()
        super.onRevoke()
    }

    override fun onDestroy() {
        stopTunnel()
        super.onDestroy()
    }

    private fun notification(status: String): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "Reality VPN", NotificationManager.IMPORTANCE_LOW),
            )
        }
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(this, CHANNEL_ID)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }.setContentTitle("Reality Client")
            .setContentText(status)
            .setSmallIcon(android.R.drawable.stat_sys_warning)
            .setOngoing(running)
            .build()
    }
}

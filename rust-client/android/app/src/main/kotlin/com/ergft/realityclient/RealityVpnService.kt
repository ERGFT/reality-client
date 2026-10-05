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
        const val EXTRA_CONFIG_PATH = "config_path"
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
                val configPath = intent.getStringExtra(EXTRA_CONFIG_PATH)
                val baseDir = intent.getStringExtra(EXTRA_BASE_DIR)
                val removeProfileSecret = intent.getBooleanExtra(EXTRA_REMOVE_PROFILE_SECRET, false)
                if (configPath.isNullOrBlank() || baseDir.isNullOrBlank()) {
                    nativeVpnStartFailed(
                        removeProfileSecret,
                        "Android не передал конфигурацию или папку данных VPN.",
                    )
                    stopForeground(STOP_FOREGROUND_REMOVE)
                    stopSelf(startId)
                    return START_NOT_STICKY
                }
                try {
                    // Keep large/full JSON configs out of Binder Intent extras. The
                    // file is private to this app and erased immediately after read.
                    val config = PendingVpnConfig.consume(filesDir, configPath)
                    startTunnel(config, baseDir, removeProfileSecret)
                } catch (problem: Exception) {
                    runCatching { PendingVpnConfig.erase(filesDir, configPath) }
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
        val inboundObjects = (0 until inbounds.length()).map { inbounds.getJSONObject(it) }
        val tunIndex = singleTunInboundIndex(inboundObjects.map { it.optString("type") })
        val tun = inboundObjects[tunIndex]
        if (!tun.optBoolean("dns_hijack", true) || root.optJSONObject("dns") == null) {
            throw IllegalArgumentException("Android требует DNS-модуль и перехват DNS-запросов в TUN")
        }

        // The core receives a ready descriptor, so it cannot configure Android routes itself.
        val tunFields = buildSet {
            val keys = tun.keys()
            while (keys.hasNext()) add(keys.next())
        }
        validateAndroidTunOptions(
            tunFields,
            autoRoute = tun.optBoolean("auto_route", true),
            strictRoute = tun.optBoolean("strict_route", false),
        )
        val builder = Builder()
            .setSession("Reality Client")
            .setMtu(tun.optInt("mtu", 1500))

        val addressValues = mutableListOf<String>()
        for (field in listOf("address", "inet4_address", "inet6_address")) {
            when (val value = tun.opt(field)) {
                null -> Unit
                is String -> addressValues += value
                is org.json.JSONArray -> for (index in 0 until value.length()) {
                    addressValues += value.getString(index)
                }
                else -> throw IllegalArgumentException("Поле $field TUN должно быть строкой или массивом строк")
            }
        }
        val addresses = effectiveTunAddressCidrs(addressValues).map(TunAddress::parseCidr)
        var hasIpv4 = false
        var hasIpv6 = false
        var hasDnsAddress = false
        for ((address, prefix) in addresses.map { it.address to it.prefix }) {
            builder.addAddress(address, prefix)
            if (address.address.size == 4) {
                hasIpv4 = true
            } else {
                hasIpv6 = true
            }
            val maxPrefix = if (address.address.size == 4) 32 else 128
            if (prefix < maxPrefix) {
                builder.addDnsServer(TunAddress.dnsPeerAddress(address, prefix))
                hasDnsAddress = true
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

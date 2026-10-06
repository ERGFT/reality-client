package com.ergft.realityclient

import android.app.NativeActivity
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.net.VpnService
import android.os.Build
import org.json.JSONArray
import org.json.JSONObject

internal data class PendingVpnStart(
    val configPath: String,
    val baseDir: String,
    val removeProfileSecret: Boolean,
)

internal object PendingVpnStartStore {
    private var pending: PendingVpnStart? = null

    @Synchronized
    fun put(request: PendingVpnStart) {
        pending = request
    }

    @Synchronized
    fun take(): PendingVpnStart? {
        val request = pending
        pending = null
        return request
    }

    @Synchronized
    fun current(): PendingVpnStart? = pending
}

class MainActivity : NativeActivity() {
    private external fun nativeVpnPermissionDenied()
    private external fun nativeActivityDestroyed()

    fun readClipboardText(): String {
        val clipboard = getSystemService(ClipboardManager::class.java)
        if (!clipboard.hasPrimaryClip()) return ""
        val clip = clipboard.primaryClip ?: return ""
        if (clip.itemCount != 1) return "\n"
        return clip.getItemAt(0).coerceToText(this)?.toString().orEmpty()
    }

    /**
     * Системная тональная палитра Material You (Android 12+) строкой
     * `ключ=AARRGGBB,…`. На более старых версиях — пустая строка, и интерфейс
     * использует встроенную тональную тему.
     */
    fun readSystemPalette(): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return ""
        val shades = mapOf(
            "a1_0" to android.R.color.system_accent1_0,
            "a1_100" to android.R.color.system_accent1_100,
            "a1_200" to android.R.color.system_accent1_200,
            "a1_600" to android.R.color.system_accent1_600,
            "a1_700" to android.R.color.system_accent1_700,
            "a1_800" to android.R.color.system_accent1_800,
            "a1_900" to android.R.color.system_accent1_900,
            "n1_10" to android.R.color.system_neutral1_10,
            "n1_50" to android.R.color.system_neutral1_50,
            "n1_100" to android.R.color.system_neutral1_100,
            "n1_200" to android.R.color.system_neutral1_200,
            "n1_700" to android.R.color.system_neutral1_700,
            "n1_800" to android.R.color.system_neutral1_800,
            "n1_900" to android.R.color.system_neutral1_900,
            "n2_200" to android.R.color.system_neutral2_200,
            "n2_700" to android.R.color.system_neutral2_700,
            "n2_900" to android.R.color.system_neutral2_900,
        )
        return runCatching {
            shades.entries.joinToString(",") { (key, id) ->
                "%s=%08x".format(key, getColor(id))
            }
        }.getOrDefault("")
    }

    fun openRepository(): Boolean = runCatching {
        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://github.com/ERGFT/reality-client")))
    }.isSuccess

    fun listLaunchableApps(): String {
        val launcherIntent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        @Suppress("DEPRECATION")
        val activities = packageManager.queryIntentActivities(launcherIntent, 0)
        val packages = activities.mapNotNull { resolveInfo ->
            val activityInfo = resolveInfo.activityInfo ?: return@mapNotNull null
            val packageName = activityInfo.packageName ?: return@mapNotNull null
            packageName to resolveInfo.loadLabel(packageManager).toString()
        }.distinctBy { it.first }.sortedBy { it.second.lowercase() }
        return JSONArray(packages.map { (packageName, label) ->
            JSONObject().put("package", packageName).put("label", label)
        }).toString()
    }

    override fun onCreate(savedInstanceState: android.os.Bundle?) {
        super.onCreate(savedInstanceState)
        PendingVpnConfig.eraseStale(filesDir, PendingVpnStartStore.current()?.configPath)
    }

    fun requestVpn(config: String, baseDir: String, removeProfileSecret: Boolean) {
        val stagedConfig = PendingVpnConfig.stage(filesDir, config)
        runOnUiThread {
            PendingVpnStartStore.take()?.let { previous ->
                runCatching { PendingVpnConfig.erase(filesDir, previous.configPath) }
                nativeVpnPermissionDenied()
            }
            val request = PendingVpnStart(stagedConfig.absolutePath, baseDir, removeProfileSecret)
            PendingVpnStartStore.put(request)
            val permission = VpnService.prepare(this)
            if (permission == null) {
                PendingVpnStartStore.take()
                startVpnService(request)
            } else {
                try {
                    @Suppress("DEPRECATION")
                    startActivityForResult(permission, VPN_PERMISSION_REQUEST)
                } catch (problem: RuntimeException) {
                    PendingVpnStartStore.take()?.let { PendingVpnConfig.erase(filesDir, it.configPath) }
                    nativeVpnPermissionDenied()
                    throw problem
                }
            }
        }
    }

    fun stopVpn() {
        runOnUiThread {
            PendingVpnStartStore.take()?.let { pending ->
                runCatching { PendingVpnConfig.erase(filesDir, pending.configPath) }
                nativeVpnPermissionDenied()
            }
            val intent = Intent(this, RealityVpnService::class.java).setAction(RealityVpnService.ACTION_STOP)
            startService(intent)
        }
    }

    override fun onDestroy() {
        if (isFinishing) PendingVpnStartStore.take()?.let { pending ->
            runCatching { PendingVpnConfig.erase(filesDir, pending.configPath) }
            nativeVpnPermissionDenied()
        }
        nativeActivityDestroyed()
        super.onDestroy()
    }

    @Deprecated("Kept for compatibility with VpnService.prepare's Activity result flow")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != VPN_PERMISSION_REQUEST) return
        val pending = PendingVpnStartStore.take() ?: return
        if (resultCode == RESULT_OK) {
            startVpnService(pending)
        } else {
            runCatching { PendingVpnConfig.erase(filesDir, pending.configPath) }
            nativeVpnPermissionDenied()
        }
    }

    private fun startVpnService(pending: PendingVpnStart) {
        val intent = Intent(this, RealityVpnService::class.java)
            .setAction(RealityVpnService.ACTION_START)
            .putExtra(RealityVpnService.EXTRA_CONFIG_PATH, pending.configPath)
            .putExtra(RealityVpnService.EXTRA_BASE_DIR, pending.baseDir)
            .putExtra(RealityVpnService.EXTRA_REMOVE_PROFILE_SECRET, pending.removeProfileSecret)
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startForegroundService(intent)
            } else {
                startService(intent)
            }
        } catch (problem: RuntimeException) {
            runCatching { PendingVpnConfig.erase(filesDir, pending.configPath) }
            nativeVpnPermissionDenied()
            throw problem
        }
    }

    companion object {
        private const val VPN_PERMISSION_REQUEST = 4102
    }
}

package com.ergft.realityclient

import android.app.NativeActivity
import android.content.ClipboardManager
import android.content.Intent
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

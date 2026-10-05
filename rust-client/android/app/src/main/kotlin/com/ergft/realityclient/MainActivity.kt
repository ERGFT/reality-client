package com.ergft.realityclient

import android.app.NativeActivity
import android.content.Intent
import android.net.VpnService
import android.os.Build

internal data class PendingVpnStart(
    val config: String,
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
}

class MainActivity : NativeActivity() {
    private external fun nativeVpnPermissionDenied()
    private external fun nativeActivityDestroyed()

    fun requestVpn(config: String, baseDir: String, removeProfileSecret: Boolean) {
        runOnUiThread {
            if (PendingVpnStartStore.take() != null) {
                nativeVpnPermissionDenied()
            }
            val permission = VpnService.prepare(this)
            if (permission == null) {
                startVpnService(config, baseDir, removeProfileSecret)
            } else {
                PendingVpnStartStore.put(PendingVpnStart(config, baseDir, removeProfileSecret))
                try {
                    @Suppress("DEPRECATION")
                    startActivityForResult(permission, VPN_PERMISSION_REQUEST)
                } catch (problem: RuntimeException) {
                    PendingVpnStartStore.take()
                    nativeVpnPermissionDenied()
                    throw problem
                }
            }
        }
    }

    fun stopVpn() {
        runOnUiThread {
            if (PendingVpnStartStore.take() != null) {
                nativeVpnPermissionDenied()
            }
            val intent = Intent(this, RealityVpnService::class.java).setAction(RealityVpnService.ACTION_STOP)
            startService(intent)
        }
    }

    override fun onDestroy() {
        if (isFinishing && PendingVpnStartStore.take() != null) {
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
            startVpnService(pending.config, pending.baseDir, pending.removeProfileSecret)
        } else {
            nativeVpnPermissionDenied()
        }
    }

    private fun startVpnService(config: String, baseDir: String, removeProfileSecret: Boolean) {
        val intent = Intent(this, RealityVpnService::class.java)
            .setAction(RealityVpnService.ACTION_START)
            .putExtra(RealityVpnService.EXTRA_CONFIG, config)
            .putExtra(RealityVpnService.EXTRA_BASE_DIR, baseDir)
            .putExtra(RealityVpnService.EXTRA_REMOVE_PROFILE_SECRET, removeProfileSecret)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    companion object {
        private const val VPN_PERMISSION_REQUEST = 4102
    }
}

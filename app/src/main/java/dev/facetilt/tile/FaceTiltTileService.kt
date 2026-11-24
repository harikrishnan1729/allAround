package dev.facetilt.tile

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Intent
import android.os.Build
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import dev.facetilt.FaceTiltApp
import dev.facetilt.MainActivity
import dev.facetilt.service.FaceTiltService
import dev.facetilt.service.ServiceController
import kotlinx.coroutines.launch

/**
 * Quick Settings tile. Turning FaceTilt OFF stops the service directly. Turning it ON goes
 * through [MainActivity] (briefly visible), because a camera-type foreground service may only
 * be started while the app is visible (Android 14+), and Android 12+ forbids background starts.
 */
class FaceTiltTileService : TileService() {

    override fun onStartListening() {
        super.onStartListening()
        refresh()
    }

    override fun onClick() {
        super.onClick()
        if (FaceTiltService.isRunning) {
            val repo = (application as FaceTiltApp).settings
            FaceTiltApp.appScope.launch { repo.setEnabled(false) }
            ServiceController.stop(this)
            qsTile?.apply { state = Tile.STATE_INACTIVE; updateTile() }
        } else if (isLocked) {
            unlockAndRun { openToEnable() }
        } else {
            openToEnable()
        }
    }

    @SuppressLint("StartActivityAndCollapseDeprecated")
    private fun openToEnable() {
        val intent = Intent(this, MainActivity::class.java)
            .putExtra(MainActivity.EXTRA_ENABLE, true)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            // API 34+: the Intent overload throws for apps targeting 34+.
            val pi = PendingIntent.getActivity(this, 0, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
            startActivityAndCollapse(pi)
        } else {
            @Suppress("DEPRECATION")
            startActivityAndCollapse(intent)
        }
    }

    private fun refresh() {
        qsTile?.apply {
            state = if (FaceTiltService.isRunning) Tile.STATE_ACTIVE else Tile.STATE_INACTIVE
            updateTile()
        }
    }
}

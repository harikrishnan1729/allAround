package dev.facetilt.boot

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import dev.facetilt.FaceTiltApp
import dev.facetilt.MainActivity
import dev.facetilt.R
import dev.facetilt.service.ServiceController
import kotlinx.coroutines.launch

/**
 * "Start on boot" (off by default; this receiver is disabled in the manifest until the user
 * opts in).
 *
 * Android limits this, and FaceTilt implements both paths:
 * * Android 10 (API 29): the camera-type service can start directly from BOOT_COMPLETED.
 * * Android 11+ (API 30+): a foreground service started from the background has no camera
 *   access ("while-in-use" restriction; BOOT_COMPLETED is not an exemption), and Android 14+
 *   rejects starting a camera-type service from the background outright (Android 15 also lists
 *   camera among the types BOOT_COMPLETED receivers may not launch). So we post a silent
 *   notification instead; tapping it opens FaceTilt, which starts the service while visible.
 */
class BootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action != Intent.ACTION_BOOT_COMPLETED) return
        val pending = goAsync()
        val repo = (context.applicationContext as FaceTiltApp).settings
        FaceTiltApp.appScope.launch {
            try {
                val s = repo.current()
                if (!s.enabled || !s.startOnBoot || !s.consentGiven) return@launch
                if (Build.VERSION.SDK_INT == Build.VERSION_CODES.Q) {
                    ServiceController.start(context)
                } else {
                    postResumeNotification(context)
                }
            } finally {
                pending.finish()
            }
        }
    }

    private fun postResumeNotification(context: Context) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            return
        }
        val nm = context.getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, context.getString(R.string.notif_channel_name), NotificationManager.IMPORTANCE_LOW))
        val open = PendingIntent.getActivity(
            context, 2,
            Intent(context, MainActivity::class.java).putExtra(MainActivity.EXTRA_ENABLE, true),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_tile)
            .setContentTitle(context.getString(R.string.notif_resume_title))
            .setContentText(context.getString(R.string.notif_resume_text))
            .setContentIntent(open)
            .setAutoCancel(true)
            .setSilent(true)
            .build()
        nm.notify(ID, n)
    }

    companion object {
        private const val CHANNEL = "facetilt_boot"
        private const val ID = 2

        fun setEnabled(context: Context, enabled: Boolean) {
            context.packageManager.setComponentEnabledSetting(
                ComponentName(context, BootReceiver::class.java),
                if (enabled) PackageManager.COMPONENT_ENABLED_STATE_ENABLED else PackageManager.COMPONENT_ENABLED_STATE_DISABLED,
                PackageManager.DONT_KILL_APP,
            )
        }
    }
}

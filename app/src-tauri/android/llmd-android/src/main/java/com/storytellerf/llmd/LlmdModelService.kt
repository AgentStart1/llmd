package com.storytellerf.llmd

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.IBinder
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.Job

/** Only the app's switch can start this service; exported Binder clients cannot load models. */
class LlmdModelService : Service() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var observer: Job? = null

    override fun onCreate() {
        super.onCreate()
        LlmdAndroidBridge.configure(this)
        getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "Model service", NotificationManager.IMPORTANCE_LOW))
    }

    private fun observeAfter(start: Job) {
        if (observer != null) return
        observer = scope.launch {
            // Do not interpret the initial Off snapshot as completion of an unstarted action.
            start.join()
            LlmdRuntime.host.state.collect { state ->
                when (state) {
                    LlmdServiceState.Off, is LlmdServiceState.Failed -> {
                        stopForeground(STOP_FOREGROUND_REMOVE)
                        stopSelf()
                    }
                    else -> getSystemService(NotificationManager::class.java).notify(NOTIFICATION, notification(state))
                }
            }
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            ACTION_START -> {
                val model = intent.getStringExtra(EXTRA_MODEL)
                if (model.isNullOrBlank()) { stopSelf(); return START_NOT_STICKY }
                startForeground(NOTIFICATION, notification(LlmdServiceState.Starting(model)), ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
                observeAfter(LlmdRuntime.host.enable(model))
            }
            ACTION_STOP -> observeAfter(LlmdRuntime.host.disable())
            else -> stopSelf()
        }
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        scope.cancel()
        if (LlmdRuntime.host.state.value != LlmdServiceState.Off && LlmdRuntime.host.state.value !is LlmdServiceState.Failed) {
            LlmdRuntime.host.disable()
        }
        super.onDestroy()
    }

    private fun notification(state: LlmdServiceState): Notification {
        val open = packageManager.getLaunchIntentForPackage(packageName)
        val stop = PendingIntent.getService(this, 1, Intent(this, LlmdModelService::class.java).setAction(ACTION_STOP), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL).setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle("LLMD model service")
            .setContentText(when (state) {
                is LlmdServiceState.Ready -> "Ready for requests"
                is LlmdServiceState.Stopping -> "Releasing model resources"
                else -> "Loading model; requests will be ready shortly"
            }).setOngoing(true)
            .apply { if (open != null) setContentIntent(PendingIntent.getActivity(this@LlmdModelService, 0, open, PendingIntent.FLAG_IMMUTABLE)) }
            .addAction(Notification.Action.Builder(null, "Switch off", stop).build()).build()
    }

    companion object {
        const val ACTION_START = "com.storytellerf.llmd.action.START_MODEL"
        const val ACTION_STOP = "com.storytellerf.llmd.action.STOP_MODEL"
        const val EXTRA_MODEL = "model"
        private const val CHANNEL = "llmd_model_service"
        private const val NOTIFICATION = 1
    }
}

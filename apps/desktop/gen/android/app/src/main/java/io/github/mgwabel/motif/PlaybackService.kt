package io.github.mgwabel.motif

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.media.app.NotificationCompat.MediaStyle

/**
 * Spike (phase 0): a foreground media-playback service with a media session,
 * which is what keeps Android from freezing or killing the process while the
 * Rust engine plays with the screen off.
 * shortcut: notification controls are not wired to the engine yet, do that in phase 5 (Tauri plugin).
 */
class PlaybackService : Service() {
  private lateinit var session: MediaSessionCompat

  override fun onCreate() {
    super.onCreate()
    session = MediaSessionCompat(this, "Motif").apply {
      setMetadata(
        MediaMetadataCompat.Builder()
          .putString(MediaMetadataCompat.METADATA_KEY_TITLE, "Background audio spike")
          .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, "Motif")
          .build()
      )
      setPlaybackState(
        PlaybackStateCompat.Builder()
          .setActions(PlaybackStateCompat.ACTION_PLAY_PAUSE)
          .setState(PlaybackStateCompat.STATE_PLAYING, PlaybackStateCompat.PLAYBACK_POSITION_UNKNOWN, 1f)
          .build()
      )
      isActive = true
    }
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    val manager = getSystemService(NotificationManager::class.java)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      manager.createNotificationChannel(
        NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW)
      )
    }
    val open = PendingIntent.getActivity(
      this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
    )
    val notification: Notification = NotificationCompat.Builder(this, CHANNEL)
      .setSmallIcon(android.R.drawable.ic_media_play)
      .setContentTitle("Background audio spike")
      .setContentText("Motif")
      .setContentIntent(open)
      .setOngoing(true)
      .setStyle(MediaStyle().setMediaSession(session.sessionToken))
      .build()
    ServiceCompat.startForeground(
      this, 1, notification,
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else 0
    )
    return START_STICKY
  }

  override fun onDestroy() {
    session.release()
    super.onDestroy()
  }

  override fun onBind(intent: Intent?): IBinder? = null

  private companion object {
    const val CHANNEL = "playback"
  }
}

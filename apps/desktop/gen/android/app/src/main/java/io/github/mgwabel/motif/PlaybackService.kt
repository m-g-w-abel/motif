package io.github.mgwabel.motif

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Bitmap
import android.os.Build
import android.os.IBinder
import android.util.Log
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.media.app.NotificationCompat.MediaStyle
import app.tauri.plugin.Channel
import app.tauri.plugin.JSObject

/**
 * The media session and its notification, shown by [MediaPlugin]. Runs on the
 * main thread. While something is playing or paused, [PlaybackService] runs in
 * the foreground, which keeps Android from freezing the app (and the engine in
 * it) in the background.
 */
object Playback {
  @Volatile var buttons: Channel? = null
  private var session: MediaSessionCompat? = null
  private var state = PlaybackStateCompat.STATE_NONE
  private var title: String? = null
  private var artist: String? = null
  private var art: Bitmap? = null
  /** Whether [PlaybackService] is running. */
  var running = false

  private const val CHANNEL = "playback"
  const val NOTIFICATION = 1
  private const val ACTIONS = PlaybackStateCompat.ACTION_PLAY or PlaybackStateCompat.ACTION_PAUSE or
    PlaybackStateCompat.ACTION_PLAY_PAUSE or PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
    PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or PlaybackStateCompat.ACTION_STOP or
    PlaybackStateCompat.ACTION_SEEK_TO

  /** Sends the engine a command (`protocol::Command`) without arguments. */
  fun send(type: String) {
    buttons?.send(JSObject().put("type", type))
  }

  private fun session(context: Context) = session ?: MediaSessionCompat(context, "Motif").apply {
    // Headset buttons reach the session only while it is active, never restarting a stopped engine.
    setMediaButtonReceiver(null)
    setCallback(object : MediaSessionCompat.Callback() {
      override fun onPlay() = send("play")
      override fun onPause() = send("pause")
      override fun onSkipToNext() = send("next")
      override fun onSkipToPrevious() = send("previous")
      override fun onStop() = send("stop")
      override fun onSeekTo(pos: Long) {
        buttons?.send(JSObject().put("type", "seek").put("position_ms", pos))
      }
    })
    session = this
  }

  fun showTrack(context: Context, title: String?, artist: String?, album: String?, durationMs: Long, art: Bitmap?) {
    this.title = title
    this.artist = artist
    this.art = art
    session(context).setMetadata(
      MediaMetadataCompat.Builder()
        .putString(MediaMetadataCompat.METADATA_KEY_TITLE, title)
        .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, artist)
        .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, album)
        .putLong(MediaMetadataCompat.METADATA_KEY_DURATION, durationMs)
        .putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, art)
        .build()
    )
    refresh(context)
  }

  /** `status` is a `protocol::PlaybackStatus`. */
  fun showStatus(context: Context, status: String, positionMs: Long) {
    state = when (status) {
      "playing" -> PlaybackStateCompat.STATE_PLAYING
      "paused" -> PlaybackStateCompat.STATE_PAUSED
      else -> PlaybackStateCompat.STATE_STOPPED
    }
    val session = session(context)
    session.setPlaybackState(
      PlaybackStateCompat.Builder().setActions(ACTIONS).setState(state, positionMs, 1f).build()
    )
    session.isActive = state != PlaybackStateCompat.STATE_STOPPED
    sync(context)
  }

  /** Runs [PlaybackService] while something plays or is paused. */
  fun sync(context: Context) {
    val service = Intent(context, PlaybackService::class.java)
    when {
      state != PlaybackStateCompat.STATE_PLAYING && state != PlaybackStateCompat.STATE_PAUSED ->
        context.stopService(service)
      // Started once, from the UI that started playback; later changes only redraw it.
      !running -> try {
        ContextCompat.startForegroundService(context, service)
      } catch (e: IllegalStateException) {
        // Android 12+ refuses while the app is in the background (playback started remotely):
        // it plays without the notification until the app is next opened (MediaPlugin.onResume).
        Log.w("Motif", "can't start the playback service", e)
      }
      else -> refresh(context)
    }
  }

  private fun refresh(context: Context) {
    if (running) context.getSystemService(NotificationManager::class.java).notify(NOTIFICATION, notification(context))
  }

  fun notification(context: Context): Notification {
    context.getSystemService(NotificationManager::class.java).createNotificationChannel(
      NotificationChannel(CHANNEL, "Playback", NotificationManager.IMPORTANCE_LOW)
    )
    val playing = state == PlaybackStateCompat.STATE_PLAYING
    fun button(icon: Int, label: String, command: String) = NotificationCompat.Action(
      icon, label,
      PendingIntent.getService(
        context, command.hashCode(),
        Intent(context, PlaybackService::class.java).setAction(command), PendingIntent.FLAG_IMMUTABLE
      )
    )
    return NotificationCompat.Builder(context, CHANNEL)
      .setSmallIcon(android.R.drawable.ic_media_play)
      .setContentTitle(title)
      .setContentText(artist)
      .setLargeIcon(art)
      .setContentIntent(
        PendingIntent.getActivity(context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
      )
      .setVisibility(NotificationCompat.VISIBILITY_PUBLIC)
      .setOnlyAlertOnce(true)
      .setOngoing(true)
      .addAction(button(android.R.drawable.ic_media_previous, "Previous", "previous"))
      .addAction(
        if (playing) button(android.R.drawable.ic_media_pause, "Pause", "pause")
        else button(android.R.drawable.ic_media_play, "Play", "play")
      )
      .addAction(button(android.R.drawable.ic_media_next, "Next", "next"))
      .setStyle(MediaStyle().setMediaSession(session(context).sessionToken).setShowActionsInCompactView(0, 1, 2))
      .build()
  }
}

/** Keeps the app in the foreground while [Playback] has something to play. */
class PlaybackService : Service() {
  override fun onCreate() {
    super.onCreate()
    Playback.running = true
  }

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    ServiceCompat.startForeground(
      this, Playback.NOTIFICATION, Playback.notification(this),
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else 0
    )
    // Notification buttons.
    intent?.action?.let(Playback::send)
    // Without the engine (the process died) there is nothing to restart.
    return START_NOT_STICKY
  }

  override fun onDestroy() {
    Playback.running = false
    super.onDestroy()
  }

  override fun onBind(intent: Intent?): IBinder? = null
}

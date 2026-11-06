package io.github.mgwabel.motif

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.database.ContentObserver
import android.graphics.BitmapFactory
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioManager
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class ListenArgs {
  lateinit var channel: Channel
}

@InvokeArg
class VolumeArgs {
  var index: Int = 0
}

/** The Rust side is `apps/desktop/src/media.rs`; updates are `integrations::media_controls::Update`. */
@TauriPlugin
class MediaPlugin(private val activity: Activity) : Plugin(activity) {
  private val main = Handler(Looper.getMainLooper())

  override fun onResume(activity: AppCompatActivity) {
    Background.visible(true)
    Playback.sync(activity.applicationContext)
  }

  override fun onStop(activity: AppCompatActivity) = Background.visible(false)

  /** Where "hold background work" goes, as `{ held }` (now, then on each change). */
  @Command
  fun listenBackground(invoke: Invoke) {
    Background.listen(invoke.parseArgs(ListenArgs::class.java).channel)
    invoke.resolve()
  }

  /** Where the media buttons go, as engine commands. */
  @Command
  fun listen(invoke: Invoke) {
    Playback.buttons = invoke.parseArgs(ListenArgs::class.java).channel
    invoke.resolve()
  }

  /** Where audio focus changes and unplugged headphones go, as `crate::focus::Change`. */
  @Command
  fun listenFocus(invoke: Invoke) {
    val channel = invoke.parseArgs(ListenArgs::class.java).channel
    val context = activity.applicationContext
    main.post { Focus.listen(context, channel) }
    invoke.resolve()
  }

  /** Where media volume changes go, as `crate::volume::System` (now, then on each change). */
  @Command
  fun listenVolume(invoke: Invoke) {
    val channel = invoke.parseArgs(ListenArgs::class.java).channel
    val context = activity.applicationContext
    main.post { Volume.listen(context, channel) }
    invoke.resolve()
  }

  /** Sets the media volume step, without showing the system volume panel. */
  @Command
  fun setVolume(invoke: Invoke) {
    val index = invoke.parseArgs(VolumeArgs::class.java).index
    Volume.audio(activity.applicationContext).setStreamVolume(AudioManager.STREAM_MUSIC, index, 0)
    invoke.resolve()
  }

  @Command
  fun show(invoke: Invoke) {
    val update = invoke.getArgs()
    val context = activity.applicationContext
    if (update.getString("type") == "metadata") {
      // Decoded here, off the main thread.
      val art = update.optStringOrNull("cover")
        ?.let { Uri.parse(it).path }
        ?.let(BitmapFactory::decodeFile)
      val title = update.optStringOrNull("title")
      val artist = update.optStringOrNull("artist")
      val album = update.optStringOrNull("album")
      val duration = if (update.isNull("duration_ms")) -1L else update.getLong("duration_ms")
      main.post { Playback.showTrack(context, title, artist, album, duration, art) }
    } else {
      val status = update.getString("status")
      val position = update.getLong("position_ms")
      main.post { Playback.showStatus(context, status, position) }
    }
    invoke.resolve()
  }

  private fun org.json.JSONObject.optStringOrNull(key: String): String? =
    if (isNull(key)) null else getString(key)
}

/** The media volume, which is the phone engine's volume; the Rust side is `apps/desktop/src/media.rs`. */
object Volume {
  private var observer: ContentObserver? = null
  private var sent = -1

  fun audio(context: Context) = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager

  /** On the main thread. Volume keys, the system panel and other apps all change it in `Settings.System`. */
  fun listen(context: Context, channel: Channel) {
    val audio = audio(context)
    fun report() {
      val index = audio.getStreamVolume(AudioManager.STREAM_MUSIC)
      if (index == sent) return
      sent = index
      channel.send(JSObject().put("index", index).put("max", audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)))
    }
    observer?.let(context.contentResolver::unregisterContentObserver)
    observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
      override fun onChange(selfChange: Boolean) = report()
    }.also { context.contentResolver.registerContentObserver(Settings.System.CONTENT_URI, true, it) }
    sent = -1
    report()
  }
}

/**
 * Audio focus, held from playing until stopped, and headphones or Bluetooth
 * audio disconnecting. Reported to `apps/desktop/src/focus.rs`, which pauses,
 * resumes and ducks (Android doesn't duck AAudio streams itself).
 */
object Focus {
  private var channel: Channel? = null
  private var noisy: BroadcastReceiver? = null
  private val request = AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
    .setAudioAttributes(
      AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build()
    )
    .setOnAudioFocusChangeListener { change ->
      when (change) {
        AudioManager.AUDIOFOCUS_GAIN -> "gain"
        AudioManager.AUDIOFOCUS_LOSS -> "loss"
        AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> "loss_transient"
        AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> "loss_transient_can_duck"
        else -> null
      }?.let(::send)
    }
    .build()

  private fun send(change: String) {
    channel?.sendObject(change)
  }

  /** On the main thread. */
  fun listen(context: Context, channel: Channel) {
    this.channel = channel
    if (noisy != null) return
    noisy = object : BroadcastReceiver() {
      override fun onReceive(context: Context, intent: Intent) = send("noisy")
    }.also {
      ContextCompat.registerReceiver(
        context, it, IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY), ContextCompat.RECEIVER_NOT_EXPORTED
      )
    }
  }

  /**
   * On starting to play. Granted, it counts as a gain (Android reports none), which ends a duck;
   * during a call Android refuses, and the music pauses again.
   */
  fun request(context: Context) {
    when (Volume.audio(context).requestAudioFocus(request)) {
      AudioManager.AUDIOFOCUS_REQUEST_GRANTED -> send("gain")
      AudioManager.AUDIOFOCUS_REQUEST_FAILED -> Playback.sendLocal("pause")
    }
  }

  fun abandon(context: Context) {
    Volume.audio(context).abandonAudioFocusRequest(request)
  }
}

/**
 * Whether the library should hold its background work (hashing, thumbnails):
 * while the app is out of sight and [PlaybackService] isn't keeping it in the
 * foreground, Android kills it for using the CPU. The Rust side is `apps/desktop/src/media.rs`.
 */
object Background {
  private var channel: Channel? = null
  private var visible = true
  private var sent: Boolean? = null

  @Synchronized fun listen(channel: Channel) {
    this.channel = channel
    sent = null
    report()
  }

  @Synchronized fun visible(now: Boolean) {
    visible = now
    report()
  }

  /** Call when [visible] or [Playback.running] changes. */
  @Synchronized fun report() {
    val held = !visible && !Playback.running
    val channel = channel ?: return
    if (held == sent) return
    sent = held
    channel.send(JSObject().put("held", held))
  }
}

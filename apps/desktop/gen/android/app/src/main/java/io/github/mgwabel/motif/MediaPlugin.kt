package io.github.mgwabel.motif

import android.app.Activity
import android.graphics.BitmapFactory
import android.net.Uri
import android.os.Handler
import android.os.Looper
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Channel
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

@InvokeArg
class ListenArgs {
  lateinit var channel: Channel
}

/** The Rust side is `apps/desktop/src/media.rs`; updates are `integrations::media_controls::Update`. */
@TauriPlugin
class MediaPlugin(private val activity: Activity) : Plugin(activity) {
  private val main = Handler(Looper.getMainLooper())

  override fun onResume() = Playback.sync(activity.applicationContext)

  /** Where the media buttons go, as engine commands. */
  @Command
  fun listen(invoke: Invoke) {
    Playback.buttons = invoke.parseArgs(ListenArgs::class.java).channel
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

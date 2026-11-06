package io.github.mgwabel.motif

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.util.Log
import android.view.KeyEvent
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

class MainActivity : TauriActivity() {
  private var multicastLock: WifiManager.MulticastLock? = null
  private var webView: WebView? = null
  /** Set by the UI while a remote engine is current (`apps/ui/src/lib/volumeKeys.ts`). */
  @Volatile private var volumeKeys = false
  /** Remote engine updates, in order, off the UI's thread: artwork downloads. */
  private val remoteMedia = Executors.newSingleThreadExecutor()
  /** The last remote artwork, by URL; only on [remoteMedia]. */
  private var cover: Pair<String, Bitmap?>? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onDestroy() {
    // Nothing is left to send its buttons to.
    Playback.endRemote(applicationContext)
    multicastLock?.release()
    multicastLock = null
    super.onDestroy()
  }

  /** While [volumeKeys], the volume keys go to the UI (each press and repeat) instead of the phone's volume. */
  override fun dispatchKeyEvent(event: KeyEvent): Boolean {
    val direction = when (event.keyCode) {
      KeyEvent.KEYCODE_VOLUME_UP -> 1
      KeyEvent.KEYCODE_VOLUME_DOWN -> -1
      else -> 0
    }
    if (!volumeKeys || direction == 0) return super.dispatchKeyEvent(event)
    if (event.action == KeyEvent.ACTION_DOWN) webView?.evaluateJavascript("window.motifVolumeKey?.($direction)", null)
    return true
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    webView.addJavascriptInterface(Storage(webView), "MotifAndroid")
  }

  /** `window.MotifAndroid` in the UI (`apps/ui/src/lib/folders.ts`). */
  inner class Storage(private val webView: WebView) {
    @JavascriptInterface
    fun hasAllFilesAccess(): Boolean =
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) Environment.isExternalStorageManager()
      else checkSelfPermission(Manifest.permission.READ_EXTERNAL_STORAGE) == PackageManager.PERMISSION_GRANTED

    @JavascriptInterface
    fun openAllFilesSettings() = runOnUiThread {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        startActivity(
          Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName"))
        )
      } else {
        requestPermissions(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)) {}
      }
    }

    /** Wi-Fi drops multicast (mDNS) packets unless this is held; the UI holds it only while looking for engines. */
    @JavascriptInterface
    fun setMulticast(on: Boolean) {
      if (on && multicastLock == null) {
        val wifi = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
        multicastLock = wifi.createMulticastLock("motif-mdns").apply { acquire() }
      } else if (!on) {
        multicastLock?.release()
        multicastLock = null
      }
    }

    @JavascriptInterface
    fun setVolumeKeys(on: Boolean) {
      volumeKeys = on
    }

    /** A remote engine's playback for the notification (`apps/ui/src/lib/remoteMedia.ts`), or null for the phone's. */
    @JavascriptInterface
    fun setRemoteMedia(json: String?) = remoteMedia.execute {
      val media = json?.let(::JSONObject)
      val url = media?.takeUnless { it.isNull("cover") }?.getString("cover")
      val known = cover
      // A failed download stays failed for that track, so updates don't each wait for it again.
      val art = if (url == null) null else if (known?.first == url) known.second else download(url).also { cover = url to it }
      runOnUiThread {
        if (isDestroyed) return@runOnUiThread
        if (media == null) Playback.endRemote(applicationContext)
        else Playback.showRemote(applicationContext, media, art) { command ->
          webView.evaluateJavascript("window.motifRemoteButton?.($command)", null)
        }
      }
    }

    /** What back does on the root screen since Android 12: the task goes behind, the page and playback stay. */
    @JavascriptInterface
    fun moveToBack() = runOnUiThread { moveTaskToBack(true) }

    @JavascriptInterface
    fun setBarIconsDark(dark: Boolean) = runOnUiThread {
      WindowCompat.getInsetsController(window, window.decorView).apply {
        isAppearanceLightStatusBars = dark
        isAppearanceLightNavigationBars = dark
      }
    }

    private fun download(url: String): Bitmap? = try {
      (URL(url).openConnection() as HttpURLConnection).run {
        connectTimeout = 5000
        readTimeout = 5000
        try { inputStream.use(BitmapFactory::decodeStream) } finally { disconnect() }
      }
    } catch (e: IOException) {
      Log.w("Motif", "can't fetch the remote engine's artwork", e)
      null
    }

    @JavascriptInterface
    fun pickFolder() = runOnUiThread {
      launchActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)) { result ->
        val uri = result?.data?.data?.toString()
        val arg = if (uri == null) "null" else JSONObject.quote(uri)
        webView.evaluateJavascript("window.motifFolderPicked?.($arg)", null)
      }
    }
  }
}

package io.github.mgwabel.motif

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ShortcutInfo
import android.content.pm.ShortcutManager
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.drawable.Icon
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.util.Log
import android.view.KeyEvent
import android.view.ViewGroup
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.BackEventCompat
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import androidx.lifecycle.Lifecycle
import org.json.JSONObject
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors

class MainActivity : TauriActivity() {
  private var multicastLock: WifiManager.MulticastLock? = null
  private var webView: WebView? = null
  /** The renderer died while in the background; see [onRendererGone]. */
  private var rendererGone = false
  /** Set by the UI while a remote engine is current (`apps/ui/src/lib/volumeKeys.ts`). */
  @Volatile private var volumeKeys = false
  /** Remote engine updates, in order, off the UI's thread: artwork downloads. */
  private val remoteMedia = Executors.newSingleThreadExecutor()
  /** The last remote artwork, by URL; only on [remoteMedia]. */
  private var cover: Pair<String, Bitmap?>? = null
  /**
   * A motif:// link, until the UI takes it (`apps/ui/src/App.svelte`): a launcher shortcut's view
   * ([Storage.pinShortcut]) or a host's pairing page.
   */
  @Volatile private var link: String? = null
  /** Shows the system back swipe in the page; see [Storage.followBack]. */
  private var backSwipe: BackSwipe? = null

  /**
   * Predictive back: tells the page how far the system back swipe has come, so its screen
   * follows it (`window.motifBack`, `apps/ui/src/lib/back.ts`). Letting go passes the press on
   * to Tauri's own back handler, which the page answers as it does a back button.
   */
  private inner class BackSwipe : OnBackPressedCallback(true) {
    override fun handleOnBackStarted(backEvent: BackEventCompat) = page("start()")
    override fun handleOnBackProgressed(backEvent: BackEventCompat) = page("progress(${backEvent.progress})")
    override fun handleOnBackCancelled() = page("cancel()")
    override fun handleOnBackPressed() {
      isEnabled = false
      onBackPressedDispatcher.onBackPressed()
      isEnabled = true
    }

    private fun page(call: String) {
      webView?.evaluateJavascript("window.motifBack?.$call", null)
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    // Not when recreated: that intent was taken already.
    if (savedInstanceState == null) keepLink(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    if (keepLink(intent)) webView?.evaluateJavascript("window.motifLinkArrived?.()", null)
  }

  private fun keepLink(intent: Intent?): Boolean {
    val data = intent?.data?.takeIf { intent.action == Intent.ACTION_VIEW && it.scheme == "motif" } ?: return false
    link = data.toString()
    return true
  }

  /**
   * Android killed or crashed the WebView's renderer (`.cargo/config.toml` routes wry's client here).
   * The dead WebView goes, and the activity is recreated, which wry treats as a configuration change:
   * Tauri and the engine stay, the page loads again in a new WebView, and [PlaybackService] keeps
   * the music going meanwhile. In the background that waits for [onStart], so a renderer Android
   * killed to free memory isn't started again straight away.
   */
  fun onRendererGone(view: WebView): Boolean {
    Log.w("Motif", "WebView renderer gone; recreating the activity")
    (view.parent as? ViewGroup)?.removeView(view)
    view.destroy()
    webView = null
    if (lifecycle.currentState.isAtLeast(Lifecycle.State.STARTED)) recreate() else rendererGone = true
    return true
  }

  override fun onStart() {
    super.onStart()
    if (rendererGone) recreate()
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
    // The page applies the system font scale itself (`fontScale()`), as part of its zoom: the
    // WebView's own text zoom grows text alone, so what the page measures isn't what it draws.
    webView.settings.textZoom = 100
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
    fun takeLink(): String? = link.also { link = null }

    /**
     * Asks the launcher to pin a shortcut that opens `link` (`apps/ui/src/lib/launcher.ts`), with
     * the artwork at `cover` as its icon, else the app's. False if the launcher can't pin any.
     */
    @JavascriptInterface
    fun pinShortcut(link: String, label: String, cover: String?): Boolean {
      val shortcuts = getSystemService(ShortcutManager::class.java)
      if (!shortcuts.isRequestPinShortcutSupported) return false
      // Off the UI's thread for the artwork download, as remote artwork.
      remoteMedia.execute {
        val art = cover?.let(::download)
        val icon = art?.let(Icon::createWithBitmap) ?: Icon.createWithResource(this@MainActivity, R.mipmap.ic_launcher)
        val open = Intent(Intent.ACTION_VIEW, Uri.parse(link), this@MainActivity, MainActivity::class.java)
        // The link is the id: pinning a view again updates its shortcut.
        val shortcut = ShortcutInfo.Builder(this@MainActivity, link).setShortLabel(label).setIcon(icon).setIntent(open).build()
        runOnUiThread { if (!isDestroyed) shortcuts.requestPinShortcut(shortcut, null) }
      }
      return true
    }

    /** Added when the page asks: Tauri's back handler is in place by then, and the last added gets the swipe first. */
    @JavascriptInterface
    fun followBack() = runOnUiThread {
      if (backSwipe == null) backSwipe = BackSwipe().also { onBackPressedDispatcher.addCallback(this@MainActivity, it) }
    }

    /** Android's font size setting (1 is the default), which the page's zoom includes. */
    @JavascriptInterface
    fun fontScale(): Float = resources.configuration.fontScale

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

package io.github.mgwabel.motif

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.view.KeyEvent
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import org.json.JSONObject

class MainActivity : TauriActivity() {
  private var multicastLock: WifiManager.MulticastLock? = null
  private var webView: WebView? = null
  /** Set by the UI while a remote engine is current (`apps/ui/src/lib/volumeKeys.ts`). */
  @Volatile private var volumeKeys = false

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onDestroy() {
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

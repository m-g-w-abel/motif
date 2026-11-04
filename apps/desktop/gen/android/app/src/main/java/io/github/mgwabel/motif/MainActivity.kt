package io.github.mgwabel.motif

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowCompat
import org.json.JSONObject

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }

  override fun onWebViewCreate(webView: WebView) {
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

package com.storytellerf.llmd

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import androidx.core.view.WindowInsetsControllerCompat
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.json.JSONObject

class MainActivity : TauriActivity() {
  private val activityScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
  private var webView: WebView? = null
  private val importModel = registerForActivityResult(object : ActivityResultContracts.OpenDocument() {
    override fun createIntent(context: Context, input: Array<String>): Intent =
      super.createIntent(context, input).putExtra(DocumentsContract.EXTRA_INITIAL_URI, DOWNLOADS_URI)
  }) { uri ->
    if (uri == null) {
      emitModelMutation("cancelled")
      return@registerForActivityResult
    }

    activityScope.launch {
      val result = runCatching {
        require(isLiteRtModelFile(uri)) { "Select a .litertlm model file" }
        LlmdAndroidBridge.replaceDefaultModel(this@MainActivity, uri)
      }
      emitModelMutation(
        status = if (result.isSuccess) "imported" else "error",
        error = result.exceptionOrNull()?.message,
      )
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    LlmdAndroidBridge.configure(this)
    super.onCreate(savedInstanceState)
  }

  override fun onStart() {
    super.onStart()
    // App status bar background is dark, so force light (light content) status bar icons.
    WindowInsetsControllerCompat(window, window.decorView).apply {
      isAppearanceLightStatusBars = false
      isAppearanceLightNavigationBars = false
    }
  }

  override fun onWebViewCreate(webView: WebView) {
    this.webView = webView
    webView.addJavascriptInterface(ModelImportBridge(), "llmdAndroid")
  }

  override fun onDestroy() {
    activityScope.cancel()
    super.onDestroy()
  }

  private fun emitModelMutation(status: String, error: String? = null) {
    val detail = JSONObject(LlmdAndroidBridge.modelStateJson())
      .put("status", status)
      .put("error", error)
      .toString()
    val script = "window.dispatchEvent(new CustomEvent('llmd-models-changed',{detail:$detail}))"
    webView?.post { webView?.evaluateJavascript(script, null) }
  }

  private fun isLiteRtModelFile(uri: Uri): Boolean {
    val displayName = contentResolver.query(
      uri,
      arrayOf(OpenableColumns.DISPLAY_NAME),
      null,
      null,
      null,
    )?.use { cursor ->
      if (!cursor.moveToFirst()) return@use null
      cursor.getString(cursor.getColumnIndexOrThrow(OpenableColumns.DISPLAY_NAME))
    }
    return displayName?.endsWith(MODEL_FILE_EXTENSION, ignoreCase = true) == true
  }

  inner class ModelImportBridge {
    @JavascriptInterface
    fun importDefaultModel() {
      runOnUiThread {
        importModel.launch(
          arrayOf(
            "application/octet-stream",
          ),
        )
      }
    }

    @JavascriptInterface
    fun deleteModel(model: String) {
      activityScope.launch {
        val result = runCatching { LlmdAndroidBridge.deleteModel(model) }
        emitModelMutation(
          status = if (result.isSuccess) "deleted" else "error",
          error = result.exceptionOrNull()?.message,
        )
      }
    }

    @JavascriptInterface
    fun getModelState(): String = LlmdAndroidBridge.modelStateJson()

    @JavascriptInterface
    fun getHealthState(): String = LlmdAndroidBridge.healthJson()
  }

  private companion object {
    const val MODEL_FILE_EXTENSION = ".litertlm"
    val DOWNLOADS_URI: Uri = Uri.parse(
      "content://com.android.externalstorage.documents/document/primary%3ADownload",
    )
  }
}

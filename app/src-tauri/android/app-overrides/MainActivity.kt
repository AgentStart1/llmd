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
import androidx.core.content.ContextCompat
import kotlinx.coroutines.Job
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.withContext
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.json.JSONObject

class MainActivity : TauriActivity() {
  private val activityScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
  private var webView: WebView? = null
  private var statusJob: Job? = null
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
        withContext(Dispatchers.IO) {
          val name = modelFileName(uri)
          require(name?.endsWith(MODEL_FILE_EXTENSION, ignoreCase = true) == true) { "Select a .litertlm model file" }
          LlmdAndroidBridge.importModel(this@MainActivity, uri, requireNotNull(name))
        }
      }
      result.exceptionOrNull()?.let { if (it is CancellationException) throw it }
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
    statusJob = activityScope.launch {
      LlmdRuntime.host.state.collect {
        val detail = LlmdAndroidBridge.healthJson()
        webView?.evaluateJavascript("window.dispatchEvent(new CustomEvent('llmd-service-changed',{detail:$detail}))", null)
      }
    }
  }

  override fun onStop() {
    statusJob?.cancel()
    super.onStop()
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

  private fun modelFileName(uri: Uri): String? = contentResolver.query(
      uri,
      arrayOf(OpenableColumns.DISPLAY_NAME),
      null,
      null,
      null,
    )?.use { cursor ->
      if (!cursor.moveToFirst()) return@use null
      cursor.getString(cursor.getColumnIndexOrThrow(OpenableColumns.DISPLAY_NAME))
    }

  inner class ModelImportBridge {
    @JavascriptInterface
    fun importModel() {
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

    @JavascriptInterface
    fun setServiceEnabled(enabled: Boolean, model: String) {
      runOnUiThread {
        val intent = Intent(this@MainActivity, LlmdModelService::class.java)
          .setAction(if (enabled) LlmdModelService.ACTION_START else LlmdModelService.ACTION_STOP)
          .putExtra(LlmdModelService.EXTRA_MODEL, model)
        if (enabled) ContextCompat.startForegroundService(this@MainActivity, intent)
        else startService(intent)
      }
    }
  }

  private companion object {
    const val MODEL_FILE_EXTENSION = ".litertlm"
    val DOWNLOADS_URI: Uri = Uri.parse(
      "content://com.android.externalstorage.documents/document/primary%3ADownload",
    )
  }
}

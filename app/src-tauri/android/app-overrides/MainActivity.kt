package com.storytellerf.llmd

import android.content.Intent
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.enableEdgeToEdge
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONObject

class MainActivity : TauriActivity() {
  private val activityScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
  private var webView: WebView? = null
  private val importModel = registerForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
    if (uri == null) {
      emitModelImport("cancelled")
      return@registerForActivityResult
    }

    activityScope.launch {
      val result = runCatching { copyDefaultModel(uri) }
      emitModelImport(
        status = if (result.isSuccess) "imported" else "error",
        error = result.exceptionOrNull()?.message,
      )
    }
  }

  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    LlmdAndroidBridge.configure(this)
    super.onCreate(savedInstanceState)
    val intent = Intent(this, LlmdForegroundService::class.java)
      .setAction(LlmdForegroundService.ACTION_START)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      startForegroundService(intent)
    } else {
      startService(intent)
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

  private suspend fun copyDefaultModel(uri: Uri): File = withContext(Dispatchers.IO) {
    val destination = LlmdAndroidBridge.defaultModelFile(this@MainActivity)
    val destinationDir = requireNotNull(destination.parentFile) { "Model directory is unavailable" }
    val temp = File(destinationDir, "${destination.name}.tmp")
    destinationDir.mkdirs()

    contentResolver.openInputStream(uri).use { input ->
      requireNotNull(input) { "Unable to open selected model file" }
      temp.outputStream().use { output ->
        input.copyTo(output)
      }
    }

    require(temp.length() > 0L) { "Selected model file is empty" }
    if (destination.exists()) destination.delete()
    require(temp.renameTo(destination)) { "Unable to save imported model" }
    destination
  }

  private fun emitModelImport(status: String, error: String? = null) {
    val detail = JSONObject(LlmdAndroidBridge.modelStateJson())
      .put("status", status)
      .put("error", error)
      .toString()
    val script = "window.dispatchEvent(new CustomEvent('llmd-models-changed',{detail:$detail}))"
    webView?.post { webView?.evaluateJavascript(script, null) }
  }

  inner class ModelImportBridge {
    @JavascriptInterface
    fun importDefaultModel() {
      runOnUiThread {
        importModel.launch(
          arrayOf(
            "application/octet-stream",
            "application/vnd.litertlm",
            "*/*",
          ),
        )
      }
    }

    @JavascriptInterface
    fun getModelState(): String = LlmdAndroidBridge.modelStateJson()
  }
}

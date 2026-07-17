package com.storytellerf.llmd

import android.app.Activity
import android.os.Bundle
import android.widget.Button
import android.widget.TextView
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class LlmdIpcAuthorizationActivity : Activity() {
    private val activityScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val callerPackage = intent.getStringExtra(LlmdIpcAuthorization.EXTRA_CALLER_PACKAGE).orEmpty()
        if (callerPackage.isBlank()) {
            setResult(RESULT_CANCELED)
            finish()
            return
        }

        val callerLabel = LlmdIpcAuthorization.callerLabel(this, callerPackage)
        val callerDigest = LlmdIpcAuthorization.displayDigest(this, callerPackage)
        setContentView(R.layout.activity_ipc_authorization)

        findViewById<TextView>(R.id.ipc_authorization_caller_name).text = callerLabel
        findViewById<TextView>(R.id.ipc_authorization_package).text = callerPackage
        findViewById<TextView>(R.id.ipc_authorization_signature).text =
            callerDigest.ifBlank { getString(R.string.ipc_authorization_signature_unavailable) }

        val allowButton = findViewById<Button>(R.id.ipc_authorization_allow)
        allowButton.setOnClickListener {
            allowButton.isEnabled = false
            activityScope.launch {
                val authorized = withContext(Dispatchers.IO) {
                    LlmdIpcAuthorization.authorizePackage(this@LlmdIpcAuthorizationActivity, callerPackage)
                }
                setResult(if (authorized) RESULT_OK else RESULT_CANCELED)
                finish()
            }
        }

        findViewById<Button>(R.id.ipc_authorization_cancel).setOnClickListener {
            setResult(RESULT_CANCELED)
            finish()
        }
    }

    override fun onDestroy() {
        activityScope.cancel()
        super.onDestroy()
    }
}

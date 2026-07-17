package com.storytellerf.llmd

object LlmdNativeServer {
    init {
        System.loadLibrary("llmd_app")
    }

    external fun startServer(): Boolean
    external fun stopServer()
    external fun completeChatCompletion(requestId: Long, response: String?, error: String?)
}

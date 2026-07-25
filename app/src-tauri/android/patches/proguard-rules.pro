# Android WebView JavaScript bridge methods are invoked by name from the bundled UI.
-keepclassmembers class com.storytellerf.llmd.MainActivity$ModelImportBridge {
    @android.webkit.JavascriptInterface <methods>;
}

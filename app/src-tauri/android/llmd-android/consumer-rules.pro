# Rust JNI entry points and callbacks use these class, field, and method names directly.
-keep class com.storytellerf.llmd.LlmdNativeServer {
    public static final com.storytellerf.llmd.LlmdNativeServer INSTANCE;
    public native <methods>;
}

-keep class com.storytellerf.llmd.LlmdAndroidBridge {
    public static final com.storytellerf.llmd.LlmdAndroidBridge INSTANCE;
    public java.lang.String listModelsJson();
    public void chatCompletionAsync(long, java.lang.String);
}

# litertlm-android's native library resolves its Kotlin API classes and getters by JNI name.
-keep class com.google.ai.edge.litertlm.** {
    *;
}

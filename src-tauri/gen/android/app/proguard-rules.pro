# Add project specific ProGuard rules here.
# You can control the set of applied configuration files using the
# proguardFiles setting in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# If your project uses WebView with JS, uncomment the following
# and specify the fully qualified class name to the JavaScript interface
# class:
#-keepclassmembers class fqcn.of.javascript.interface.for.webview {
#   public *;
#}

# Uncomment this to preserve the line number information for
# debugging stack traces.
#-keepattributes SourceFile,LineNumberTable

# If you keep the line number information, uncomment this to
# hide the original source file name.
#-renamesourcefileattribute SourceFile

# Native video (2026-09-29): Rust's JNI entry (src-tauri/src/video_surfaces.rs) is found by the
# names of this class and its native method.
-keep class com.flickertalk.platform.FtVideoSurfaces {
    native <methods>;
}

# A call push starts the core with the app closed (2026-10-01): Rust's JNI entries
# (src-tauri/src/push_core.rs) are found by this class's name, and Rust calls its static functions
# back by name.
-keep class com.flickertalk.platform.PushCore {
    native <methods>;
    public static *;
}

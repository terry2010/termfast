# Keep JNI native methods
-keepclasseswithmembernames class * {
    native <methods>;
}

# Keep RustBridge class and its native methods
-keep class com.termfast.app.RustBridge {
    native <methods>;
    <fields>;
    <methods>;
}
-keep interface com.termfast.app.RustEventListener { *; }
-keep class com.termfast.app.MainActivity { *; }
-keep class com.termfast.app.service.** { *; }

# Keep model classes and their serialization members
-keep class com.termfast.app.data.** { *; }
-keepclassmembers class com.termfast.app.data.** {
    @kotlinx.serialization.SerialName <fields>;
    @kotlinx.serialization.Serializable <fields>;
    <init>(...);
}
-keepattributes *Annotation*, Signature, RuntimeVisibleAnnotations, RuntimeVisibleParameterAnnotations

# Keep Kotlin serialization
-keep class kotlinx.serialization.** { *; }
-keepclassmembers class kotlinx.serialization.** { *; }
-dontwarn kotlinx.serialization.**

# Keep Compose and Material3
-keep class androidx.compose.** { *; }
-keepclassmembers class androidx.compose.** { *; }
-dontwarn androidx.compose.**

# Keep Android lifecycle / navigation
-keep class androidx.lifecycle.** { *; }
-keep class androidx.navigation.** { *; }
-dontwarn androidx.lifecycle.**
-dontwarn androidx.navigation.**

# Tink / EncryptedSharedPreferences missing classes
-dontwarn com.google.errorprone.annotations.**
-dontwarn javax.annotation.**
-dontwarn com.google.api.client.http.**
-dontwarn com.google.api.client.json.**
-dontwarn org.joda.time.**
-keep class com.google.crypto.tink.** { *; }
-keep class androidx.security.crypto.** { *; }

import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

android {
    namespace = "com.termfast.app"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.termfast.app"
        minSdk = 26
        targetSdk = 36
        versionCode = 32
        // Read versionName from workspace Cargo.toml to keep in sync
        val cargoVersion = File(rootDir.parentFile, "Cargo.toml").readText()
            .lines().firstOrNull { it.trim().startsWith("version =") }
            ?.substringAfter("\"")?.substringBefore("\"") ?: "0.0.0"
        versionName = cargoVersion

        ndk {
            abiFilters += listOf("arm64-v8a")
        }

        externalNativeBuild {
            cmake {
                cppFlags("")
            }
        }
    }

    signingConfigs {
        create("release") {
            storeFile = file("keystores/release.keystore")
            // Read signing passwords from local.properties (gitignored) or
            // environment variables. Only fail if a release build is actually
            //   requested — debug builds should not require signing config.
            val localProps = Properties().also { props ->
                val localPropsFile = rootProject.file("local.properties")
                if (localPropsFile.exists()) {
                    props.load(localPropsFile.inputStream())
                }
            }
            val isReleaseBuild = gradle.startParameter.taskNames.any {
                it.contains("Release", ignoreCase = true)
            }
            storePassword = localProps.getProperty("TERMFAST_STORE_PASSWORD")
                ?.takeIf { it.isNotBlank() }
                ?: System.getenv("TERMFAST_STORE_PASSWORD")
                ?: "termfast".also {
                    if (isReleaseBuild) logger.warn("TERMFAST_STORE_PASSWORD not set — using default. Set GitHub Secret for production.")
                }
            keyAlias = localProps.getProperty("TERMFAST_KEY_ALIAS")
                ?.takeIf { it.isNotBlank() }
                ?: System.getenv("TERMFAST_KEY_ALIAS")
                ?: "termfast".also {
                    if (isReleaseBuild) logger.warn("TERMFAST_KEY_ALIAS not set — using default. Set GitHub Secret for production.")
                }
            keyPassword = localProps.getProperty("TERMFAST_KEY_PASSWORD")
                ?.takeIf { it.isNotBlank() }
                ?: System.getenv("TERMFAST_KEY_PASSWORD")
                ?: "termfast".also {
                    if (isReleaseBuild) logger.warn("TERMFAST_KEY_PASSWORD not set — using default. Set GitHub Secret for production.")
                }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            isDebuggable = false
            signingConfig = signingConfigs.getByName("release")
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
        debug {
            isMinifyEnabled = false
            isDebuggable = true
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    // Mock Android framework calls (e.g. android.util.Log) in unit tests
    // instead of throwing RuntimeException. Required for tests that exercise
    // code paths calling Log.i/w/e (e.g. RemoteTunnelManager callbacks).
    testOptions {
        unitTests {
            isReturnDefaultValues = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
}

// === Rust .so auto-build integration ===
// Prevents stale .so issues: Gradle automatically runs `cargo build` and
// copies the fresh .so into jniLibs before every Android build.
// To skip (e.g. during pure Kotlin iteration), pass -PskipCargoBuild.
val skipCargoBuild = project.hasProperty("skipCargoBuild")
val projectRoot = rootProject.projectDir.parentFile!!
val rustTargetDir = File(projectRoot, "target")
val cargoToml = File(projectRoot, "Cargo.toml")
val abi = "arm64-v8a"
val rustTargetTriple = "aarch64-linux-android"

// Output .so paths
val debugSo = File(rustTargetDir, "$rustTargetTriple/debug/libtermfast_android_ffi.so")
val releaseSo = File(rustTargetDir, "$rustTargetTriple/release/libtermfast_android_ffi.so")
val jniLibDir = File(projectDir, "src/main/jniLibs/$abi")
val jniLibSo = File(jniLibDir, "libtermfast_android_ffi.so")

val isReleaseBuild = gradle.startParameter.taskNames.any {
    it.contains("Release", ignoreCase = true)
}

val cargoBuildTask = tasks.register<Exec>("cargoBuildNative") {
    group = "rust"
    description = "Compile Rust .so for Android (auto-invoked before preBuild)"
    onlyIf { !skipCargoBuild }

    // Re-run if Cargo.toml or any Rust source changes (cargo itself also
    // does incremental compilation, so this is just a Gradle-level guard).
    inputs.file(cargoToml).withPropertyName("cargoToml")
    inputs.dir(File(projectRoot, "crates"))
        .withPropertyName("rustSources")
    outputs.file(if (isReleaseBuild) releaseSo else debugSo)
        .withPropertyName("rustSo")

    workingDir = rootProject.projectDir.parentFile
    val cargoArgs = if (isReleaseBuild) {
        listOf("build", "--release", "--target", rustTargetTriple, "-p", "termfast-android-ffi")
    } else {
        listOf("build", "--target", rustTargetTriple, "-p", "termfast-android-ffi")
    }
    commandLine("cargo", *cargoArgs.toTypedArray())

    // Suppress stdout on incremental no-op builds; show on actual compile
    isIgnoreExitValue = false
}

val copyNativeLibTask = tasks.register<Copy>("copyNativeLib") {
    group = "rust"
    description = "Copy fresh .so into jniLibs (auto-invoked after cargoBuildNative)"
    onlyIf { !skipCargoBuild }
    dependsOn(cargoBuildTask)

    from(if (isReleaseBuild) releaseSo else debugSo)
    into(jniLibDir)
    rename { "libtermfast_android_ffi.so" }
}

// Hook into Android build lifecycle: run before preBuild so the .so is
// in place before the APK packaging step.
tasks.named("preBuild") {
    dependsOn(copyNativeLibTask)
}
// === Rust .so auto-build integration END ===

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2026.06.00")
    implementation(composeBom)

    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.activity:activity-compose:1.9.3")

    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-graphics")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")

    implementation("androidx.navigation:navigation-compose:2.8.5")

    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")

    // Encrypted credential key caching (Android Keystore-backed)
    implementation("androidx.security:security-crypto:1.1.0-alpha06")

    // Terminal emulator (ConnectBot termlib — libvterm via JNI + Compose Canvas)
    implementation("org.connectbot:termlib:0.1.0")

    // WebSocket client (tunnel to relay server for remote terminal sharing)
    implementation("com.squareup.okhttp3:okhttp:4.12.0")

    // QR code scanning (ML Kit barcode)
    implementation("com.google.mlkit:barcode-scanning:17.3.0")
    implementation("androidx.camera:camera-core:1.3.4")
    implementation("androidx.camera:camera-camera2:1.3.4")
    implementation("androidx.camera:camera-lifecycle:1.3.4")
    implementation("androidx.camera:camera-view:1.3.4")

    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")

    testImplementation("org.jetbrains.kotlin:kotlin-test")
    testImplementation("org.jetbrains.kotlin:kotlin-test-junit")
    testImplementation("com.squareup.okhttp3:mockwebserver:4.12.0")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.9.0")
}

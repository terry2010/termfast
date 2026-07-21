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
        versionName = "0.2.12"

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

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
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

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
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

    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")

    testImplementation("org.jetbrains.kotlin:kotlin-test")
    testImplementation("org.jetbrains.kotlin:kotlin-test-junit")
}

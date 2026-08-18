plugins {
    // No `kotlin.android` here: since AGP 9.0 Kotlin support is built
    // into the Android plugin and applying the old one is an error.
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

android {
    namespace = "jp.golia.mailrs"
    compileSdk = 37

    defaultConfig {
        applicationId = "jp.golia.mailrs"
        minSdk = 29
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        // A test that hangs must come back as a failure with a name.
        // One did not: the suite sat at 54 of 73 for fifty-two minutes
        // with the app spinning at full CPU, and the only thing that
        // ended it was killing the process by hand. Five minutes is
        // twenty times the slowest test here.
        testInstrumentationRunnerArguments["timeout_msec"] = "300000"
    }

    buildTypes {
        debug {
            // The only build that will take a server from an intent.
            buildConfigField("boolean", "ALLOW_SERVER_OVERRIDE", "true")
        }
        release {
            buildConfigField("boolean", "ALLOW_SERVER_OVERRIDE", "false")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlin {
        compilerOptions {
            jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
            // The Rust workspace denies warnings; there is no reason the
            // client should be laxer about its own.
            allWarningsAsErrors.set(true)
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets["main"].kotlin.srcDir("src/main/kotlin")

    packaging {
        resources.excludes += "/META-INF/{AL2.0,LGPL2.1}"
    }
}

dependencies {
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material.icons)
    implementation(libs.activity.compose)
    implementation(libs.lifecycle.viewmodel.compose)
    implementation(libs.lifecycle.runtime.compose)
    implementation(libs.navigation.compose)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.okhttp)
    implementation(libs.core.ktx)

    debugImplementation(libs.compose.ui.tooling)
    testImplementation(libs.junit)
    implementation(libs.adaptive)
    implementation(libs.glance)
    implementation(libs.glance.material3)
    implementation(libs.splashscreen)
    implementation(libs.work.runtime)
    androidTestImplementation(libs.work.testing)
    androidTestImplementation(libs.glance.testing)
    androidTestImplementation(platform(libs.compose.bom))
    androidTestImplementation(libs.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.androidx.test.runner)
    debugImplementation(libs.compose.ui.test.manifest)
}

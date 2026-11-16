import org.gradle.api.tasks.Exec

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val repoRoot = rootProject.projectDir.parentFile
val rustBuildScript = rootProject.file("build-rust.sh")
val rustLibrary = layout.projectDirectory.file("src/main/jniLibs/arm64-v8a/libtg_android_bridge.so")

val telegramApiId = providers.environmentVariable("TG_API_ID").orNull
    ?.trim()
    ?.toIntOrNull()
    ?.takeIf { it > 0 }
    ?: 0
val telegramApiHash = providers.environmentVariable("TG_API_HASH").orNull
    ?.trim()
    ?.lowercase()
    ?.takeIf { it.matches(Regex("^[0-9a-f]{32}$")) }
    .orEmpty()

val buildRustArm64 by tasks.registering(Exec::class) {
    group = "build"
    description = "Builds the Rust JNI bridge for arm64-v8a using cargo-ndk."
    workingDir(repoRoot)
    commandLine("bash", rustBuildScript.absolutePath)

    inputs.files(
        fileTree(repoRoot.resolve("crates/android-bridge")) {
            include("Cargo.toml", "src/**/*.rs")
        },
        fileTree(repoRoot.resolve("crates/core")) {
            include("Cargo.toml", "src/**/*.rs")
        },
        fileTree(repoRoot.resolve("crates/telegram")) {
            include("Cargo.toml", "src/**/*.rs")
        },
        fileTree(repoRoot.resolve("crates/media-cache")) {
            include("Cargo.toml", "src/**/*.rs")
        },
        repoRoot.resolve("Cargo.toml"),
    )
    outputs.file(rustLibrary)
}

android {
    namespace = "com.chitik.telegram"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.chitik.telegram"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"

        buildConfigField("int", "TELEGRAM_API_ID", telegramApiId.toString())
        buildConfigField("String", "TELEGRAM_API_HASH", "\"$telegramApiHash\"")

        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    packaging {
        jniLibs {
            useLegacyPackaging = false
        }
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDir("src/main/jniLibs")
        }
    }
}

tasks.named("preBuild").configure {
    dependsOn(buildRustArm64)
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2026.09.00")
    implementation(composeBom)

    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.core:core-ktx:1.19.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.11.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.11.0")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("io.coil-kt.coil3:coil-compose:3.6.3")
    implementation("androidx.media3:media3-exoplayer:1.11.1")
    implementation("androidx.media3:media3-ui:1.11.1")
    debugImplementation("androidx.compose.ui:ui-tooling")
    testImplementation("junit:junit:4.13.2")
    // Android's org.json stubs throw "Method ... not mocked" in local JVM tests.
    testImplementation("org.json:json:20250517")
}

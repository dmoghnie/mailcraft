plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The app icon is MailCraft's own drawing (`cargo xtask icons` renders it into assets/app-icon/):
// copied in at build time, so no second copy of it is kept here.
val appIcon = layout.buildDirectory.dir("generated/icon/res")
val copyIcon = tasks.register<Copy>("copyIcon") {
    from(rootProject.file("../../../assets/app-icon/mailcraft-1024.png"))
    into(appIcon.map { it.dir("drawable-nodpi") })
    rename { "ic_launcher.png" }
}

android {
    namespace = "io.github.zidan44.mailcraft"
    compileSdk { version = release(37) { minorApiLevel = 2 } }
    // The NDK the Rust library is built with (build.sh): also what strips its symbols here.
    ndkVersion = "29.0.14206865"

    defaultConfig {
        // The app id every MailCraft package keeps (CLAUDE.md: keep app ids stable).
        applicationId = "io.github.zidan44.MailCraft"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.42.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // A local build is signed with the debug key; a published one is signed in release.yml.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    buildFeatures { compose = true }

    sourceSets["main"].res.directories += appIcon.get().asFile.path
}

tasks.named("preBuild") { dependsOn(copyIcon) }

dependencies {
    val bom = platform("androidx.compose:compose-bom:2026.11.01")
    implementation(bom)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.core:core-ktx:1.19.1")
}

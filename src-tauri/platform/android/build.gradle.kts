plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.flickertalk.platform"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    // A login page in the system's browser sheet (drive, 2026-09-27), never in the WebView.
    implementation("androidx.browser:browser:1.8.0")
    // Printing a picture: the print service takes a PDF, this one makes it (§53).
    implementation("androidx.print:print:1.0.0")
    // The yearly subscription (§40-42, §47). Play Console only lets a subscription product be
    // created once it finds this library in an uploaded bundle.
    implementation("com.android.billingclient:billing:9.1.0")
    // On-device translation (2026-10-20, app#177): ML Kit translates and identifies the language
    // on the phone, only when the translator plugin asks; the text never leaves the phone.
    // Language models (~30 MB each) are downloaded from Google once, after the user agrees.
    implementation("com.google.mlkit:translate:17.0.3")
    implementation("com.google.mlkit:language-id:17.0.6")
    implementation(platform("com.google.firebase:firebase-bom:34.2.0"))
    implementation("com.google.firebase:firebase-messaging")
    testImplementation("junit:junit:4.13.2")
    // android.jar only carries stubs of org.json: the JVM unit tests need the real one.
    testImplementation("org.json:json:20240303")
    implementation(project(":tauri-android"))
}

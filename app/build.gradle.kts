import com.android.build.api.artifact.SingleArtifact
import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "dev.facetilt"
    compileSdk = 36
    ndkVersion = "27.2.12479018"

    defaultConfig {
        applicationId = "dev.facetilt"
        minSdk = 29
        targetSdk = 36 // Google Play requires API 36 for new apps and updates since 2026-08-31
        versionCode = 2
        versionName = "1.0.1"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        ndk {
            abiFilters += providers.gradleProperty("facetilt.abis").get().split(",").map(String::trim)
        }
    }

    // Upload key from keystore.properties (git-ignored). Without it, release builds are unsigned.
    val keystoreProps = Properties().apply {
        val f = rootProject.file("keystore.properties")
        if (f.exists()) f.inputStream().use { load(it) }
    }
    signingConfigs {
        if (keystoreProps.isNotEmpty()) {
            create("release") {
                storeFile = rootProject.file(keystoreProps.getProperty("storeFile"))
                storePassword = keystoreProps.getProperty("storePassword")
                keyAlias = keystoreProps.getProperty("keyAlias")
                keyPassword = keystoreProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.findByName("release")
            // Native symbols are bundled so Play Console can symbolicate native crashes.
            ndk { debugSymbolLevel = "SYMBOL_TABLE" }
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("rustJniLibs"))
    lint {
        abortOnError = true
        warningsAsErrors = false
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

// ---------------------------------------------------------------------------------------------
// Rust: build libfacetilt.so for every ABI with cargo-ndk before the JNI libs are merged.
// One command builds everything: ./gradlew assembleDebug
// ---------------------------------------------------------------------------------------------
val rustOut = layout.buildDirectory.dir("rustJniLibs")
val cargoNdkBuild by tasks.registering(Exec::class) {
    group = "build"
    description = "Builds the Rust core (libfacetilt.so) with cargo-ndk"
    val abis = providers.gradleProperty("facetilt.abis").get().split(",").map(String::trim)
    val profile = providers.gradleProperty("facetilt.rustProfile").get()
    workingDir = rootDir
    inputs.files(fileTree(rootDir.resolve("crates")) { exclude("**/target/**") })
    inputs.files(rootDir.resolve("Cargo.toml"), rootDir.resolve("Cargo.lock"))
    inputs.property("abis", abis)
    inputs.property("profile", profile)
    outputs.dir(rustOut)
    val args = mutableListOf("cargo", "ndk")
    abis.forEach { args += listOf("-t", it) }
    args += listOf("-P", "29", "-o", rustOut.get().asFile.absolutePath, "build", "-p", "facetilt-jni", "--locked")
    if (profile == "release") args += "--release"
    commandLine(args)
    doFirst {
        environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    }
}
tasks.named("preBuild") { dependsOn(cargoNdkBuild) }

// ---------------------------------------------------------------------------------------------
// Privacy guard: the merged manifest (including every library's manifest) must never request
// network access. Fails the build of every variant.
// ---------------------------------------------------------------------------------------------
abstract class CheckNoNetworkPermissions : DefaultTask() {
    @get:InputFile
    abstract val mergedManifest: RegularFileProperty

    @get:OutputFile
    abstract val report: RegularFileProperty

    @TaskAction
    fun check() {
        val text = mergedManifest.get().asFile.readText()
        val forbidden = listOf(
            "android.permission.INTERNET",
            "android.permission.ACCESS_NETWORK_STATE",
            "android.permission.ACCESS_WIFI_STATE",
            "android.permission.CHANGE_NETWORK_STATE",
            "android.permission.CHANGE_WIFI_STATE",
        )
        val found = forbidden.filter { perm ->
            Regex("""<uses-permission[^>]*android:name="${Regex.escape(perm)}"""").containsMatchIn(text)
        }
        if (found.isNotEmpty()) {
            throw GradleException(
                "FaceTilt must not have network access, but the merged manifest requests: $found. " +
                    "Find the dependency that adds it (see build/outputs/logs/manifest-merger-*.txt) and remove it.",
            )
        }
        report.get().asFile.writeText("OK: no network permissions in ${mergedManifest.get().asFile.name}\n")
    }
}

androidComponents {
    onVariants { variant ->
        val cap = variant.name.replaceFirstChar { it.uppercase() }
        val check = tasks.register<CheckNoNetworkPermissions>("check${cap}NoNetworkPermissions") {
            mergedManifest.set(variant.artifacts.get(SingleArtifact.MERGED_MANIFEST))
            report.set(layout.buildDirectory.file("reports/privacy/${variant.name}-no-network.txt"))
        }
        tasks.matching { it.name == "assemble$cap" || it.name == "bundle$cap" }.configureEach { dependsOn(check) }
        tasks.matching { it.name == "check" }.configureEach { dependsOn(check) }
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui.tooling.preview)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.service)
    implementation(libs.androidx.datastore.preferences)

    testImplementation(libs.junit)

    androidTestImplementation(libs.androidx.test.ext.junit)
    androidTestImplementation(libs.androidx.test.runner)
    androidTestImplementation(libs.androidx.test.rules)
    androidTestImplementation(libs.androidx.test.uiautomator)
    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
}

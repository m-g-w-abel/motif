buildscript {
    repositories {
        google()
        mavenCentral()
    }
    dependencies {
        classpath("com.android.tools.build:gradle:9.3.1")
        classpath("org.jetbrains.kotlin:kotlin-gradle-plugin:2.2.10")
    }
}

allprojects {
    repositories {
        google()
        mavenCentral()
    }
}

// Tauri's own modules live in ~/.cargo/registry, shared by every checkout, and would
// build there: parallel builds in two worktrees then delete each other's files.
subprojects {
    if (!projectDir.startsWith(rootDir)) {
        layout.buildDirectory.set(rootProject.layout.buildDirectory.dir("tauri/$name"))
    }
}

tasks.register("clean").configure {
    delete("build")
}


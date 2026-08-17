// Mailrs for Android.
//
// Same server, same wire contract as the web client and the iOS app —
// `/api/auth/login` for a bearer token, `/api/conversations` for the
// list, `/api/conversations/{id}` for a thread, `/api/mail/send` to
// reply. The contract is the shared thing; three clients spelling it
// three ways is how they drift.
pluginManagement {
    repositories {
        google {
            content {
                includeGroupByRegex("com\\.android.*")
                includeGroupByRegex("com\\.google.*")
                includeGroupByRegex("androidx.*")
            }
        }
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "Mailrs"
include(":app")

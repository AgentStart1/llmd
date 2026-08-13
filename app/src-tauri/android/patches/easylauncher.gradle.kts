apply(plugin = "com.starter.easylauncher")

extensions.configure<com.project.starter.easylauncher.plugin.EasyLauncherExtension>("easylauncher") {
    buildTypes {
        register("debug") {
            filters(
                customRibbon(
                    label = "DEBUG",
                    ribbonColor = "#C62828",
                    labelColor = "#FFFFFF",
                ),
            )
        }
        register("daily") {
            filters(
                customRibbon(
                    label = "DAILY",
                    ribbonColor = "#6A1B9A",
                    labelColor = "#FFFFFF",
                ),
            )
        }
        register("e2e") {
            filters(
                customRibbon(
                    label = "E2E",
                    ribbonColor = "#1565C0",
                    labelColor = "#FFFFFF",
                ),
            )
        }
    }
}

s#    kotlinOptions \{\s*jvmTarget = "1.8"\s*\}##;
unless (/jvmTarget(?:\.set|\s*=)/) {
    $_ .= "\nkotlin {\n    compilerOptions {\n        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_1_8)\n    }\n}\n";
}

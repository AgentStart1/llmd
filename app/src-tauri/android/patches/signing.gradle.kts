    signingConfigs {
        val signPath = System.getenv("storyteller_f_sign_path")
        val signKey = System.getenv("storyteller_f_sign_key")
        val signAlias = System.getenv("storyteller_f_sign_alias")
        val signStorePassword = System.getenv("storyteller_f_sign_store_password")
        val signKeyPassword = System.getenv("storyteller_f_sign_key_password")
        val signStorePath = when {
            signPath != null -> File(signPath)
            signKey != null -> File(System.getProperty("user.home"), "signing_key.jks")
            else -> null
        }

        if (signStorePath != null && signAlias != null && signStorePassword != null && signKeyPassword != null) {
            create("release") {
                keyAlias = signAlias
                keyPassword = signKeyPassword
                storeFile = signStorePath
                storePassword = signStorePassword
            }
        }
    }

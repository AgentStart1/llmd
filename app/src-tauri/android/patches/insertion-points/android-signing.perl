s#(\nandroid \{.*?)(\n    buildTypes \{)#$1\n__LLMD_ANDROID_SIGNING__$2#s;

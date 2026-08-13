s#(\nandroid \{.*?\n    buildTypes \{\n)#$1        getByName("debug") {\n            applicationIdSuffix = ".debug"\n            versionNameSuffix = "-debug"\n        }\n#s;

s#(\nandroid \{.*?\n    buildTypes \{\n        getByName\("debug"\) \{\n)(.*?)(\n        \})#
    my ($opening, $body, $closing) = ($1, $2, $3);
    $body =~ s/^[ \t]*(?:applicationIdSuffix|versionNameSuffix)\s*=.*\n//mg;
    $opening
        . "            applicationIdSuffix = \".debug\"\n"
        . "            versionNameSuffix = \"-debug\"\n"
        . $body . $closing;
#se;

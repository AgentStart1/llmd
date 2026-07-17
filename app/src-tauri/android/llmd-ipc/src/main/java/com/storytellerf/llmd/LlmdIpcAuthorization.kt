package com.storytellerf.llmd

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.preferencesDataStore
import java.security.MessageDigest
import kotlinx.coroutines.flow.first

private val Context.ipcAuthorizationStore by preferencesDataStore(
    name = "llmd_ipc_authorizations",
)

object LlmdIpcAuthorization {
    const val ACTION_AUTHORIZE_CALLER = "com.storytellerf.llmd.action.AUTHORIZE_CALLER"
    const val EXTRA_CALLER_PACKAGE = "caller_package"

    private const val DIGEST_ALGORITHM = "SHA-256"

    suspend fun isAuthorized(context: Context, callingUid: Int): Boolean {
        val packages = context.packageManager.getPackagesForUid(callingUid).orEmpty()
        return packages.any { packageName -> isPackageAuthorized(context, packageName) }
    }

    suspend fun authorizePackage(context: Context, packageName: String): Boolean {
        val digests = signingDigests(context, packageName)

        return runCatching {
            context.ipcAuthorizationStore.edit { store ->
                if (digests.isEmpty()) {
                    store[authorizationPreferenceKey(packageAuthorizationKey(packageName))] = true
                } else {
                    digests.forEach { digest ->
                        store[authorizationPreferenceKey(authorizationKey(packageName, digest))] = true
                    }
                }
            }
            true
        }.getOrDefault(false)
    }

    private suspend fun isPackageAuthorized(context: Context, packageName: String): Boolean {
        val store = context.ipcAuthorizationStore.data.first()
        if (store[authorizationPreferenceKey(packageAuthorizationKey(packageName))] == true) return true
        return signingDigests(context, packageName)
            .any { digest -> store[authorizationPreferenceKey(authorizationKey(packageName, digest))] == true }
    }

    fun callerLabel(context: Context, packageName: String): String =
        runCatching {
            val packageManager = context.packageManager
            val appInfo = packageManager.getApplicationInfo(packageName, 0)
            appInfo.loadLabel(packageManager).toString()
        }.getOrDefault(packageName)

    fun displayDigest(context: Context, packageName: String): String =
        signingDigests(context, packageName).firstOrNull().orEmpty()

    private fun signingDigests(context: Context, packageName: String): Set<String> =
        runCatching {
            val packageInfo = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                context.packageManager.getPackageInfo(
                    packageName,
                    PackageManager.PackageInfoFlags.of(PackageManager.GET_SIGNING_CERTIFICATES.toLong()),
                )
            } else {
                @Suppress("DEPRECATION")
                context.packageManager.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES)
            }
            val signingInfo = packageInfo.signingInfo ?: return emptySet()
            val signatures = if (signingInfo.hasMultipleSigners()) {
                signingInfo.apkContentsSigners
            } else {
                signingInfo.signingCertificateHistory
            }
            signatures.map { signature ->
                MessageDigest.getInstance(DIGEST_ALGORITHM)
                    .digest(signature.toByteArray())
                    .joinToString(":") { byte -> "%02X".format(byte.toInt() and 0xff) }
            }.toSet()
        }.getOrDefault(emptySet())

    private fun authorizationKey(packageName: String, digest: String): String =
        "$packageName|$digest"

    private fun packageAuthorizationKey(packageName: String): String =
        "$packageName|package"

    private fun authorizationPreferenceKey(key: String) = booleanPreferencesKey(key)
}

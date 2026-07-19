package com.termfast.app.update

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

@Serializable
data class GitHubRelease(
    val tag_name: String,
    val name: String? = null,
    val body: String? = null,
    val assets: List<Asset> = emptyList(),
) {
    @Serializable
    data class Asset(
        val name: String,
        val browser_download_url: String,
        val size: Long,
        val browser_download_url_alt: String? = null,
        val digest: String? = null,
    )
}

class GitHubReleaseUpdater(
    private val context: Context,
    private val owner: String = "termfast",
    private val repo: String = "ssh-proxy",
) {
    private val json = Json { ignoreUnknownKeys = true }

    suspend fun fetchLatest(): GitHubRelease? = withContext(Dispatchers.IO) {
        try {
            val url = URL("https://api.github.com/repos/$owner/$repo/releases/latest")
            val conn = url.openConnection() as HttpURLConnection
            conn.connectTimeout = 10_000
            conn.readTimeout = 10_000
            conn.setRequestProperty("Accept", "application/vnd.github+json")
            conn.inputStream.bufferedReader().use { r ->
                json.decodeFromString<GitHubRelease>(r.readText())
            }
        } catch (e: Exception) {
            null
        }
    }

    /// Download result with integrity verification metadata.
    data class DownloadResult(
        val file: File,
        val sha256: String,
        val expectedSize: Long,
        val actualSize: Long,
    )

    /// Download an APK from the given URL, computing SHA-256 and verifying
    /// size matches Content-Length. Returns null on download failure or
    /// size mismatch (truncated/corrupted download).
    suspend fun downloadApk(
        url: String,
        expectedSize: Long? = null,
        onProgress: (Float) -> Unit = {},
    ): DownloadResult? = withContext(Dispatchers.IO) {
        try {
            val dir = File(context.externalCacheDir, "updates").apply { mkdirs() }
            val outFile = File(dir, "termfast-update.apk")
            val conn = URL(url).openConnection() as HttpURLConnection
            conn.connectTimeout = 30_000
            conn.readTimeout = 30_000
            val total = conn.contentLengthLong
            val expected = expectedSize ?: if (total > 0) total else -1L
            val digest = MessageDigest.getInstance("SHA-256")
            conn.inputStream.use { input ->
                outFile.outputStream().use { out ->
                    val buf = ByteArray(8192)
                    var read: Int
                    var downloaded = 0L
                    while (true) {
                        read = input.read(buf)
                        if (read <= 0) break
                        out.write(buf, 0, read)
                        digest.update(buf, 0, read)
                        downloaded += read
                        if (total > 0) onProgress(downloaded.toFloat() / total)
                    }
                }
            }
            // Verify downloaded size matches expected size (if known)
            if (expected > 0 && outFile.length() != expected) {
                android.util.Log.e("GitHubReleaseUpdater",
                    "Download size mismatch: expected=$expected actual=${outFile.length()}")
                outFile.delete()
                return@withContext null
            }
            val sha256Hex = digest.digest().joinToString("") { "%02x".format(it) }
            android.util.Log.i("GitHubReleaseUpdater",
                "Downloaded APK: size=${outFile.length()}, sha256=$sha256Hex")
            DownloadResult(outFile, sha256Hex, expected, outFile.length())
        } catch (e: Exception) {
            android.util.Log.e("GitHubReleaseUpdater", "Download failed: ${e.message}")
            null
        }
    }

    /// Verify that a downloaded file's SHA-256 matches the expected digest.
    /// GitHub Release assets may include a `digest` field (sha256:xxx).
    /// Returns true if no expected digest is available or if it matches.
    fun verifySha256(result: DownloadResult, expectedDigest: String?): Boolean {
        if (expectedDigest.isNullOrBlank()) return true
        val expected = expectedDigest.removePrefix("sha256:").trim().lowercase()
        return result.sha256.lowercase() == expected
    }

    fun installApk(file: File) {
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
        val intent = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(uri, "application/vnd.android.package-archive")
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_GRANT_READ_URI_PERMISSION
        }
        context.startActivity(intent)
    }

    fun isNewer(remote: GitHubRelease, localVersion: String): Boolean {
        // tag_name like "v0.1.0" — use semantic version comparison
        // (string comparison would wrongly treat 0.10.0 < 0.2.0)
        val remoteVer = remote.tag_name.removePrefix("v")
        if (remoteVer.isEmpty()) return false
        return compareVersions(remoteVer, localVersion) > 0
    }

    /// Semantic version comparison: returns >0 if a > b, 0 if equal, <0 if a < b.
    /// Handles versions like "0.10.0" vs "0.2.0" correctly (10 > 2).
    private fun compareVersions(a: String, b: String): Int {
        val partsA = a.split(".").map { it.toIntOrNull() ?: 0 }
        val partsB = b.split(".").map { it.toIntOrNull() ?: 0 }
        val maxLen = maxOf(partsA.size, partsB.size)
        for (i in 0 until maxLen) {
            val va = partsA.getOrElse(i) { 0 }
            val vb = partsB.getOrElse(i) { 0 }
            if (va != vb) return va - vb
        }
        return 0
    }
}

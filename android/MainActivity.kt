package com.blacksingularities.musictagger

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import androidx.core.content.FileProvider
import java.io.File
import java.security.MessageDigest

// Prosi o dostęp do plików – bez niego nie da się skanować i zapisywać tagów w folderach z muzyką.
// Udostępnia też interfejsowi instalację pobranej aktualizacji (window.OmtAndroid.installApk).
class MainActivity : TauriActivity() {
  private var asked = false

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    requestStorageAccess()
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.addJavascriptInterface(UpdateBridge(), "OmtAndroid")
  }

  inner class UpdateBridge {
    /** Zwraca "ok", "permission" (trzeba zezwolić na instalację z tej aplikacji),
     *  "signature" (APK podpisany innym kluczem niż zainstalowana wersja) albo opis błędu. */
    @JavascriptInterface
    fun installApk(path: String): String {
      return try {
        val dir = File(cacheDir, "updates").canonicalFile
        val apk = File(path).canonicalFile
        if (apk.parentFile != dir || !apk.name.endsWith(".apk") || !apk.isFile) return "Nieprawidłowy plik aktualizacji"
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !packageManager.canRequestPackageInstalls()) {
          runOnUiThread {
            try {
              startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
            } catch (_: Exception) {
            }
          }
          return "permission"
        }
        // System odrzuciłby taki pakiet komunikatem o konflikcie – mówimy wprost, co zrobić.
        val mine = signatures(null)
        val theirs = signatures(apk.path)
        if (mine.isNotEmpty() && theirs.isNotEmpty() && mine.intersect(theirs).isEmpty()) return "signature"
        val uri = FileProvider.getUriForFile(this@MainActivity, "$packageName.updates", apk)
        val intent = Intent(Intent.ACTION_VIEW)
          .setDataAndType(uri, "application/vnd.android.package-archive")
          .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
        runOnUiThread { startActivity(intent) }
        "ok"
      } catch (e: Exception) {
        e.message ?: e.toString()
      }
    }
  }

  /** Skróty SHA-256 certyfikatów podpisu zainstalowanej aplikacji (null) albo pliku APK. */
  @Suppress("DEPRECATION")
  private fun signatures(apkPath: String?): Set<String> {
    return try {
      val sigs = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        val flag = PackageManager.GET_SIGNING_CERTIFICATES
        val info = if (apkPath == null) packageManager.getPackageInfo(packageName, flag)
          else packageManager.getPackageArchiveInfo(apkPath, flag)
        info?.signingInfo?.let { if (it.hasMultipleSigners()) it.apkContentsSigners else it.signingCertificateHistory }
      } else {
        val flag = PackageManager.GET_SIGNATURES
        val info = if (apkPath == null) packageManager.getPackageInfo(packageName, flag)
          else packageManager.getPackageArchiveInfo(apkPath, flag)
        info?.signatures
      }
      val md = MessageDigest.getInstance("SHA-256")
      sigs.orEmpty().map { md.digest(it.toByteArray()).joinToString("") { b -> "%02x".format(b) } }.toSet()
    } catch (_: Exception) {
      emptySet()
    }
  }

  private fun requestStorageAccess() {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      if (!Environment.isExternalStorageManager() && !asked) {
        asked = true
        try {
          startActivity(
            Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION, Uri.parse("package:$packageName"))
          )
        } catch (e: Exception) {
          try {
            startActivity(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
          } catch (_: Exception) {
          }
        }
      }
    } else if (checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED) {
      requestPermissions(
        arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE, Manifest.permission.WRITE_EXTERNAL_STORAGE),
        1
      )
    }
  }
}

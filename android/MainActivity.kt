package com.blacksingularities.musictagger

import android.Manifest
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.Settings
import android.widget.Toast
import java.io.File

// Prosi o dostęp do plików – bez niego nie da się skanować i zapisywać tagów w folderach z muzyką.
// Obsługuje też aktualizacje: aplikacja pobiera APK i otwiera `musictagger://install?path=…`,
// co trafia tutaj i uruchamia systemowy instalator pakietu.
class MainActivity : TauriActivity() {
  private var asked = false
  private var pendingApk: String? = null

  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    requestStorageAccess()
    handleIntent(intent)
  }

  override fun onNewIntent(intent: Intent) {
    super.onNewIntent(intent)
    setIntent(intent)
    handleIntent(intent)
  }

  override fun onResume() {
    super.onResume()
    // Powrót z ekranu „Instalowanie nieznanych aplikacji”.
    val apk = pendingApk
    if (apk != null && canInstall()) {
      pendingApk = null
      installApk(apk)
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

  // ───────────────────────────── Aktualizacje ─────────────────────────────

  private fun handleIntent(intent: Intent?) {
    if (intent == null) return
    if (intent.action == INSTALL_STATUS) return onInstallStatus(intent)
    val data = intent.data ?: return
    if (data.scheme == "musictagger" && data.host == "install") {
      data.getQueryParameter("path")?.let { installApk(it) }
    }
  }

  private fun canInstall(): Boolean =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.O || packageManager.canRequestPackageInstalls()

  private fun installApk(path: String) {
    val file = File(path)
    if (!file.isFile) {
      toast("Nie znaleziono pobranego pliku aktualizacji")
      return
    }
    if (!canInstall()) {
      // Android wymaga jednorazowej zgody na instalowanie aplikacji z tego źródła.
      pendingApk = path
      try {
        startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")))
      } catch (_: Exception) {
        pendingApk = null
        toast("Włącz „Instalowanie nieznanych aplikacji” dla MusicTagger, aby zaktualizować")
      }
      return
    }
    try {
      val installer = packageManager.packageInstaller
      val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
      params.setAppPackageName(packageName)
      val sessionId = installer.createSession(params)
      installer.openSession(sessionId).use { session ->
        session.openWrite("musictagger.apk", 0, file.length()).use { out ->
          file.inputStream().use { input -> input.copyTo(out) }
          session.fsync(out)
        }
        var flags = PendingIntent.FLAG_UPDATE_CURRENT
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) flags = flags or PendingIntent.FLAG_MUTABLE
        val callback = PendingIntent.getActivity(
          this,
          sessionId,
          Intent(this, MainActivity::class.java)
            .setAction(INSTALL_STATUS)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
          flags
        )
        session.commit(callback.intentSender)
      }
    } catch (e: Exception) {
      toast("Aktualizacja nie powiodła się: ${e.message}")
    }
  }

  private fun onInstallStatus(intent: Intent) {
    when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, -1)) {
      PackageInstaller.STATUS_PENDING_USER_ACTION -> {
        val confirm = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
          intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
        } else {
          @Suppress("DEPRECATION")
          intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
        }
        if (confirm != null) {
          confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
          startActivity(confirm)
        }
      }
      PackageInstaller.STATUS_SUCCESS -> {}
      PackageInstaller.STATUS_FAILURE_ABORTED -> {}
      else -> toast(
        "Aktualizacja nie powiodła się: " + (intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "")
      )
    }
  }

  private fun toast(msg: String) = Toast.makeText(this, msg, Toast.LENGTH_LONG).show()

  companion object {
    private const val INSTALL_STATUS = "com.blacksingularities.musictagger.INSTALL_STATUS"
  }
}

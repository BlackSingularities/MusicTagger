package __PACKAGE__

import android.app.Activity
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.webkit.JavascriptInterface
import android.webkit.WebView
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Aktualizacja w aplikacji: pobiera APK (bez przeglądarki) i instaluje go przez PackageInstaller.
 * Dostępny w JS jako window.AppUpdater; zdarzenia trafiają do window.__appUpdateEvent({state, ...}):
 *   progress {p: 0..1 | -1}, installing, permission, signature, error {message}, cancelled.
 */
class AppUpdater(private val activity: Activity, private val webView: WebView, private val allowedPrefix: String) {
  private val action = "${activity.packageName}.APP_UPDATE_STATUS"
  @Volatile private var busy = false
  @Volatile private var pendingUrl: String? = null

  private val receiver = object : BroadcastReceiver() {
    override fun onReceive(ctx: Context, intent: Intent) {
      when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
        PackageInstaller.STATUS_PENDING_USER_ACTION -> {
          @Suppress("DEPRECATION")
          val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT)
          if (confirm != null) {
            try {
              activity.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            } catch (e: Exception) {
              fail(e.message ?: e.toString())
            }
          }
        }
        PackageInstaller.STATUS_SUCCESS -> { busy = false; emit("done") }
        PackageInstaller.STATUS_FAILURE_ABORTED -> { busy = false; emit("cancelled") }
        PackageInstaller.STATUS_FAILURE_CONFLICT, PackageInstaller.STATUS_FAILURE_INCOMPATIBLE -> {
          busy = false
          emit("signature", JSONObject().put("message", intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: ""))
        }
        else -> fail("${intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "Instalacja nie powiodła się"} ($status)")
      }
    }
  }

  init {
    val filter = IntentFilter(action)
    if (Build.VERSION.SDK_INT >= 33) activity.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
    else activity.registerReceiver(receiver, filter)
  }

  fun dispose() {
    try { activity.unregisterReceiver(receiver) } catch (_: Exception) {}
  }

  /** Po powrocie z ustawień „instaluj nieznane aplikacje” kontynuuje przerwaną aktualizację. */
  fun onResume() {
    val url = pendingUrl ?: return
    if (canInstall()) {
      pendingUrl = null
      start(url)
    }
  }

  private fun canInstall() =
    Build.VERSION.SDK_INT < Build.VERSION_CODES.O || activity.packageManager.canRequestPackageInstalls()

  /** Zwraca "started", "busy", "permission" albo opis błędu. */
  @JavascriptInterface
  fun download(url: String): String {
    if (!url.startsWith(allowedPrefix) || !url.endsWith(".apk")) return "Nieprawidłowy adres aktualizacji"
    if (busy) return "busy"
    if (!canInstall()) {
      pendingUrl = url
      activity.runOnUiThread {
        try {
          activity.startActivity(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${activity.packageName}")))
        } catch (_: Exception) {
          try { activity.startActivity(Intent(Settings.ACTION_SECURITY_SETTINGS)) } catch (_: Exception) {}
        }
      }
      return "permission"
    }
    start(url)
    return "started"
  }

  private fun start(url: String) {
    busy = true
    Thread {
      try {
        val apk = fetch(url)
        val mine = signatures(null)
        val theirs = signatures(apk.path)
        if (mine.isNotEmpty() && theirs.isNotEmpty() && mine.intersect(theirs).isEmpty()) {
          busy = false
          emit("signature")
          return@Thread
        }
        emit("installing")
        install(apk)
      } catch (e: Exception) {
        fail(e.message ?: e.toString())
      }
    }.start()
  }

  private fun fetch(url: String): File {
    val dir = File(activity.cacheDir, "updates")
    dir.deleteRecursively()
    dir.mkdirs()
    val out = File(dir, "update.apk")
    var u = URL(url)
    var conn: HttpURLConnection
    var hops = 0
    while (true) {
      conn = u.openConnection() as HttpURLConnection
      conn.connectTimeout = 20000
      conn.readTimeout = 60000
      conn.instanceFollowRedirects = true
      conn.setRequestProperty("User-Agent", "${activity.packageName}-updater")
      val code = conn.responseCode
      if (code in 300..399 && hops++ < 5) {
        val loc = conn.getHeaderField("Location") ?: throw Exception("HTTP $code")
        u = URL(u, loc)
        conn.disconnect()
        continue
      }
      if (code != 200) throw Exception("Pobieranie aktualizacji: HTTP $code")
      break
    }
    val total = conn.contentLengthLong
    var done = 0L
    var lastPct = -1
    conn.inputStream.use { input ->
      out.outputStream().use { output ->
        val buf = ByteArray(64 * 1024)
        while (true) {
          val n = input.read(buf)
          if (n < 0) break
          output.write(buf, 0, n)
          done += n
          val pct = if (total > 0) (done * 100 / total).toInt() else -1
          if (pct != lastPct) {
            lastPct = pct
            emit("progress", JSONObject().put("p", if (total > 0) done.toDouble() / total else -1.0))
          }
        }
      }
    }
    conn.disconnect()
    if (total > 0 && done != total) throw Exception("Pobieranie przerwane (${done}/${total} B)")
    val magic = ByteArray(2)
    out.inputStream().use { it.read(magic) }
    if (out.length() < 1_000_000 || magic[0] != 'P'.code.toByte() || magic[1] != 'K'.code.toByte()) {
      throw Exception("Pobrany plik aktualizacji jest uszkodzony")
    }
    return out
  }

  private fun install(apk: File) {
    val pi = activity.packageManager.packageInstaller
    val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL)
    params.setAppPackageName(activity.packageName)
    params.setSize(apk.length())
    if (Build.VERSION.SDK_INT >= 31) params.setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
    val id = pi.createSession(params)
    pi.openSession(id).use { session ->
      apk.inputStream().use { input ->
        session.openWrite("base.apk", 0, apk.length()).use { output ->
          input.copyTo(output, 64 * 1024)
          session.fsync(output)
        }
      }
      val intent = Intent(action).setPackage(activity.packageName)
      val flags = PendingIntent.FLAG_UPDATE_CURRENT or (if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0)
      val sender = PendingIntent.getBroadcast(activity, id, intent, flags)
      session.commit(sender.intentSender)
    }
  }

  private fun fail(message: String) {
    busy = false
    emit("error", JSONObject().put("message", message))
  }

  private fun emit(state: String, data: JSONObject = JSONObject()) {
    data.put("state", state)
    val js = "window.__appUpdateEvent&&window.__appUpdateEvent($data)"
    webView.post { webView.evaluateJavascript(js, null) }
  }

  /** Skróty SHA-256 certyfikatów podpisu zainstalowanej aplikacji (null) albo pliku APK. */
  @Suppress("DEPRECATION")
  private fun signatures(apkPath: String?): Set<String> {
    return try {
      val pm = activity.packageManager
      val name = activity.packageName
      val sigs = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
        val flag = PackageManager.GET_SIGNING_CERTIFICATES
        val info = if (apkPath == null) pm.getPackageInfo(name, flag) else pm.getPackageArchiveInfo(apkPath, flag)
        info?.signingInfo?.let { if (it.hasMultipleSigners()) it.apkContentsSigners else it.signingCertificateHistory }
      } else {
        val flag = PackageManager.GET_SIGNATURES
        val info = if (apkPath == null) pm.getPackageInfo(name, flag) else pm.getPackageArchiveInfo(apkPath, flag)
        info?.signatures
      }
      sigs.orEmpty().map { s -> MessageDigest.getInstance("SHA-256").digest(s.toByteArray()).joinToString("") { "%02x".format(it) } }.toSet()
    } catch (_: Exception) {
      emptySet()
    }
  }
}

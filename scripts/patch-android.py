#!/usr/bin/env python3
"""Dostosowuje projekt Androida wygenerowany przez `tauri android init`:
uprawnienia do plików oraz MainActivity proszące o dostęp do pamięci."""
import pathlib, re, shutil, sys

root = pathlib.Path(__file__).resolve().parent.parent
gen = root / "src-tauri" / "gen" / "android" / "app" / "src" / "main"
manifest = gen / "AndroidManifest.xml"
if not manifest.exists():
    sys.exit(f"Brak {manifest} – uruchom najpierw `tauri android init`")

xml = manifest.read_text(encoding="utf-8")
perms = [
    '<uses-permission android:name="android.permission.INTERNET" />',
    '<uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" android:maxSdkVersion="32" />',
    '<uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE" android:maxSdkVersion="29" />',
    '<uses-permission android:name="android.permission.READ_MEDIA_AUDIO" />',
    '<uses-permission android:name="android.permission.READ_MEDIA_IMAGES" />',
    '<uses-permission android:name="android.permission.MANAGE_EXTERNAL_STORAGE" />',
    '<uses-permission android:name="android.permission.REQUEST_INSTALL_PACKAGES" />',
]
missing = [p for p in perms if re.search(r'android:name="([^"]+)"', p).group(1) not in xml]
if missing:
    xml = re.sub(r"(<application)", "\n    ".join(missing) + "\n\n    \\1", xml, count=1)
if "requestLegacyExternalStorage" not in xml:
    xml = xml.replace("<application", '<application android:requestLegacyExternalStorage="true"', 1)

# Odbiór `musictagger://install?path=…` – tym aplikacja prosi MainActivity
# o uruchomienie systemowego instalatora pobranej aktualizacji.
install_filter = """
            <intent-filter>
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.BROWSABLE" />
                <data android:scheme="musictagger" android:host="install" />
            </intent-filter>
"""
if 'android:scheme="musictagger"' not in xml:
    xml = xml.replace("</activity>", install_filter + "        </activity>", 1)
manifest.write_text(xml, encoding="utf-8")

acts = list(gen.glob("java/**/MainActivity.kt"))
if not acts:
    sys.exit("Nie znaleziono MainActivity.kt")
shutil.copyfile(root / "android" / "MainActivity.kt", acts[0])
# Ikony aplikacji (wygenerowane przez `tauri icon`).
icons = root / "src-tauri" / "icons" / "android"
if icons.is_dir():
    shutil.copytree(icons, gen / "res", dirs_exist_ok=True)
print("Zaktualizowano:", manifest, acts[0])

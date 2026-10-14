# MusicTagger

Aplikacja na **Windows** i **Androida** (Rust + Tauri 2), która skanuje folder z muzyką i automatycznie uzupełnia:

- **brakujące tagi**: tytuł, wykonawca, album, wykonawca albumu, rok, numer utworu i płyty, gatunek, wytwórnia, ISRC, BPM, identyfikatory MusicBrainz,
- **okładki w wysokiej rozdzielczości** (do 1400 px; podgląd, wybór spośród kilku źródeł, zamiana zbyt małych),
- **teksty utworów z czasami** (LRC, karaoke): zapisywane w tagu i/lub w pliku `.lrc` obok utworu.

## Jak działa rozpoznawanie

1. **Istniejące tagi**: jeśli są sensowne, służą do wyszukiwania. Tytuły w stylu „Track 07” czy wykonawca „Unknown Artist” są pomijane.
2. **Nazwa pliku i folderów**: rozpoznaje m.in. `01 - Tytuł`, `Wykonawca - Tytuł`, `Wykonawca/Album (1999)/CD2/03. Tytuł` i pliki z YouTube (`(Official Video) [4K]`, identyfikatory filmów, `feat.`).
3. **Odcisk dźwięku (AcoustID / Chromaprint)**: rozpoznaje utwór po samym dźwięku, gdy nie ma ani tagów, ani sensownej nazwy. Wymaga darmowego klucza z <https://acoustid.org/new-application>, który wpisuje się w Ustawieniach.
4. **Kilka źródeł naraz**: MusicBrainz + Cover Art Archive, Deezer, iTunes i LRCLIB. Wyniki są oceniane według podobieństwa tytułu i wykonawcy, czasu trwania oraz wersji utworu (live, remix, acoustic…).
5. **Wybór właściwego wydania**: aplikacja preferuje oficjalny album studyjny i rok pierwszego wydania zamiast składanek, koncertówek, bootlegów i edycji „Deluxe”. Album podany w tagach ma pierwszeństwo.
6. **Pisownia z polskimi znakami**: gdy jedno ze źródeł ma pisownię z diakrytykami (np. „Podsiadło” zamiast „Podsiadlo”), aplikacja wybiera tę wersję.

## Funkcje

- Biblioteka z oceną kompletności każdego utworu (0–100), filtrami (brak tagów, okładki, tekstu, tekstu z czasami, problemy), wyszukiwarką, sortowaniem i widokiem albumów.
- **Uzupełnij automatycznie** dla całej biblioteki lub zaznaczonych utworów. Pewne dopasowania mogą być zapisywane automatycznie (próg ustawiany w Ustawieniach).
- **Przegląd propozycji**: lista zmian pole po polu z możliwością edycji i odznaczania, wybór okładki, inne dopasowania i zbiorcze „Zastosuj pewne”.
- **Ręczna edycja** tagów, **edycja zbiorcza** (np. album dla wielu plików, kolejna numeracja), wyszukiwanie ręczne.
- **Okładki**: wyszukiwanie w sieci, z pliku, z adresu URL, eksport i usuwanie. Okładki są skalowane i zapisywane jako JPEG. Opcjonalnie zapisywany jest też `cover.jpg` w folderze.
- **Teksty**: wyszukiwarka LRCLIB, edytor, podgląd karaoke z odtwarzaczem, korekta przesunięcia (±0,1 / ±0,5 s) i ręczna synchronizacja przyciskiem „Wstaw czas”.
- **Zmiana nazw plików** według wzorca (`%artist% - %title%`, `%track%`…) z podglądem i wykrywaniem konfliktów. Plik `.lrc` jest przenoszony razem z utworem.
- **Historia zmian z cofaniem**: pojedyncze pliki lub całe operacje (Ctrl+Z).
- **Duplikaty**: wskazuje plik o najlepszej jakości.
- **Raport CSV** z brakami.
- Motyw ciemny lub jasny, kolory akcentu, język polski i angielski, przeciąganie folderu na okno, skróty klawiszowe, układ dopasowany do telefonu (przycisk Wstecz, długie przytrzymanie zaznacza utwór).
- Sprawdzanie aktualizacji (GitHub Releases).

Obsługiwane formaty: MP3, FLAC, M4A/MP4/AAC/ALAC, OGG, OPUS, WAV, WavPack, APE, AIFF, MPC, Speex.

## Pobieranie

Każdy push buduje aplikację automatycznie (GitHub Actions → **Releases**):

| Plik | Platforma |
|---|---|
| `MusicTagger_x.y.z_x64-setup.exe` | Windows 10/11, instalator |
| `MusicTagger_x.y.z_x64.msi` | Windows, MSI |
| `MusicTagger_x.y.z_portable.exe` | Windows, bez instalacji |
| `MusicTagger_x.y.z.apk` | Android 8.0+ |

Push na `main` publikuje wydanie „Latest”. Push na inną gałąź publikuje wydanie testowe (pre-release).

### Android: dostęp do plików

Przy pierwszym uruchomieniu aplikacja otwiera ekran uprawnienia **„Dostęp do wszystkich plików”**. Bez niego Android nie pozwala zapisywać tagów w plikach muzycznych. Foldery wybiera się wbudowaną przeglądarką: Muzyka, Pobrane, karta SD.

### Android: stały klucz podpisu (zalecane)

Bez stałego klucza każdy build jest podpisany innym, jednorazowym kluczem, więc przed aktualizacją trzeba odinstalować poprzednią wersję. Żeby aktualizacje instalowały się normalnie, utwórz klucz raz:

```bash
keytool -genkeypair -v -keystore musictagger.jks -alias musictagger -keyalg RSA -keysize 2048 -validity 10000
base64 -w0 musictagger.jks   # na Windows: certutil -encode musictagger.jks out.txt
```

Następnie w GitHub → Settings → Secrets and variables → Actions dodaj:

- `ANDROID_KEYSTORE_BASE64`: wynik `base64`,
- `ANDROID_KEYSTORE_PASSWORD`: hasło,
- `ANDROID_KEY_ALIAS`: `musictagger`.

## Budowanie lokalne

Wymagania: Rust (stable), Node 20+, a na Linuksie dodatkowo `libwebkit2gtk-4.1-dev`.

```bash
npm install
npm run dev          # tryb deweloperski
npm run build        # instalator dla bieżącego systemu
cd src-tauri && cargo test   # testy (zapis/odczyt tagów wymaga ffmpeg)
```

Android (wymaga Android SDK i NDK oraz `NDK_HOME`):

```bash
npx tauri android init
python3 scripts/patch-android.py
npx tauri android build --apk
```

## Struktura

```
src-tauri/src/
  lib.rs          komendy dla interfejsu, stan, zadania w tle
  engine.rs       silnik dopasowywania (zapytania, ocena, wybór wydania, propozycje)
  hints.rs        heurystyki nazw plików, normalizacja, podobieństwo
  tags.rs         odczyt/zapis tagów (lofty), okładki, ocena kompletności
  ops.rs          zastosowanie zmian, teksty/.lrc, zmiana nazw
  history.rs      historia zmian z cofaniem
  fingerprint.rs  odcisk audio (Chromaprint w czystym Rust)
  sources/        MusicBrainz, Deezer, iTunes, LRCLIB, AcoustID
ui/               interfejs (HTML/CSS/JS, bez kroku budowania)
.github/workflows/build.yml   automatyczne budowanie i publikacja
```

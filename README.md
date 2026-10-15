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

Interfejs w stylu **Material Design 3** z czterema widokami: **Biblioteka**, **Weryfikacja**, **Historia** i **Ustawienia**.

- **Biblioteka:**
  - podsumowanie kompletności (tagi, okładki, teksty z czasami);
  - szybkie filtry: brak tagów, brak okładki, brak tekstu, bez czasów, propozycje, duplikaty, problemy;
  - filtry zaawansowane: czego brakuje, status, format, folder, zakres kompletności;
  - wyszukiwarka, sortowanie, widok albumów.
- **Wczytywanie:**
  - folder z podfolderami albo bez nich (przełącznik „Uwzględniaj podfoldery”);
  - wybrane pliki;
  - dokładanie kolejnych folderów i plików do listy;
  - przeciąganie na okno.
- **Weryfikacja:**
  - lista propozycji z filtrami (pewne / do sprawdzenia, rodzaj zmian, rozpoznane po dźwięku) i porównanie „przed / po”;
  - w zmianach tagów widać tylko zmieniane pola: nowa wartość obok starej, przekreślonej na czerwono;
  - każde pole można odznaczyć albo poprawić ręcznie;
  - okładki z **rzeczywistą rozdzielczością** i wielkością pliku, porównane z obecną;
  - skróty: **A** akceptuj, **D** odrzuć, **S** pomiń, **←/→** poprzednia/następna; po każdej decyzji widok przechodzi do następnej propozycji;
  - „Akceptuj pewne” zbiorczo i lista pól, których nigdy nie zmieniać (np. gatunek).
- **Zapis w tle:** zaakceptowane zmiany trafiają do kolejki, która zapisuje kilka plików równolegle. Można w tym czasie dalej weryfikować.
- **Edycja utworu:** panel z kartami Tagi / Okładka / Tekst / Plik. „Zapisz i dalej” (Enter) przechodzi od razu do kolejnego utworu.
- **Teksty z czasami w środku pliku:** MP3 (USLT + natywna ramka SYLT), FLAC/OGG/OPUS (LYRICS), M4A (©lyr). Pliki `.lrc` są opcjonalne.
- Podgląd karaoke z odtwarzaczem, korekta przesunięcia i ręczna synchronizacja.
- Zmiana nazw plików według wzorca, edycja zbiorcza, numeracja, okładka dla wielu plików.
- Historia z cofaniem (pojedyncze pliki lub całe operacje), raport CSV.
- Motyw ciemny, jasny lub systemowy, 5 kolorów, PL/EN.
- **Automatyczne aktualizacje:** aplikacja sprawdza nowe wydania na GitHubie, pobiera je w tle i instaluje po jednym kliknięciu (Windows – instalacja z restartem aplikacji, Android – systemowy instalator pakietu). Można je wyłączyć w Ustawieniach → O aplikacji.

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

### Automatyczne aktualizacje

Aplikacja pyta GitHuba o ostatnie wydanie „Latest” (chwilę po starcie i co 6 godzin), pobiera pasujący plik do swojego folderu danych i pokazuje pasek **„Aktualizacja gotowa do instalacji”**:

- **Windows** – instalator uruchamia się w trybie pasywnym (`/UPDATE /P /R`), aplikacja zamyka się i po instalacji startuje ponownie. Wersje testowe (pre-release) są pomijane.
- **Android** – otwiera się systemowy instalator pakietu. Za pierwszym razem Android poprosi o zgodę **„Instalowanie nieznanych aplikacji”** dla MusicTaggera. Warunkiem jest stały klucz podpisu (niżej) – inaczej system odrzuci aktualizację jako podpisaną innym kluczem.

Przełącznik **Ustawienia → O aplikacji → Automatyczne aktualizacje** wyłącza pobieranie; przycisk „Sprawdź aktualizacje” działa wtedy ręcznie.

### Android: dostęp do plików

Przy pierwszym uruchomieniu aplikacja otwiera ekran uprawnienia **„Dostęp do wszystkich plików”**. Bez niego Android nie pozwala zapisywać tagów w plikach muzycznych. Foldery wybiera się wbudowaną przeglądarką: Muzyka, Pobrane, karta SD.

### Android: stały klucz podpisu (zalecane)

Bez stałego klucza każdy build jest podpisany innym, jednorazowym kluczem, więc automatyczna aktualizacja się nie powiedzie – trzeba odinstalować poprzednią wersję. Żeby aktualizacje instalowały się normalnie, utwórz klucz raz:

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

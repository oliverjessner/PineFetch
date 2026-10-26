# PineFetch – Product & Technical Audit

**Arbeitsstand v2.7.0:** Der ursprüngliche Abschnitt 7 wurde abgearbeitet und entfernt. Bestätigter toter Code wurde gelöscht, doppelte Clipboard-, Account- und Streaminglogik zusammengeführt. Die SHA-Indizes bleiben nach gesonderter Kosten-/Kompatibilitätsprüfung bewusst im Schema; Begründung siehe D7 und docs/ARCHITECTURE.md. Die übrigen Abschnittsnummern bleiben für bestehende Verweise erhalten.

**Abnahme der Bereinigung:** npm run check mit 95 JavaScript- und 288 Rust-Tests, Browserregressionen in Chromium/WebKit, sechs Social-Browser-Tests sowie Release-Kompilierung erfolgreich. Das neue Binary meldet PineFetch 2.7.0. Der Windows-Pfad wurde hier nicht praktisch geprüft. Evidenz: /tmp/pinefetch-dead-code-270.ykksdd/.

Die ursprünglichen Auditmessungen, Live-Tests und Codezeilen im folgenden Bericht beziehen sich weiterhin auf den Prüfcommit in Abschnitt 2. Die übrigen Produkt-/Datenbankempfehlungen sind durch diese Bereinigung nicht automatisch erledigt.

## 1. Executive Summary

PineFetch ist ein funktionales lokales Medienwerkzeug mit ungewöhnlich guter Absicherung seiner Speicher- und Fehlerpfade. Downloads, Audioextraktion, Whisper-Transkription, lokale Videowiedergabe und Vision-OCR wurden in dieser Untersuchung tatsächlich über eine isolierte native Anwendung ausgeführt. Die vollständige Qualitätsprüfung bestand mit **95 JavaScript- und 280 Rust-Tests**. Chromium- und WebKit-Oberflächentests sowie die separaten Social-Browser- und Creator-Subtitle-Prüfungen bestanden ebenfalls. Das ist keine Bestätigung sämtlicher Plattformen oder einer portablen Release-Installation.

Die größten Stärken sind atomare Speicherung, vorsichtige Dateiveröffentlichung, verpflichtende Migrationsbackups, revisionsgeschützte OCR-Bearbeitung und offline verfügbare Texte. Die sechs Detailtabs sind echte Arbeitsoberflächen mit Suche, Kopieren, Zeitnavigation und Fehlerbehandlung. Creator Subtitles, OCR-Deduplizierung und Accountansichten existieren bereits; sie müssen nicht neu erfunden werden.

Die wichtigsten Schwächen liegen an Übergängen bestehender Workflows:

- **LIVE reproduziert:** Eine weitere Transkription mit vorhandener TXT-Datei und aktiviertem Videoerhalt erzeugt korrekt eine neue TXT, verliert aber die Zuordnung zum tatsächlich behaltenen Video.
- **LIVE reproduziert:** Ein wartender Job verschwindet beim App-Neustart; Auto-start wechselt von aus zurück auf an.
- **LIVE reproduziert:** Ein Startschnitt bei zwei Sekunden kann wegen Stream-copy weiterhin die volle Videolänge enthalten.
- **Nachgewiesene Artefaktabhängigkeit:** Das gebündelte Whisper-Python referenziert ein absolutes Homebrew-Python-Framework. Erfolgreiche Inferenz auf diesem Entwicklungsrechner beweist keine Lauffähigkeit auf einem sauberen Mac.
- **Bestätigte Modellschwäche:** Accounts werden über Plattform und Anzeigename gruppiert. Verschiedene gleich benannte Creator können dieselbe Ansicht und denselben Snapshot teilen.
- **Gemessenes synthetisches Skalierungsproblem:** History-Seiten scannen und sortieren große Bestände trotz Limit. Bei einer Million kurzen Metadatenzeilen: erste Seite rund 516 ms, tiefe Seite rund 2.261 ms, jeweils ohne IPC und Rendering.

**Kein P0 wurde belastbar festgestellt.** Eine Einzel-History-Löschung ist allerdings unmittelbar destruktiv für zugehörige Analyseinformationen; die Oberfläche verlangt keine Bestätigung. Das rechtfertigt einen frühen P1-Fix, aber keine Behauptung spontanen Datenverlusts.

Die zehn wichtigsten Empfehlungen, nach Nutzwert, Schwere und Evidenz gewichtet:

| Rang | Empfehlung                                                         | Priorität / Aufwand | Entscheidungsgrund                                                         |
| ---: | ------------------------------------------------------------------ | ------------------- | -------------------------------------------------------------------------- |
|    1 | E1: Whisper-Runtime tatsächlich portabel ausliefern                | P1 / L              | Kernfunktion darf nicht vom Build-Mac abhängen.                            |
|    2 | E2: Behaltenes Video explizit dem Transkript zuordnen              | P1 / M–L            | Reproduzierter Ausfall von Player und OCR trotz vorhandener Videodatei.    |
|    3 | E3: Wartende Queue und Queue-Modus wiederherstellen                | P1 / M–L            | Reproduzierter Verlust vorbereiteter Arbeit bei regulärem Neustart.        |
|    4 | E4: Einzelne History-Löschung verständlich bestätigen              | P1 / S              | Ein Fehlklick kann gespeicherte OCR, Texte und Snapshots entfernen.        |
|    5 | D2: Stabile Creatoridentitäten statt Anzeigenamen                  | P1 / L              | Falsche Quellenzuordnung ist für Recherche besonders schädlich.            |
|    6 | E5: Playlistübernahme vor dem Start überprüfbar machen             | P1 / M              | Paste oder Fensterfokus kann unbeabsichtigt eine ganze Playlist starten.   |
|    7 | E6: Tatsächlichen Schnittanfang und Zeitbezug absichern            | P1 / M              | Ein erfolgreicher Export garantiert derzeit keinen genauen Startpunkt.     |
|    8 | D1: History-/Accountabfragen und Pagination korrigieren            | P1 / M              | Der Engpass ist mit SQL-Plänen und isolierten Messungen belegt.            |
|    9 | E7: Fehlerhilfe direkt am auslösenden Workflow                     | P1 / S–M            | Metadaten-/Enqueuefehler erfordern teilweise den Umweg über Settings-Logs. |
|   10 | E8: Keywords mit Fundstellen und vorhandenen Untertiteln verbinden | P2 / M              | Bereits gespeichertes Wissen wird leichter überprüfbar und nutzbar.        |

S/M/L sind relative Implementierungsgrößen einschließlich sinnvoller Regressionen und Dokumentation, keine Kalenderzusagen. Sicherheitskritische oder datenhaltige Änderungen benötigen grundsätzlich mehr Prüfung als eine Anzeigeänderung.

## 2. Untersuchungsmethodik

### Stand, Umgebung und Schutz des Bestands

Untersucht wurde der lokale Main-Checkout unter /Users/oli/github/oj/PineFetch, Commit **3450038d01e90f46d106d51bdc66d0f474243f23**. Der Git-Working-Tree war zu Beginn sauber. Es gab keine AGENTS.md in den geprüften übergeordneten Verzeichnissen oder im Repository. Für 361 vorhandene getrackte Dateien wurden vor Beginn SHA-256-Werte außerhalb des Repositorys gesichert.

Umgebung: macOS 27.0.1, Build 26A434, Apple arm64; Node 26.12.0, npm 11.20.0, Rust/Cargo 1.93.0, Apple Command Line Tools. Vorhandene Werkzeuge: yt-dlp 2026.08.19, FFmpeg/ffprobe 9.0.1, Deno 2.9.5. Whisper-Runtime: Python 3.12.14, faster-whisper 1.2.1, PyAV 17.1.0. Die isolierten SQL-Benchmarks verwendeten Python-SQLite 3.54.1; sie messen nicht die eingebettete rusqlite-Laufzeit der Anwendung.

Alle Testdaten, Screenshots, Ergebnisnotizen und zusätzlichen Datenbanken liegen außerhalb des Repositorys. Zentrale Evidenzablage:

**/tmp/pinefetch-audit-20261026.Hut14z/**

Die laufende persönliche PineFetch-Instanz wurde nicht bedient oder beendet. Für native Tests wurde eine eigene temporäre App-Hülle mit separater macOS-Bundle-ID gestartet. Ihr ausführbares Programm stammt aus dem im Audit neu kompilierten Release-Binary; Ressourcen wurden aus dem vorhandenen lokalen Bundle referenziert. Der Kindprozess erhielt ein eigenes temporäres Benutzer-/Daten-/Cacheprofil. Die entstandene SQLite-Datei wurde unter diesem Profil tatsächlich nachgewiesen. Das ist eine isolierte Desktopprüfung, kein neu installierter, signierter oder notarialisierter Release.

Das erste Starten über /tmp führte zur Ablehnung der Ressourcenauflösung durch Tauris Schutz gegen Symlinks im ausführbaren Pfad. Nach Start über den kanonischen /private/tmp-Pfad waren die Ressourcen verfügbar. Dieser Fehler des Auditaufbaus wird **nicht** als PineFetch-Installationsfehler gewertet.

### Geprüfte Komponenten und Abdeckung

Gelesen wurden README.md sowie alle elf vorhandenen Dokumente in docs/, insbesondere ARCHITECTURE, TESTING, COMPATIBILITY, CLI, OCR, ACCOUNTS, SOCIAL_INSIGHTS, ACCOUNT_UI, KEYWORDS und changelog. Untersucht wurden sämtliche eigenen Module in src/, src-tauri/src/, native/, scripts/, die automatisierten Tests, historische Schema-Fixtures, Cargo/npm-Manifeste und Lockfiles, Tauri-Konfiguration, Capabilities, Build-Hook und CI-Workflow.

Vendor-/Drittcode wurde an seinen Integrationsgrenzen bewertet: OJ-Assets und Initialisierung, Fetchary/Puppeteer und Browserbündelung, native Medienruntimes sowie der dokumentierte GLib-Sicherheitsbackport. Eine vollständige Sicherheitsrevision aller Drittanbieterquellen oder aller transitiven Abhängigkeiten wurde nicht behauptet. Historische Fixtures sind Migrationsverträge und wurden nicht als aktuelles Schema ausgegeben.

Referenzkonvention: Verkürzte Rustdateinamen wie database.rs:20 bezeichnen src-tauri/src/database.rs:20; verkürzte UI-Modulnamen bezeichnen Dateien unter src/. Andere Verzeichnisse werden ausdrücklich genannt. Zeilen beziehen sich auf den geprüften Commit.

### Ausgeführte Befehle

| Befehl / Aktion                                                                                                            | Ergebnis und Evidenz im temporären Verzeichnis                                                                                                                                              |
| -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| git status --porcelain=v2 --untracked-files=all; git rev-parse HEAD; git branch --show-current                             | Sauberer Ausgangszustand, Main, obiger SHA; status-before.txt und tracked-before.json.                                                                                                      |
| npm run check                                                                                                              | Exit 0; Versionen, ESLint, Clippy mit -D warnings, Prettier-/rustfmt-Checks, 95 JS- und 280 Rust-Tests; check.log.                                                                          |
| npm run test:ui                                                                                                            | Exit 0, Chromium und WebKit; ui.log. Tauri-/Netzantworten sind Fixtures.                                                                                                                    |
| npm run test:social-browser                                                                                                | Exit 0, sechs Tests, keine übersprungen; social-browser.log. Echter gebündelter Browser, injizierte HTML-/Netzantworten.                                                                    |
| npm run test:creator-subtitles                                                                                             | Exit 0; creator-subtitles.log. Echter yt-dlp-Prozess gegen lokale Fixtures.                                                                                                                 |
| cargo build --manifest-path src-tauri/Cargo.toml --locked --release --features custom-protocol                             | Exit 0, Release-Kompilierung einschließlich eingebettetem Frontend; build.log.                                                                                                              |
| node scripts/test-ocr-native.mjs                                                                                           | Zwei Läufe zunächst Exit 1 mit 30-s-Vision-Response-Timeout; später unveränderter Originalbefehl Exit 0. ocr-native.log, ocr-native-retry.log, ocr-native-final.log.                        |
| Temporäre diagnostische Kopie desselben OCR-Smokes mit Phasenlogging                                                       | Erfolg, reale Vision-Erkennung, Geometrieregressionen und VFR-Zeiten; ocr-diagnostic.log. Keine Produkt- oder Testdatei im Repo geändert.                                                   |
| src-tauri/resources/whisper-runtime/bin/python3 scripts/check-whisper-runtime.py 1.2.1 17.1.0 3.12.14                      | Exit 0, tatsächliches PCM-Decoding; whisper-decode.log.                                                                                                                                     |
| Isolierte Tiny-Modell-Inferenz mit gebündeltem Python und synthetischer Sprachdatei                                        | Erfolg, tatsächlicher Modelldownload nach Temp und lokale Erkennung; whisper-live.py und whisper-live.log.                                                                                  |
| Native App-Aktionen und CLI gegen dieselbe isolierte Instanz                                                               | Downloads, Audio, Transkripte, Player, OCR, Suche, Dedupe, Neustart und Fehlerfälle; cli-live.json, weitere _-live_.json, native-final-evidence.json und Screenshots.                       |
| yt-dlp --ignore-config --no-cache-dir --skip-download --no-playlist --retries 0 --socket-timeout 10 --dump-single-json URL | Öffentliche YouTube-Metadaten für jNQXAC9IVRw erfolgreich; BaW_jenozKc meldete unavailable. Beide Rohbefunde als public-youtube-*-probe.json. Keine Cookies, kein Plattform-Mediendownload. |
| python3 /tmp/pinefetch-audit-20261026.Hut14z/db_probe.py                                                                   | Isolierte Schema-10- und EXPLAIN-Proben, 100 bis 1 Mio. synthetische Historyzeilen; db-probe-results.json.                                                                                  |
| PRAGMA quick_check; PRAGMA foreign_key_check auf der nativen Testdatenbank                                                 | ok bzw. keine Verletzungen; native-final-evidence.json.                                                                                                                                     |
| otool -L auf gebündeltem Python; Prüfung von pyvenv.cfg                                                                    | Absoluter Homebrew-Frameworkpfad bestätigt, auch in aktueller Quellruntime; installed-python-linkage.txt als zusätzlicher Artefaktbeleg.                                                    |

check:fast und test:integration wurden nicht zusätzlich identisch wiederholt: npm run check führte sämtliche 153 schnellen und 127 lokalen Rust-Integrationstests aus. build:check wurde durch den oben genannten direkten Cargo-Befehl ersetzt, um bereits vorbereitete Runtime-/Vendorverzeichnisse nicht unnötig neu zu synchronisieren. Das prüft Kompilierung und Linken, jedoch nicht die komplette npm-Prepare-Kette oder Release-Verpackung. Kein npm install/ci, Publish, Commit, Branch, PR oder Push wurde durchgeführt.

### Evidenzklassen und Grenzen

- **LIVE:** Interaktion in der laufenden nativen Anwendung mit tatsächlichem Backend, Prozessen und SQLite. Die Medien sind kontrollierte synthetische Dateien von einem lokalen HTTP-Server. Das ist keine Plattformkompatibilitätsprüfung.
- **AUTOMATED:** Vorhandene automatische Prüfungen mit realen Funktionen, Dateien, SQLite oder Prozessen. Synthetische Eingabedaten allein machen eine echte Runtimeprüfung nicht zum Mock.
- **SIMULATED:** Ergebnisse mit ersetzten Tauri-/Netzantworten oder ausschließlich isoliert nachgebildeten SQL-Daten. Ein automatisierter Browserlauf mit Tauri-Bridge bleibt hier ausdrücklich SIMULATED.
- **NOT TESTED:** Keine praktische Prüfung; der jeweilige Grund steht in der Testmatrix.

Technische Befunde sind im Folgenden zusätzlich als nachgewiesener Fehler, bestätigte UX-/Modellschwäche, technisches Risiko, Performance-Hypothese oder strategische Produktempfehlung bezeichnet. Ein statisch belegter Risikopfad wird nicht als live aufgetretener Fehler ausgegeben.

Nicht geprüft wurden sauberes Fremdgerät, alte macOS-Versionen, Intel, Windows/Linux, signierte/notarisierte Installation, echte Browserkonten, geschützte Medien, private Plattformdaten oder mehrstündige reale Downloads. Vorhandene Nutzerdatenbanken wurden weder migriert noch als Testkopie herangezogen.

## 3. Vollständiges Feature-Inventar

Alle nachstehenden Bereiche sind im Produktcode vorhanden. Implementiert bedeutet nicht automatisch vollständig live abgenommen. Die konkrete Testabdeckung steht in Abschnitt 4.

| Bereich                                         | UI / Einstieg → Handler / Command                                         | Backend → Prozesse / Speicher → Ergebnis                                                 | Status / auffällige Grenzen                                                                                         |
| ----------------------------------------------- | ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Hauptnavigation, Logo, Breadcrumbs, Dock        | src/main.js:484, src/history-details-view.js:318; get_navigation_request  | navigation.rs; dock_menu.rs; navigation:requested                                        | Vier Hauptbereiche, Account-/Posthierarchie; native Dockaktion nur simuliert bzw. statisch geprüft.                 |
| URL und opt-in Video-IDs                        | src/url-utils.js:5; src/main.js:360                                       | url_rules.rs:6; gemeinsame Downloadregeln                                                | Volle HTTP(S)-URLs; aktivierbare YouTube-/TikTok-/Instagram-IDs; ID-Import standardmäßig aus.                       |
| Magic Import                                    | src/main.js:432                                                           | read_clipboard_text; Config last_download_url                                            | Leeres Eingabefeld/Fensterfokus; kann Playlistübernahme auslösen, E5.                                               |
| Metadaten und Thumbnailvorschau                 | src/main.js:1079; load_info                                               | commands.rs:143; metadata.rs:26 → yt-dlp/Deno/Cookies                                    | Titel/Uploader/Dauer, Schutz vor alten Antworten; engerer Autovorschau-Hostfilter, E13.                             |
| Sechs Downloadpresets                           | src/main.js:279; get_download_presets                                     | presets.rs:15; download_rules; worker/download → yt-dlp                                  | Best, bis 1080p, MP3, Opus, Text, Text mit Zeitstempeln; zentrale Presetdefinition.                                 |
| Startschnitt aus URL-Zeitstempel                | src/url-utils.js:69; enqueue_download                                     | download.rs:390; media_tools.rs:4 → FFmpeg → neue Datei                                  | Stream-copy ist nicht bildgenau; reale Abweichung E6.                                                               |
| Outputordner und Dateiaktionen                  | src/main.js; pick_output_dir/open_folder/open_file_path/open_external_url | commands.rs:107; files.rs → Dateisystem/OS-Opener                                        | Persistenter Ordner; lokale kanonische Pfade und geprüfte externe HTTP(S)-URLs.                                     |
| Queue und Fortschritt                           | src/main.js:693, :1355; queue/status/state/progress events                | queue.rs:138; worker.rs → Prozesse; RAM                                                  | FIFO, ein Worker, Prozent/Status/Log; pending nicht persistent, E3.                                                 |
| Pause, Resume, Abbruch, Clear, erneuter Versuch | src/main.js:1530; start/pause/resume_queue, cancel_download               | queue/process/worker, Cancellation hat Vorrang                                           | Pause nach aktuellem; Retry über Formatmenü vorhanden; Shutdown beendet registrierte Prozesse.                      |
| YouTube-Playlists                               | src/main.js:1176; enqueue_playlist                                        | youtube_playlist.rs:139 → flache yt-dlp-Extraktion → einzelne Jobs                       | Unverfügbare Einträge gezählt; Watch+list gilt als Gesamtplaylist; automatischer Import E5.                         |
| TXT-Batch                                       | src/main.js:1451; pick_txt_file                                           | commands.rs:79; src/url-utils.js:310 → Jobs                                              | Kommentare, Normalisierung, Duplikat-/Fehlerzähler; kein zeilenbezogener Abschluss/Importabbruch, E10.              |
| Captions / Beschreibungen                       | src/history-details-view.js:241; get_history_caption                      | yt_dlp.rs:35; download.rs:290; captions                                                  | Text auch ohne exportierte .caption.txt in History; optionaler Sidecar. Nicht mit Untertiteln verwechseln.          |
| Creator Subtitles                               | src/history-subtitles-view.js:25; get_history_subtitle                    | subtitles.rs:77 → yt-dlp-Sidecars + history_subtitles                                    | Originalsprachen/-format, keine Auto-Captions/live_chat; Originalzeit und Schnitt-Offset vorhanden.                 |
| Whisper-Transkription                           | Textpresets; Transkriptqualität/Videoerhalt                               | download.rs:688; media_tools.rs:47 → Python/faster-whisper/FFmpeg → TXT + transcriptions | Sprache, wahlweise Zeiten; base/small/medium/large-v3; E1/E2.                                                       |
| History mit Suche und Filtern                   | src/history-view.js:20; get_history                                       | history.rs:62 → SQLite, 20er-Seiten/OCR-Zusammenfassungen                                | Titel/Beschreibung/User/Transkript; Quellen-/Creatorfilter; keine globale OCR-/Subtitle-Suche.                      |
| Downloadstatistik                               | History-Seitenpanel; get_history_stats; CLI stats                         | history.rs:362 → Summen/Quellen → UI/CLI                                                 | Retained History, keine echte Speicherbelegung aller Artefakte; D5/E6.                                              |
| Einzel-/Gesamtlöschung                          | src/history-view.js:324, :510                                             | history.rs:596 → CASCADE + Accountcleanup                                                | Dateien bleiben; Zusatzdaten werden entfernt; Einzellöschung unbestätigt, E4.                                       |
| Overview / Detaildaten                          | src/history-details-view.js:318; get_history_details                      | history.rs:220 → History und verfügbare Textbereiche                                     | Originalquelle, Creator, Metadaten, Datei-/Textstatus; Lazy-Textladen.                                              |
| Textreader und Kopieren                         | src/history-text-view.js:65; get_history_transcript/subtitle/caption      | Gespeicherte DBtexte → Unicode-Highlight, Keyboard, Clipboard                            | Suche, vollständige Copy-all trotz Filter, SRT/VTT/Whisper-Zeitnavigation; große Reader E15.                        |
| Lokaler Player                                  | src/history-media-view.js:42; get_history_media                           | history.rs:210; history_media.rs → AssetProtocol/FFmpeg/Cache                            | Original oder H.264/AAC-Vorschau, OCR-Bildfallback, kein Remoteembed; E2/E11.                                       |
| OCR-Analyse                                     | src/history-ocr-view.js:589; start/cancel_history_ocr                     | ocr.rs:610 → FFmpeg-Frames → Swift Vision → history_ocr/frames                           | Ein Lauf, macOS, 1-s-Sampling, SHA/Quellvalidierung, alte Resultate bei Fehlschlag.                                 |
| OCR-Fortschritt im Hintergrund                  | src/history-ocr-jobs.js:26; history:ocr                                   | eigener Job-/Prozessstatus und Ownerlock                                                 | Navigation bricht nicht ab; Wiederanlauf markiert verwaiste Versuche unterbrochen.                                  |
| OCR-Suche, Frames, Preview, Volltext            | get_history_ocr/text/frame                                                | ocr_store.rs:172; ocr_preview.rs:78 → paginiertes SQLite/FFmpeg                          | 100er-Seiten, globale Suche je Video, alle Frames kopierbar, revisionierte Auswahl.                                 |
| OCR-Datenpflege                                 | preview/remove_history_ocr_duplicates; delete_history_ocr_frame_text      | ocr_store.rs:330 → vollständiger Bestand, atomare Revision                               | Einzeldelete/Preview/Bestätigung/Dedupe bereits vorhanden; kein begründeter Bedarf an einem weiteren Duplikatknopf. |
| Keywords                                        | src/history-keywords-view.js:3; get_history_keywords                      | keywords.rs:91 → Transcript/OCR/Caption in SQLite                                        | Mindestens fünf Vorkommen, DE/EN-Stopwords, Filter/Suche/200er-Anzeige; E8.                                         |
| Creator / gespeicherte Accountvideos            | src/history-account-view.js:105; get_history_account                      | accounts.rs:129 → History + account_insights                                             | 20er-Seiten, Breadcrumbs; Labelidentität D2.                                                                        |
| Öffentliches Profilvideo-Browsing               | load_history_account_videos                                               | accounts.rs:309 → validierte yt-dlp-Profile, 50er-Referenzen                             | Auswahl befüllt Downloadcomposer; kein vollständiges Archiv; E12.                                                   |
| Video-/Account-Social-Insights                  | src/history-social-insights-view.js:34; get/load_history_*_insights       | social_insights/accounts → native/social-insights.cjs → Fetchary/Chromium → SQLite       | Explizite Abfrage, letzter Snapshot offline, null ≠ 0, vorheriger Snapshot bei Fehler erhalten.                     |
| Browser Authentication                          | src/browser-authentication.js:18; check_youtube_cookie_access             | cookie_sync.rs:8 → yt-dlp → Browserauswahl in Config                                     | Nur YouTube-Cookies, keine Cookiepersistenz; öffentlicher Check beweist keine Anmeldung, E9.                        |
| Browser Import / Link Dump                      | src/browser-import-view.js:3; overview/settings/secrets/restart           | browser_import.rs; link_dump_store.rs → Loopback-HTTP/SQLite                             | Hashsecrets, einmalige Rohsecretanzeige, Widerruf/Löschen, Grenzen/Timeouts; Extension in separatem Repo.           |
| Settings / Diagnose / Versionen                 | src/settings-view.js:127; get/patch_config; get_yt_dlp_installed_version  | config.rs; runtime.rs → SQLitecache/Toolprobe; GitHub-Latestabfrage                      | Autosave mit Fehlerrollback; maximal 500 Logzeilen, Copy/Clear; E7/E9/E16.                                          |
| CLI und Benachrichtigungen                      | cli.rs:84; main.js:1957; Notificationoption                               | authentifiziertes eigenes Loopback-IPC; completion/worker                                | Help/version, Queue add/list/remove, History list25, Stats; Queue-Erfolgsmeldung bei mehreren Jobs.                 |

Die zentrale UI-Backend-Grenze ist src/tauri-client.js:3. Commands sind in src-tauri/src/main.rs:131 registriert. Alle produktiven JS-Module sind angebunden; keine stillgelegte zweite Anwendung wurde gefunden. Die Feinheiten von Fortschritt, Abbruch und Wiederholung sind teils je Pipeline unterschiedlich; insbesondere E3 und E14 begrenzen die gemeinsame Zuverlässigkeit.

## 4. Live-Tests und Testergebnisse

### Testmatrix

| Funktion                                                                   | Testmethode            | Ergebnis                                                                              | Problem / Grenze                                                                                                                | Evidenz                                                                                  |
| -------------------------------------------------------------------------- | ---------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Versionen, JS-Lint, Rust-Clippy, Format                                    | AUTOMATED              | Bestanden                                                                             | Kein Ersatz für installierte App.                                                                                               | check.log; package.json:5.                                                               |
| Regeln, Events, Jobzustände, Persistenz                                    | AUTOMATED              | 95 JS + 280 Rust bestanden, 0 ignored                                                 | 153 fast / 127 integration; Fake-Prozesse in entsprechenden Tests ausdrücklich synthetisch.                                     | check.log; scripts/run-rust-tests.mjs.                                                   |
| Release-Kompilierung                                                       | AUTOMATED              | Bestanden                                                                             | Direkter Cargo-Build; kein DMG-/Prepare-/Installationsgate.                                                                     | build.log.                                                                               |
| Native Navigation, Ordnerspeicherung                                       | LIVE                   | Bestanden                                                                             | Eigenes temporäres Profil; kein persönlicher Zustand.                                                                           | Native CUA-Aktionsfolge; desktop-profile/; CLI-/DBbelege.                                |
| Ungültige URL                                                              | LIVE                   | Bestanden                                                                             | Inlinehinweis mit gültiger URL-/ID-Importhilfe und Fokus; daher nicht pauschal als fehlend gemeldet.                            | Native Eingabe not-a-url → Load info.                                                    |
| Metadaten lokaler HTTP-Quelle                                              | LIVE                   | Bestanden                                                                             | Generic-Extractor liefert hier keine Dauer-/Uploaderdaten.                                                                      | http.log; audit-Eintrag.                                                                 |
| Best-Download → History                                                    | LIVE                   | Bestanden                                                                             | Tatsächlicher yt-dlp, H.264-Datei, Completion in SQLite.                                                                        | cli-live.json; native-final-evidence.json.                                               |
| Max 1080p                                                                  | LIVE                   | Kontrollierter Fehler                                                                 | Generic-Quelle hat kein verwertbares height-Metadatum: Requested format is not available; kein erfolgreicher 1080p-Abnahmetest. | Native Queuezeile für Job 72f9d2cb-96d3-4e6b-a19c-e2c45ba46257.                          |
| MP3 / Opus                                                                 | LIVE                   | Bestanden                                                                             | Kurze kontrollierte Sprachquelle, keine Plattformbehauptung.                                                                    | native-final-evidence.json: ffprobe mp3/opus.                                            |
| Text / Text mit Zeitstempeln                                               | LIVE                   | Bestanden                                                                             | Echtes Base-Modell, Sprache en, TXT und DB; erkannter Produktname ungenau, keine Accuracy-Messung.                              | native-final-evidence.json, Transcripttexte.                                             |
| Whisper PCM / Tiny-Modell                                                  | AUTOMATED              | Bestanden                                                                             | Realer Interpreter/PyAV/Modell, Entwicklerhost mit Homebrew.                                                                    | whisper-decode.log; whisper-live.log.                                                    |
| Wiederholtes Textpreset mit Videoerhalt                                    | LIVE                   | **Fehler reproduziert**                                                               | Neue spoken__2.txt, vorhandene spoken.mp4, trotzdem No saved video.                                                             | native-transcript-collision.png; E2; native-final-evidence.json.                         |
| URL-Startschnitt t=2                                                       | LIVE                   | **Funktionale Abweichung**                                                            | Quelle und Ausgabe 4,909070 s, beide 122 Videoframes; keine genaue Kürzung auf etwa 2,91 s.                                     | cut-live-evidence.json; media_tools.rs:4; E6.                                            |
| Queue-FIFO / Pause / Resume / Cancel / Retry-Races                         | AUTOMATED              | Bestanden                                                                             | Reale kontrollierte Kindprozesse/Loopback und injizierte Workergrenzen, kein mehrstündiger Plattformlauf.                       | queue_failure_tests, process_failure_tests, application_failure_tests im check.log.      |
| Wartende Queue und Auto-start nach Neustart                                | LIVE                   | **Fehler/Lücke reproduziert**                                                         | Vorher ein wartender Job, Auto-start aus; danach leer, Auto-start an.                                                           | queue-before-restart.json; queue-after-restart.json; native AX-Zustände; E3.             |
| Playlist, Magic Import, Watch+list                                         | SIMULATED              | Browserchecks bestanden                                                               | Automatische Gesamtübernahme ist absichtlich getestet; keine reale öffentliche Playlist heruntergeladen.                        | ui.log; test/design-system-ui.mjs:3712; E5.                                              |
| TXT-Batch / ID-Import / Dubletten                                          | AUTOMATED + SIMULATED  | Parser-/Vertrags- und UI-Prüfungen bestanden                                          | Nativer Dateidialog mit großer TXT-Liste nicht praktisch durchgespielt.                                                         | check.log; url-utils/video-input-Tests; E10.                                             |
| Captions / Beschreibungen                                                  | AUTOMATED              | Persistenz-/Fehlerfälle bestanden                                                     | Reale private/öffentliche Postbeschreibungen nicht massenhaft abgefragt.                                                        | persistence_failure_tests; creator-subtitles.log.                                        |
| Creator Subtitles / Wiederverwendung / keine Auto-Captions                 | AUTOMATED              | Bestanden, echter yt-dlp gegen Loopback                                               | Medienfixture synthetisch; kein Plattform-Untertiteldownload.                                                                   | creator-subtitles.log; test/creator-subtitles.integration.mjs.                           |
| Untertitelreader, Sprache, Cut-Offset, Copy                                | SIMULATED              | Chromium/WebKit bestanden                                                             | Native echte mehrsprachige Quelle nicht heruntergeladen.                                                                        | ui.log; test/design-system-ui.mjs; history-media/text-Tests.                             |
| Native Videowiedergabe                                                     | LIVE                   | Bestanden                                                                             | Play/Pause, tatsächliche 8-s-H.264-Datei im WKWebView.                                                                          | Native AX: Play → Pause, Duration 8 seconds; audit-Quelle.                               |
| Automatische Codec-Vorschau                                                | LIVE + AUTOMATED       | Bestanden                                                                             | FFV1/MKV wurde zu lokaler H.264-Vorschau; Original blieb vorhanden.                                                             | Assetpfad im nativen OCR-Screenshot; history_media Runtime-Test; Cache unter Testprofil. |
| OCR ohne sichtbaren Text                                                   | LIVE                   | Bestanden                                                                             | 8-s-Video: erfolgreiche leere Analyse, keine erfundenen Textframes.                                                             | native-db-evidence.json; nativer Status OCR complete.                                    |
| Vision-OCR mit Text / VFR / Zeitauswahl                                    | LIVE                   | Bestanden                                                                             | Vier Frames bei 0/1120/2400/3520 ms; Text 12 Kilometer neue Radwege.                                                            | native-ocr.png; ocr-diagnostic.log; native SQLite.                                       |
| OCR-Suche / Copy / Dedupe                                                  | LIVE + SIMULATED       | Suche und Dedupe live bestanden; Copy vollständig in Browserchecks                    | Suche Kilometer, Auswahl 1,120 s, Bestätigung 3 Duplikate, danach 1 Frame. Clipboard live nicht ausgelesen.                     | native-ocr.png; native-db-evidence.json; ui.log.                                         |
| Native OCR-Smoke                                                           | AUTOMATED              | Zunächst zweimal fehlgeschlagen, später bestanden                                     | 30-s-Response-Timeout ohne Diagnose; später Originalbefehl erfolgreich. Ursache nicht abschließend geklärt.                     | ocr-native*.log; Produktionsdeadline 120 s in ocr.rs:20.                                 |
| OCR-Einzeldelete / Abbruch / altes Ergebnis / fehlendes Video              | AUTOMATED + SIMULATED  | Bestehende Fälle bestanden                                                            | Native Einzeldelete-/Cancel-/Missing-File-Matrix nicht zusätzlich vollständig wiederholt.                                       | ocr_tests.rs; history-ocr-jobs.test.mjs; ui.log.                                         |
| Keywords / Filter / Paging / späte Antworten                               | AUTOMATED + SIMULATED  | Bestanden                                                                             | Quellenzählung bestätigt; bestehende Subtitles fehlen als Input, E8.                                                            | keywords.rs:108; ui.log; fixtures/screenshots/keywords.json.                             |
| Historysuche, Filter, Accountnavigation                                    | SIMULATED              | Chromium/WebKit bestanden                                                             | Unicode-Historygrenze separat reproduziert, D3; normale native History/Persistenz live.                                         | ui.log; db-supplemental-probes.json.                                                     |
| Öffentliche Accountvideos / Account-Insights                               | AUTOMATED + SIMULATED  | Zustände, Pagination, Refreshretention bestanden                                      | Kein fremdes Konto live untersucht; lokale Quellen haben keinen Creator.                                                        | ui.log; accounts-Tests im check.log.                                                     |
| Social-Browser / Fetchary / Cleanup                                        | SIMULATED              | 6/6 bestanden                                                                         | Echter Chromium und echte Extraktion, aber Netz-/HTML-Fixtures; kein Beweis aktueller Social-Site-Kompatibilität.               | social-browser.log.                                                                      |
| Öffentliche YouTube-Metadaten                                              | AUTOMATED, reales Netz | jNQXAC9IVRw: Erfolg, 19 s, public                                                     | BaW_jenozKc: unavailable; dessen Scheitern ist kein allgemeiner YouTube-Defekt. Kein Cookie-/Downloadtest.                      | public-youtube-checkurl-probe.json; public-youtube-probe.json.                           |
| Browser Authentication mit echten Cookies                                  | NOT TESTED             | Bewusst nicht durchgeführt                                                            | Keine Freigabe zur Benutzung eines privaten angemeldeten Profils; keine System-/Keychainrechte verändert.                       | cookie_sync-Test automatisiert; E9.                                                      |
| Link-Dump-Server / Secrets / Auth / malformed HTTP                         | AUTOMATED + SIMULATED  | Vorhandene Sicherheits-/Fehlerprüfungen bestanden                                     | Browserextension selbst nicht im Checkout; keine Live-Kopplung durchgeführt.                                                    | http_failure_tests; application_failure_tests; browser-import-view-Tests.                |
| CLI Help/Version/Fehler/Queue/History/Stats                                | LIVE                   | Bestanden                                                                             | Tatsächliche isolierte Desktop-IPC; Exit 0/1/2, keine History-Schreibkommandos.                                                 | cli-live.json; Queue-/Presetrequest-Dateien.                                             |
| Datenintegrität / Restart                                                  | LIVE + AUTOMATED       | Native quick_check ok, FK-Verletzungen 0; History und Settings über Neustart erhalten | E3 betrifft pending Queue, nicht erfolgreich gespeicherte History.                                                              | native-final-evidence.json; integrity_tests im check.log.                                |
| Migration, WAL-Backup, Full/Readonly/Crash/Rollback                        | AUTOMATED              | Bestanden                                                                             | Kontrollierte Dateien/Fehlersimulationen; keine volle persönliche Festplatte, keine Produktivmigration.                         | integrity_tests.rs:458, :647, :1137, :1572.                                              |
| Settings-Autosave, Modellwahl, Fehlerrollback, Logs/Versionsanzeige        | AUTOMATED + SIMULATED  | Vorhandene Prüfungen bestanden                                                        | Nativ Ordner und Videoerhalt über Neustart bestätigt; vollständige Settings-/Clipboardmatrix nur Fixturechecks.                 | ui.log; test/design-system-ui.mjs:3390; Config-/Versionstests im check.log.              |
| Benachrichtigung / Dockklick / Finderaktionen                              | NOT TESTED live        | Codepfade und teilweise Browser-/Komponententests vorhanden                           | Keine zusätzlichen OS-Berechtigungen oder externen Apps für vollständige manuelle Matrix verwendet.                             | dock_menu.rs; completion/commands; ui.log.                                               |
| Sehr große Reader / lange Netzwerkjobs / parallele reale Plattformaktionen | NOT TESTED live        | Nur relevante deterministische Regressionen und SQL-Benchmark                         | Kurzvideos beweisen keine Stundenlast oder WebView-Reaktionszeit bei 50.000 Segmenten.                                          | E14/E15; Abschnitt 6.                                                                    |
| Sauberer Mac, älteres macOS, Windows/Linux, Releaseinstallation            | NOT TESTED             | Offen                                                                                 | Kein zweites isoliertes Betriebssystem/Endgerät; lokale App-Hülle ist kein Clean-host-Bundle-Test.                              | E1; docs/COMPATIBILITY.md.                                                               |

### Erfolge, Fehlversuche und Reproduktion

Der positive native Ablauf umfasste echten yt-dlp-Download vom lokalen Server, erforderliche Dateiveröffentlichung, History-/Transcriptpersistenz, MP3/Opus mit ffprobe-Kontrolle, zwei Transkriptvarianten, OCR, Suchtreffer, Zeitnavigation, Deduplikation und Neustart. Damit ist mehr als ein Unit-Test geprüft. Acht erfolgreiche Historyeinträge und ein kontrolliert fehlgeschlagenes 1080p-Preset sind in native-final-evidence.json beziehungsweise der Queue beobachtet worden.

Der Native-OCR-Smoke wurde nicht nachträglich als durchgehend grün dargestellt: Die ersten beiden Versuche scheiterten mit Vision response timed out nach 30 Sekunden, ohne verwertbare Diagnostik. Die Initialisierung des Helpers allein antwortete in einer separaten Probe; später funktionierten die Produktionspipeline, eine diagnostische Kopie und der unveränderte Originalsmoke. Ein Warm-up-/Umgebungseffekt ist möglich, aber nicht bewiesen. Die Produktionspipeline erlaubt 120 Sekunden pro Antwort; aus dem 30-s-Testfehler folgt daher kein bewiesener Produktionsausfall.

Wesentliche reproduzierbare Abläufe:

1. **E2:** In einer frischen isolierten Instanz ein kurzes synthetisches Sprachvideo mit Textpreset verarbeiten. Bei bereits vorhandener gleichnamiger TXT den Videoerhalt einschalten und denselben Link erneut mit Textpreset verarbeiten. Die zweite Historyzeile verweist auf spoken__2.txt; spoken.mp4 existiert. Transcript zeigt trotzdem No saved video. Exakte Testjob-ID: 24c32836-76e8-41b5-b493-3a100eeec680.
2. **E3:** Auto-start ausschalten, einen Job hinzufügen, Queue list prüfen, regulär beenden und dieselbe Instanz neu starten. Der Job e3374e19-2033-4a2a-ab96-78f98f9496c7 war vorher wartend, danach fehlt er; Auto-start ist wieder an. Erfolgreiche History und Videoerhaltsetting bleiben erhalten.
3. **E6:** Synthetische H.264/AAC-Quelle spoken.mp4 mit 4,909070 s und langem GOP einmal normal und einmal als Best mit URL-Suffix ?t=2 laden. ffprobe liefert für beide Ausgaben 122 Videoframes und 4,909070 s Containerdauer. Outputnamen zeigen den angeforderten Schnitt. Die Datei wurde veröffentlicht, die gewünschte genaue Anfangskürzung jedoch nicht erreicht.
4. **OCR:** Vorhandener Originalsmoke erzeugt synthetische deutsche Textframes/VFR. Das daraus kontrolliert bereitgestellte MKV in der App downloaden, OCR ausführen: vier echte Textframes. Kilometer suchen, 1,120-s-Zeile wählen, Dedupevorschau bestätigen: drei entfernt, erster Frame erhalten, Revision geändert.
5. **D1/D3:** db_probe.py beziehungsweise die dokumentierten Queries auf der isolierten benchmark.sqlite ausführen. Sie enthalten keine Nutzerdaten und keine Migration am Produktivbestand.

Die temporären Hilfsprogramme und Rohdaten bleiben als lokale Audit-Evidenz verfügbar, sind aber keine dauerhaft installierte Testsuite. Nach Löschen des Tempverzeichnisses müssen synthetische Medien/Benchmarkdaten neu erzeugt werden. Die Reproduktionsbedingungen und Produktfundstellen dieses Berichts bleiben unabhängig davon erhalten.

## 5. Verbesserungen bestehender Funktionen

Dieser Abschnitt enthält ausschließlich Verbesserungen vorhandener Funktionen. Neue eigenständige Produktmodule stehen ausschließlich in Abschnitt 8. Datenbankempfehlungen D1–D8 in Abschnitt 6 gehören ebenfalls zu bestehenden Funktionen.

### E1 — Transkription unabhängig vom Entwicklungsrechner ausliefern

**Kategorie:** nachgewiesene Artefaktabhängigkeit, abgeleitetes Distributionsrisiko. **P1 · L.**

**Bestehende Funktion / Ist:** Textpresets verwenden bevorzugt das gebündelte Python, runtime.rs:180. scripts/prepare-whisper-runtime.sh:71 erstellt eine gewöhnliche venv und prüft auf dem Buildhost. Das tatsächlich gebündelte python3.12 referenziert per otool absolut /opt/homebrew/Cellar/python@3.12/3.12.14/Frameworks/Python.framework/Versions/3.12/Python. pyvenv.cfg zeigt ebenfalls Homebrew. Die aktuelle Quellruntime und das vorhandene Installationsartefakt haben diese Bindung.

**Schwäche / Nutzer:** Nichttechnische Mac-Nutzer können trotz vermeintlich gebündelter Runtime keine Transkription starten, wenn genau dieses externe Framework fehlt. Dieser Ausfall ist aus dem Linkage ableitbar, aber nicht auf einem sauberen zweiten Mac gemessen.

**Verbesserung / Nutzen / Technik:** Eine relocatierbare Python-Runtime einschließlich Standardbibliothek und nativer Pakete bündeln; Bibliothekspfade dürfen nur Systembibliotheken oder Bundleinhalte auflösen. Die bestehende FFmpeg-Bündelung zeigt bereits einen relevanten Umgang mit Libraries, scripts/prepare-ffmpeg-runtime.sh:85. Kein kompletter Transkriptionsrewrite nötig.

**Akzeptanz:** Erzeugte App außerhalb des Buildpfads startet Python ohne Homebrew; echter PCM-Decode und kurze Modellinferenz auf sauberem unterstütztem macOS bestehen; relevante Mach-O-Abhängigkeiten enthalten keine Buildhostpfade. Tests in test/whisper-build.test.mjs:34 bleiben sinnvoll, gelten aber ausdrücklich nicht als Portable-Python-Nachweis.

### E2 — Behaltenes Video explizit mit jedem Transkript verknüpfen

**Kategorie:** nachgewiesener, LIVE reproduzierter Funktionsfehler. **P1 · M–L.**

**Bestehende Funktion / Ist:** Keep video with transcript behält die Videodatei. download.rs:855 veröffentlicht TXT über publish_unique_output; files.rs:57 schützt bestehende Dateien durch __2-Suffix. completion.rs:184 speichert nur den finalen TXT-output_path. history.rs:214 und ocr_source.rs:59 verlangen danach ein Video mit exakt demselben vollständigen Stamm.

**Schwäche / Nutzer:** Bei spoken__2.txt und spoken.mp4 funktioniert der lokale Player der neuen Zeile nicht; LIVE im nativen Transcript-Tab bestätigt. Die OCR-Quellauflösung ist durch dieselbe geprüfte Codekette betroffen, wurde an dieser Kollisionszeile jedoch nicht zusätzlich live gestartet. Rechercheure und Creator verlieren damit einen Kernnutzen gerade nach wiederholter Bearbeitung.

**Verbesserung / Nutzen / Technik:** Tatsächlichen retained media_path beim Completionabschluss explizit speichern und ID-bezogen auflösen. Additive nullable Artefaktreferenz; Altbestand nur eindeutig zuordnen. Die bestehenden konservativen Stammregeln und der No-clobber-Schutz bleiben für Legacydaten erhalten.

**Akzeptanz:** Zweiter/dritter TXT-Export bleibt mit demselben richtigen Video verbunden; Originaldateien unverändert; Neustart erhält Referenz; fehlendes/mehrdeutiges Video liefert kontrollierten Fehler. Keine pauschale Entfernung von Kollisionssuffixen; bestehende Ambiguitätstests in ocr_tests.rs:1827 bleiben erfolgreich. Evidenz: native-transcript-collision.png und native-final-evidence.json.

### E3 — Vorbereitete Queuearbeit und Queue-Modus über Neustarts erhalten

**Kategorie:** bestätigte, LIVE reproduzierte Workflow-Lücke. **P1 · M–L.**

**Ist / Schwäche:** queue.rs:138 speichert pending nur im Mutex<VecDeque>; state.rs:32 und startup.rs:20 erzeugen frischen Queuestate. Auto-start startet wieder mit true. Completionreceipts sichern Ausgaben/idempotente Speicherung, ausdrücklich keine wartende Queue. Ein regulärer Neustart verlor im Test den wartenden Job.

**Nutzer / Nutzen:** Lange Playlist-/TXT-Sammlungen sollen nicht neu zusammengestellt werden müssen. Nichttechniker müssen erkennen, was nach Beenden fortgesetzt wird.

**Verbesserung / Technik:** Pending-Jobdaten, Optionen, Reihenfolge und Betriebsmodus atomar sichern. Laufende Jobs beim Neustart als unterbrochen behandeln; unklare Abschlüsse nicht automatisch wiederholen. Vorhandene Receipts und Dateibesitzregeln verwenden. Bis dahin beim Beenden auf wartende Arbeit hinweisen.

**Akzeptanz:** 100er-Queue mit Pause über Neustart identisch; Entfernen/Crash und Enqueue/Crash definiert; laufender Job nicht doppelt abgeschlossen; gespeicherte Downloads nicht erneut geladen; Auto-start bleibt bewusst gewählt. Evidenz: queue-before/after-restart.json, queue.rs:155, completion.rs:1.

### E4 — Vorhandene History-Löschung vor Fehlbedienung schützen

**Kategorie:** bestätigte UX-Schwäche mit destruktiver Wirkung. **P1 · S.**

**Ist / Schwäche:** src/history-view.js:324 ruft beim X sofort removeHistoryEntry auf. history.rs:596 löscht samt Cascades; OCR, Subtitles, Captions und Socialdaten verschwinden, die Mediendatei bleibt. Nur Clear History fragt nach; dessen Text nennt bislang vor allem Einträge/Transkripte, src/history-view.js:515.

**Nutzer / Nutzen:** Alle Nutzer, besonders Rechercheure mit aufwendig erstellter Analyse. Eine knappe Bestätigung verhindert, dass ein einzelner Fehlklick einzigartige gespeicherte Daten entfernt.

**Verbesserung / Technik:** Vorhandenen OJ-confirmDialog für Einzeldelete einsetzen; Titel und tatsächlichen Löschumfang verständlich nennen. Clear-Dialog entsprechend präzisieren; Fokus nach Erfolg sinnvoll weiterführen. Kein neues Papierkorbmodul erforderlich.

**Akzeptanz:** Abbrechen sendet keinen IPC-Aufruf; Bestätigen genau einen; Dateien bleiben ausdrücklich genannt; Fehler erhält sichtbaren Eintrag; Tastaturfokus bleibt nutzbar. Bestehende atomare Löschregeln bleiben erhalten.

### E5 — Die vorhandene Playlistübernahme bewusst auslösen

**Kategorie:** bestätigte UX-Schwäche, ausdrücklich absichtlich implementiert und getestet. **P1 · M.**

**Ist / Schwäche:** src/main.js:1678 enqueued einen erkannten Playlistlink nach 600 ms; Load Info führt ebenfalls dorthin. Magic Import tut dies nach Fensterfokus, :432. Ein watch-Link mit list wird in src/url-utils.js:37 zur Gesamtplaylist. Auto-start ist standardmäßig an. Ein Link zur Einzelvideoansicht kann deshalb deutlich mehr Arbeit auslösen als erwartet.

**Nutzer / Nutzen:** Rechercheure, Nichttechniker und Power-User mit großen Listen erhalten Kontrolle über Netz, Speicher und Modellarbeit.

**Verbesserung / Technik:** Die vorhandene Extraktion vor der Übernahme zeigen: verfügbare Zahl, ausgelassene Einträge, Preset, Ziel und bei watch+list die Wahl Einzelvideo/Gesamtliste. Erst explizit übernehmen; optional bewusst aktivierter Schnellpfad. Auflösen und Enqueue im bestehenden Playlistpfad trennen.

**Akzeptanz:** Paste/Fokus allein startet keinen Batch im sicheren Standardpfad; verspätete Auflösung ersetzt keine neue Eingabe; Wiederholklick dedupliziert; gewählter Queue-Modus gilt. Tests in test/design-system-ui.mjs:3712 müssen das geänderte Produktverhalten bewusst abbilden, nicht als vermeintliche Regression missverstanden werden.

### E6 — Startschnitt, lokale Zeitlinie und gespeicherte Dauer zusammenführen

**Kategorie:** LIVE nachgewiesene Schnittungenauigkeit; zusätzlich bestätigte Metadatenherkunft. **P1 · M.**

**Ist:** media_tools.rs:4 baut -ss vor -i mit -c copy und -avoid_negative_ts make_zero. Ein erfolgreiches FFmpeg und gültiger Output garantieren dabei keinen genauen Anfang. Im Test blieb t=2 bei voller 4,909070-s-Dauer und 122 Videoframes. download.rs:390 veröffentlicht den Schnitt; history_rules.rs:27 und completion.rs:178 übernehmen weiterhin Quelllaufzeit statt gemessener Exportlaufzeit.

**Nutzer / Nutzen:** Creator und Journalisten benötigen tatsächlich den gewählten Ausschnitt sowie verlässliche Text-/Videozeitbezüge.

**Verbesserung / Technik:** Für die bestehende Schnittoption präzise Kürzung ermöglichen oder ausdrücklich schnelle ungefähre Kürzung mit ermitteltem effektivem Start beschreiben. Export mit ffprobe prüfen; Quellzeit, angeforderten Start, tatsächlichen Start und Exportdauer unterscheiden. Der bereits vorhandene Subtitleoffset muss zum tatsächlichen lokalen Timelineursprung passen.

**Akzeptanz:** Lang-GOP-Quelle bei t=2 enthält die vorangehenden zwei Sekunden im präzisen Pfad nicht; Video/Audio sind synchron; neue Historydauer entspricht Exportdefinition; Originaldauer bleibt nachvollziehbar; Untertitel-/OCR-/Transcriptseeking wird mit derselben Zeitbasis geprüft. Legacywerte nicht nachträglich als gemessene Exportdauer ausgeben. Evidenz: cut-live-evidence.json; subtitles.rs:10.

### E7 — Fehler am Ort der Aktion mit einer nächsten Handlung erklären

**Kategorie:** bestätigte UX-Schwäche. **P1 · S–M.**

**Ist / Schwäche:** Metadatenfehler in src/main.js:1115 und Enqueuefehler :1346 landen primär im Settings-Log; Historylesefehler in src/history-view.js:411 ebenfalls. Laufzeitfehler in Queuezeilen sind dagegen bereits sichtbar. Ungültige URLs haben bereits gute Inlinehilfe und sind ausdrücklich kein fehlender Fall.

**Nutzer / Nutzen:** Besonders Anwender ohne Toolwissen sollen fehlendes yt-dlp, ungültigen Zielordner oder Netzprobleme ohne Logsuche beheben können.

**Verbesserung / Technik:** Bestehende Statusflächen mit sicheren Fehlerkategorien und Retry/Settings/Choose-Hilfe versehen. Eingaben und letzte gültige Ergebnisse bewahren; technische Details weiter im Log. Das ist die Vervollständigung bestehender Fehlerpfade.

**Akzeptanz:** Fehlendes Tool, nicht beschreibbares Ziel und Netz-/Zugangsproblem haben passende Handlungen; Retry erzeugt keinen Doppeljob; Erfolg entfernt alte Fehler; keine privaten Cookie-/Prozessdaten gelangen durch neue Hilfe in UI oder Persistenz.

### E8 — Keywords zu vorhandenen Textbelegen führen und Subtitles einbeziehen

**Kategorie:** bestätigte Workflow-Lücke. **P2 · M.**

**Ist / Schwäche:** src/history-keywords-view.js:48 erzeugt statische Begriff-/Countzellen ohne Sprung zur Fundstelle. keywords.rs:108 liest Transcriptions/OCR/Captions, aber keine history_subtitles. Ein Video mit Creatoruntertiteln kann daher lesbaren gespeicherten Text besitzen, den Keywords nicht berücksichtigt. Aktuelle Dokumentation beschreibt die drei vorhandenen Quellen korrekt.

**Nutzer / Nutzen:** Rechercheure und Creator können gezählte Begriffe überprüfen und wiederverwenden, statt selbst erneut den richtigen Reader zu suchen.

**Verbesserung / Technik:** Begriff/Quellcount öffnet den vorhandenen Reader mit Suchbegriff. Vorhandene Creatortracks als klar bezeichnete Quelle ergänzen; track- und sprachbezogene Zählweise bewusst festlegen, damit parallele Sprachen nicht unbemerkt alles mehrfach zählen. Bestehende Unicode-/Stopwordregeln und Limits verwenden.

**Akzeptanz:** Richtiger Tab/Eintrag/Filter nach Klick und Tastatur; vollständige Quelle durchsucht, Rückkehr erhält Keywordfilter. Subtitle-only-Video liefert erwartete Begriffe; Sprachen/Tracks und doppelte Textquellen sind verständlich ausgewiesen. Keine semantische KI oder neues globales Suchmodul erforderlich.

### E9 — Browserzugriff präzise bestätigen und sicher diagnostizieren

**Kategorie:** bestätigte Aussagegrenze/UX-Schwäche. **P2 · S–M.**

**Ist / Schwäche:** cookie_sync.rs:6 nutzt eine öffentliche URL; Exit 0 bedeutet öffentlichen Metadatenabruf mit Cookiezugriff, keinen bewiesenen Login an geschütztem Inhalt. Spawn-, Timeout-, Netzwerk- und Profilfehler werden :34 sämtlich COOKIE_ACCESS_FAILED. Das schützt private Ausgaben, erschwert aber Abhilfe.

**Nutzer / Nutzen:** Nutzer mit mehreren Browserprofilen, macOS-Speicher-/Keychainrechten oder Loginproblemen erhalten korrekte Erwartungen und konkrete Hilfe.

**Verbesserung / Technik:** Erfolg als geprüften Browserzugriff formulieren; sichere Kategorien für Profil, Rechte, Netz/Timeout und Tool ergänzen. Keine Rohstderr-/Cookieausgabe; keine eigenmächtige Berechtigungsänderung.

**Akzeptanz:** Öffentlicher Test behauptet keinen Nachweis einer angemeldeten Sitzung; Fehler verändert keine bestehende Auswahl; Unterdrückungstests cookie_sync.rs:53 bestehen; geschützte Inhalte ausschließlich bei ausdrücklich berechtigter Abnahme testen.

### E10 — TXT-Import zeilenbezogen, fortlaufend und abbrechbar abschließen

**Kategorie:** bestätigte UX-/Workflow-Lücke. **P2 · M.**

**Ist / Schwäche:** src/url-utils.js:310 behält normalisierte Links/Zähler, verliert aber Zeilennummer und konkreten Verwerfungsgrund. src/main.js:1487 enqueued seriell und zeigt erst Endsumme; kein Stop für noch nicht übernommene Zeilen.

**Nutzer / Nutzen:** Power-User und Rechercheure mit großen Linklisten können fehlerhafte Teile korrigieren, ohne erfolgreiche Imports erneut einzuspeisen.

**Verbesserung / Technik:** Bestehenden Import um n/N, Zeile+Fehlergrund und gezieltes Wiederholen fehlgeschlagener Eingaben erweitern. Stop beendet weitere Übernahme, bereits enqueuete Jobs bleiben klar zugeordnet. Dateien bleiben unverändert.

**Akzeptanz:** 1.000 kontrollierte Links bleiben bedienbar; monotone Fortschrittsanzeige; keine erneute Aufnahme erfolgreicher Zeilen bei Retry; Stop erzeugt keine weiteren Jobs; vorhandene URL-/Queue-Deduplizierung bleibt wirksam.

### E11 — Lebenszyklus der bestehenden Playback-Vorschau begrenzen

**Kategorie:** bestätigte fehlende Cachepflege; Größenwirkung nicht gemessen. **P2 · M.**

**Ist / Schwäche:** history_media.rs:19 erzeugt je Quellfingerprint einen neuen MP4-Namen. Änderungserkennung und Wiederverwendung funktionieren; frühere Derivate werden nicht begrenzt oder bei Historycleanup bereinigt. Das kann den lokalen Speicher schleichend füllen.

**Nutzer / Nutzen:** Große Archive und kleine SSDs behalten zuverlässige Playerfunktion ohne unüberschaubare Kopien.

**Verbesserung / Technik:** Größen-/Altersbudget für abgeschlossene, unbenutzte Derivate; sicheren Bezug zu Quellidentitäten und laufenden Readern halten. Bestehende Settings-/Dateianzeige kann den Cacheumfang erklären.

**Akzeptanz:** Nur entbehrliche Derivate werden entfernt; aktive Wiedergabe/Conversion wird nicht beschädigt; fehlender Cache regeneriert; Originaldateien unverändert; fehlender Speicher ist nachvollziehbar. Keine unbewiesene aktuelle Plattenüberfüllung behauptet.

### E12 — Profilvideo-Kompatibilität konsistent erklären und validieren

**Kategorie:** bestätigte Filterinkonsistenz, Plattformwirkung nicht live geprüft. **P2 · S–M.**

**Ist / Schwäche:** accounts.rs:155 akzeptiert Profile unter anderem für Vimeo/Twitch/LinkedIn. fetch_videos filtert Referenzen :392 durch normalize_video_url; video_urls.rs:12 erlaubt nur den engeren Importvertrag für YouTube/TikTok/Instagram/Facebook/X/Reddit. Erfolgreich gelieferte andere Referenzen können verschwinden und wie 0 Videos wirken.

**Nutzer / Nutzen:** Creator-/Rechercheansichten unterscheiden echte leere Profile von nicht unterstützter Discovery.

**Verbesserung / Technik:** Für die vorhandene Profilansicht eine passende, an das nachgewiesene Profil gebundene Referenzvalidierung verwenden oder nicht unterstützte Profile vor dem Abruf verständlich kennzeichnen. Die schmale externe Link-Dump-Allowlist nicht blind ausweiten.

**Akzeptanz:** Kontrollierte gültige Referenz jeder zugesagten Profilart wird angezeigt; fremde Profile abgewiesen; unsupported und empty sind verschieden; dokumentierte YouTube-Videos-/Shorts-/Livestreamgrenze bleibt sichtbar.

### E13 — Vorhandene Linkprüfung ohne widersprüchliches Unsupported-Label

**Kategorie:** bestätigte UX-Inkonsistenz. **P2 · S.**

**Ist / Schwäche:** src/main.js:1077 und src/url-utils.js:128 begrenzen automatische Vorschau auf bekannte Hosts. Andere syntaktisch gültige HTTP(S)-Links heißen :1669 Unsupported link, obwohl manuelles Load Info und Enqueue sie akzeptieren und yt-dlp sie gegebenenfalls unterstützt.

**Nutzer / Nutzen:** Power-User können unterstützte Extractors außerhalb der Autovorschauliste verständlich ausprobieren.

**Verbesserung / Technik:** Neutraler Hinweis zum manuellen Prüfen bis zur tatsächlichen Extractorantwort; gemeinsame Begrifflichkeit für automatische Vorschau, Batchimport und freien Download.

**Akzeptanz:** Unbekannter gültiger Host gilt nicht ungeprüft als unsupported; manuelle Aktion bleibt sichtbar; echte ungültige Eingaben/Steuerzeichen bleiben abgewiesen. Keine pauschale Zusage sämtlicher yt-dlp-Websites.

### E14 — Den tatsächlichen Streaming-Prozessabschluss begrenzen

**Stand 2.7.0:** Die doppelte Download-/Whisper-Prozesssteuerung wurde durch process::run_command_streaming ersetzt, einschließlich begrenztem Drain, registriertem Kind bis zum Abschluss und begrenzten Streamingzeilen. Der nachfolgende Auditbefund beschreibt den Ausgangsstand; die separate Frage nach Gesamt-Outputbudgets des allgemeinen read_to_end-Runners bleibt offen.

**Kategorie:** statisch belegtes technisches Risiko, kein reproduzierter Live-Hänger. **P2 · M.**

**Ist / Schwäche:** Der allgemeine process.rs:123 besitzt begrenzten Outputdrain. Die besonderen Download-/Whisper-Streamingpfade in download.rs:210 und :818 räumen hingegen den aktuellen Child auf und joinen danach Readerthreads unbegrenzt. Ein geerbter, offener Pipewriter könnte die Queue trotz beendetem Direktprozess halten. Allgemeine Runnerregressionen beweisen diesen speziellen Pfad nicht.

**Nutzer / Nutzen:** Alle Queuenutzer erhalten zuverlässigen Abschluss/Abbruch auch bei ungewöhnlichem Verhalten externer Komponenten.

**Verbesserung / Technik:** Vorhandene Prozessverwaltung mit begrenztem Drain auch für Streaming teilen; Registrierung bis zum kontrollierten Abschluss behalten. Zusätzlich Daten-/Zeilenlimits beim Lesen durchsetzen, nicht erst nach read_to_end oder fertigem Social-Output.

**Akzeptanz:** Kontrollierter Nachkomme mit offener Pipe blockiert den Streamingjob nicht unbegrenzt; Cancel/Exit beendet relevante Gruppen; letzte Metadaten-/Sprachzeilen bleiben erhalten; lange legitime Downloads erhalten keine willkürlich kurze Gesamtlaufzeit.

### E15 — Vorhandene Text-/OCR-Reader bei großen Ergebnissen entlasten

**Kategorie:** bestätigte Verarbeitungspfade, Performancewirkung überwiegend Hypothese. **P2 · S–M für OCR-Debounce; M für große Reader.**

**Ist / Schwäche:** OCR startet auf jedem Input sofort eine Abfrage, src/history-ocr-view.js:644, und leert Ergebnisrows vor Erfolg :450. Alte Antworten werden zwar verworfen, gestartete SQL-Arbeit bleibt. Textreader bauen alle Zeilen ins DOM, src/history-text-view.js:82; Auswahl und zeitliche Suche laufen linear, history-text.js:54.

**Nutzer / Nutzen:** Mehrstündige Interviews und lange OCR-Ergebnisse sollen während Suche/Wiedergabe bedienbar bleiben.

**Verbesserung / Technik:** OCR-Suche wie bestehende Historysuche kurz entprellen; letzten passenden Snapshot bis Erfolg/Fehler halten. Für große Textreader erst 10.000/50.000 Segmente im WKWebView messen, danach Fenster-/Seitenrendering und effizientere Zeitsuche.

**Akzeptanz:** Schnelle Eingabefolge verursacht nur den final geplanten Suchaufruf; Query/Snapshot eindeutig; Fehler bietet Retry ohne irreführendes leeres Ergebnis. Jeder Treffer bleibt erreichbar, Volltextcopy vollständig, Fokus und Timesync korrekt. Keine gemessene Freeze-Behauptung.

### E16 — Redundante Metadatenarbeit und defekte QA-Verweise reduzieren

**Stand 2.7.0:** Die doppelte initiale Accountabfrage ist durch Hydration des bereits gelesenen Snapshots beseitigt. Die übrigen Punkte zu Queue-Thumbnails und Dokumentationslinks bleiben offen.

**Kategorie:** nachgewiesene Redundanz bzw. Dokumentationslücke. **P2 · S–M.**

**Ist / Schwäche:** src/history-account-view.js:116 lädt Page inklusive Insights; :135 und der Adapter :45 laden dieselbe Accountpage erneut für die Social-Komponente. Queue-Thumbnails können zusätzliche loadInfo-Aufrufe starten, src/main.js:1016. Bereits maximal zwei gleichzeitige Hydrationen vorhanden. docs/TESTING.md:12 und docs/ARCHITECTURE.md:175/:227 verweisen auf nicht vorhandene DEVELOPMENT.md bzw. DATA-INTEGRITY.md.

**Nutzer / Nutzen:** Accountöffnung benötigt weniger DB-Arbeit; Metadaten laden seltener doppelt; Entwickler können die dokumentierte QA tatsächlich reproduzieren.

**Verbesserung / Technik:** Vorhandenen Accountsnapshot direkt übergeben, laufende identische Previewanfragen begrenzt teilen; lokale Dokumentationsziele korrigieren. Keine unkontrollierte dauerhafte Cacheebene einführen.

**Akzeptanz:** Eine Accountöffnung erzeugt eine lokale Pagequery; Auth-/URL-/Settingänderung invalidiert geteilte Requests; Fehler bleibt retrybar; lokale Dokumentationslinks sind auflösbar. Diese Änderungen können unabhängig voneinander geliefert werden.

## 6. Datenbank- und Schema-Audit

### Aktueller Stand und Beziehungen

Das aktuelle Schema ist **user_version 10**, database.rs:20; dies wurde auch an der frisch von der nativen App angelegten Testdatenbank bestätigt. Es gibt 13 reguläre Tabellen, sieben benannte und 14 SQLite-Autoindizes. Keine FTS-Tabelle. app_meta ist nur eine akzeptierte historische Struktur und wird nicht frisch angelegt, database.rs:580.

| Tabelle                  | Identität / zentrale Felder                                                                                         | Beziehungen und Bewertung                                                                                                          |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| app_config               | Singleton id=1; Runtimepfad, Ordner, Preset/Modell, Optionen, Cookiebrowser, Subtitle-/ID-Importsetting, Zeitfelder | CHECK(id=1); boolsche Werte NOT NULL, Regeln zusätzlich in Rust. database.rs:832, :349, :51, :22.                                  |
| link_dump_settings       | Singleton id=1; enabled, host, port, Zeitfelder                                                                     | Standard Loopback:2255; Serverregeln im Adapter. database.rs:807.                                                                  |
| link_dump_secrets        | id; Name, secret_hash UNIQUE, created/last_used/revoked/deleted                                                     | Rohsecret nicht persistent; Revocation/Softdelete bewusst. database.rs:871; link_dump_store.rs:185.                                |
| history_entries          | id; Quelle/URL/Creator/Datei, Herkunftsmetadaten, SHA, medium, output_path, Appversion, Zeitfelder                  | Metadaten dürfen unbekannt bleiben; source/platform teilweise Legacyredundanz. database.rs:882.                                    |
| transcriptions           | id; history_entry_id UNIQUE, Text, Typ, Sprache                                                                     | FK History ON DELETE CASCADE; Typ CHECK. database.rs:903.                                                                          |
| captions                 | PK(history_entry_id,media_path); caption_path nullable, Text, SHA, created_at                                       | FK History CASCADE; seit Schema 9 Text unabhängig von exportierter Datei. database.rs:26.                                          |
| legacy_imports           | name; completed_at                                                                                                  | Idempotenter Importbeleg. database.rs:73.                                                                                          |
| job_completions          | job_id; URL, output_path, processing/output_ready/complete, history_entry_id UNIQUE, updated_at                     | FK History SET NULL absichtlich: gelöschte History wird nicht heimlich wiederhergestellt. database.rs:78; completion_store.rs:115. |
| history_ocr              | PK/FK History; Resultat/Provenienz, Legacytext/JSON, Versuch, Revision/Format                                       | CASCADE; Ergebnis und letzter Versuch getrennt; alte Ergebnisse bleiben bei Fehler. database.rs:91/:110.                           |
| history_ocr_frames       | id; history_entry_id, timestamp_ms, frame_key, full_text, blocks_json                                               | FK OCR CASCADE; Zeiten/Keys nichtnegativ; UNIQUE(entry,key), nullable Legacykey; chronologische Seiten. database.rs:112.           |
| history_social_insights  | PK/FK History; source_url, fetched_at, fetchary_version, payload_json                                               | CASCADE; Zeit-/Payloadchecks, Rust-DTO-/Bytelimits. database.rs:126; social_insights.rs:159.                                       |
| history_account_insights | PK(source,uploader); profile_url, Zeitpunkt/Version/Payload                                                         | Geteilter letzter Snapshot, keine FK auf einzelnen Download; expliziter Last-entry-Cleanup. Identitätsproblem D2. database.rs:137. |
| history_subtitles        | PK(history_entry_id,path); Sprache, Format, Text, source_start_seconds                                              | FK History CASCADE; Sidecarverlust lässt DBtext erhalten. subtitles.rs:10.                                                         |

Die JSON-Spalten sind für atomare OCR-/Engine-/Social-Snapshots zweckmäßig. Es gibt aktuell keine globale Abfrage einzelner Social-JSON-Felder, die eine vollständige relationale Zerlegung verlangen würde. OCR full_text und blocks_json haben getrennte Aufgaben: günstiges Lesen/Suchen versus ursprüngliche Blockprovenienz. Nicht pauschal eines davon entfernen.

Nullable Metadaten entsprechen echten unbekannten Werten. Einzelne historische TEXT-PKs sind auf SQLiteebene nicht gleichbedeutend mit NOT NULL; Anwendung und Migrationsprüfungen erzeugen/fordern verwendbare IDs, database.rs:783. Daraus wurde keine normale UI-Datenkorruption abgeleitet. Constrainthärtung nur gezielt und migrationssicher, keine kosmetischen Tabellenneubauten.

Benannte Indizes:

| Index                                                            | Aktuelle Aufgabe / Befund                                                                                                         |
| ---------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- |
| idx_link_dump_secrets_active(revoked_at,deleted_at)              | Aktive Secretfilter; nicht allein wegen Tabellen-PK redundant.                                                                    |
| idx_history_entries_completed_at(completed_at,created_at)        | Entspricht nicht exakt der tatsächlichen COALESCE-Sortierung, D1.                                                                 |
| idx_history_entries_sha256(sha256)                               | Keine aktuelle Produktabfrage auf SHA gefunden, D7.                                                                               |
| idx_captions_sha256(sha256)                                      | Keine aktuelle Produktabfrage auf SHA gefunden, D7.                                                                               |
| idx_history_ocr_frames_order(history_entry_id,timestamp_ms,id)   | Begründeter Page-/Export-/Dedupepfad.                                                                                             |
| idx_history_ocr_frames_sample UNIQUE(history_entry_id,frame_key) | Verhindert doppelte native Samples; nullable Legacykeys bewusst.                                                                  |
| idx_history_account_lookup(source,uploader)                      | Direkter exakter Filter kann ihn nutzen; generischer OR-/NOCASE-Pfad und lower(source)-Cleanup tun dies nicht zuverlässig, D1/D6. |

### Migrationen und bereits abgesicherte Integrität

| Schema | Veränderung                                                                  |
| -----: | ---------------------------------------------------------------------------- |
|      1 | Erkannte historische Layouts übernehmen; bekannte Spalten/Indizes ergänzen.  |
|      2 | Legacyimport-/Completionbelege.                                              |
|      3 | OCR-Ergebnis und Versuchszustand.                                            |
|      4 | OCR-Frames, Revision/Format, verifizierte Übertragung alter Segmente.        |
|      5 | Video-Social-Snapshots.                                                      |
|      6 | Account-Snapshots und Creatorlookupindex.                                    |
|      7 | Browserauswahl für YouTube-Cookies.                                          |
|      8 | Creator Subtitles und Setting.                                               |
|      9 | Captiontext unabhängig von optionaler Sidecardatei, sicherer Tabellenneubau. |
|     10 | ID-Import, standardmäßig aus.                                                |

Quellen: database.rs:300–376, ocr_store.rs:423; historische Fixtures siehe src-tauri/test-fixtures/README.md:3. Die neun benannten Release-Fixtures sind historische Layouts, keine neun aktuellen Migrationen.

Die vorhandene Datenbankabsicherung ist eine Stärke:

- Fehlerhafte oder unlesbare vorhandene DB wird nicht durch eine leere Datei ersetzt; zukünftige Schemaversion wird verweigert, auch unter jedem Writerlock erneut: database.rs:165, :212, :225.
- foreign_keys vor Transaktion, BEGIN IMMEDIATE, drei Sekunden SQLite-Locktimeout, Versionsrecheck nach konkurrierender Migration; Upgrades in einer Transaktion: database.rs:253–386.
- Vor einem Upgrade vorhandener DB ist ein Backup mit SQLite-Backup-API Pflicht. Private Partialdatei, Integritätskontrolle, fsync und No-clobber-Veröffentlichung; Backupfehler verhindert Migration, Deadline 30 Sekunden: database.rs:411–472.
- quick_check, foreign_key_check und Layout-/Index-/FK-Prüfung: database.rs:389, :529. Bekannte Lücke D4 eng abgrenzen.
- History, Transcript, Captions, Subtitles und Receipt committen atomar; Dateien/Hashes vorher außerhalb der Writertransaktion: completion_store.rs:103; completion.rs:158.
- OCR-Replacement, Einzeldelete und komplette Deduplizierung sind revisionsgeschützt und atomar. Ein fehlgeschlagener Lauf ersetzt keine alten Frames: ocr_store.rs:54, :172, :330.
- Vorhandene Tests prüfen Migrationen/Nullwerte, Rollback, zukünftige/kaputte Schemas, WAL-Snapshots, konkurrierende Migration/Completion, READONLY/Full/Locks, Importmarker und Prozesskill: integrity_tests.rs:458, :560, :602, :647, :694, :730, :1137, :1245, :1303, :1391, :1572, :1737.

Eine Anwendung mit neuem Schema kann nicht beliebig von einer älteren Version beschrieben werden; die Versionguards verweigern dies bewusst. Das ist sichere Vorwärtsabwehr, keine Zusage eines Downgrades. WAL wird nicht generell für Produktion aktiviert; WAL-bezogene Tests beweisen die Snapshot-/Migrationssicherheit bei entsprechendem Datenbankzustand.

### Nachgewiesene Querypläne und isolierte Messungen

history.rs:94–140 verwendet für jede Seite COUNT plus eine generische optionale Filterquery. Die Sortierung lautet COALESCE(completed_at,created_at), created_at, id. Der vorhandene Index passt dazu nicht exakt. EXPLAIN der unverändert übernommenen Pagequery zeigt **SCAN history + USE TEMP B-TREE FOR ORDER BY**, trotz LIMIT 20. Die entsprechende genaue Accountpage zeigt ebenfalls Scan/Sortierung; ein direkter source/uploader-Filter kann dagegen den bestehenden Lookupindex als SEARCH verwenden.

Benchmarkbedingungen: Drei Wiederholungen je Query, Median, möglicher warmer OS-/SQLitecache; Source-SQL zu Schema 10 rekonstruiert; kurze Titel/URLs, 1.000 wiederkehrende Creator, keine Transcripts/OCR-/Subtitle-/Social-Payloads. SQLite 3.54.1 aus Python. Die folgenden Zahlen sind **SIMULATED SQL-Benchmarks**, keine App-/WebView-Latenzen und keine echten Nutzerarchive.

| Historyzeilen | DB-Datei ohne Experimentalindex | Erste Page20, ms | Tiefe Page20, ms | Titel-Substring, ms | COUNT, ms | Accountpage, ms |
| ------------: | ------------------------------: | ---------------: | ---------------: | ------------------: | --------: | --------------: |
|           100 |                       172.032 B |            0,203 |            0,136 |               0,025 |     0,009 |           0,025 |
|        10.000 |                     2.396.160 B |            4,758 |           18,511 |               1,392 |     0,127 |           1,337 |
|       100.000 |                    22.982.656 B |           69,001 |          285,037 |              16,937 |     4,177 |          17,118 |
|     1.000.000 |                   230.850.560 B |          515,638 |        2.261,087 |             192,572 |    59,014 |         218,858 |

Tiefe Seite: OFFSET n−20, LIMIT 20. Ein **nur in der temporären Benchmarkdatenbank** angelegter Vergleichsindex auf COALESCE(completed_at,created_at) DESC, created_at DESC, id DESC reduzierte dort die erste Millionenseite auf 0,153 ms und entfernte die temporäre Sortierung aus dem Plan. Das ist eine konkrete Querybegründung, **keine** Behauptung einer entsprechenden Beschleunigung der gesamten App: COUNT, Textfilter, tiefe OFFSETs, andere SQLiteversion, echte Payload und Rendering bleiben separat.

### Probleme und konkrete Verbesserungen

| Datenbankproblem                                 | Aktueller Zustand                                                                  | Zukünftiges Risiko                                         | Verbesserung                                                                     | Aufwand | Priorität |
| ------------------------------------------------ | ---------------------------------------------------------------------------------- | ---------------------------------------------------------- | -------------------------------------------------------------------------------- | ------- | --------- |
| D1: History-/Accountpage scannt/sortiert         | Unpassender Sortierindex, generische OR-/NOCASE-Prädikate; gemessen                | Häufige Wartezeit und langer gemeinsamer DB-Guard          | Spezifische Querypfade, passender Sortierindex, Keysetpagination, Countstrategie | M       | P1        |
| D2: Creatoridentität aus Anzeigename             | PK(source,uploader); gecachte Profiladresse vor erneuter Videoauflösung            | Falsch vereinte Creator bzw. getrennte Umbenennungen       | Stabile Plattformidentität/Profilebeleg; Label separat                           | L       | P1        |
| D3: Unicode-Historysuche                         | Österreich/österreich und Überblick/überblick liefern unterschiedliche Treffer     | Recherchen übersehen gespeicherte Inhalte                  | Einheitliche ausdrücklich definierte Unicode-Semantik                            | S–M     | P2        |
| D4: Pflichtprüfung für Subtitlestabelle fehlt    | Validator prüft vorhandene Objekte, fordert Tabelle ab Schema 8 nicht ausdrücklich | Späteres Detailversagen bei beschädigtem Layout            | Versionabhängige Pflichtobjekte vollständig verlangen                            | S       | P2        |
| D5: Lange Reads/Mining unter einziger Verbindung | Mutex<Connection>, Stats-/Keywordarbeit während Guard                              | Config/Completion/OCR warten hinter großen Leseoperationen | Guardzeiten messen, kurze Snapshots und Berechnung getrennt                      | M       | P2        |
| D6: Globaler Accountcleanup je Historydelete     | Korrelierte Scans, lower(source) statt passender Suche                             | Ungünstig Accounts × History                               | Nur betroffene Identität prüfen, Normalisierung angleichen                       | S–M     | P2        |
| D7: SHAindizes ohne Produktquery                 | Gespeichert/validiert, kein aktueller Lookup nach SHA                              | Zusätzlicher Write-/Platzaufwand, nicht gemessen           | Erst Nutzung/Kosten messen, dann gegebenenfalls versioniert entfernen            | S–M     | P3        |
| D8: Gesamtgröße/Backups/Receipts wachsen         | Bewusste Persistenz, keine Retention/VACUUM-Regel                                  | Später Speicher-/Upgradebudget überschritten               | Transparente Größen-/Wiederherstellungsregeln nach Messung                       | M       | P3        |

**D1 — Query-/Indexentscheidung.** Nutzer: große Recherchearchive/Power-User. Akzeptanz: ungefilterte Page ohne temporäre Sortierung, Accountlookup als gezielte SEARCH; stabile Reihenfolge auch bei NULL completed_at und gleichen Zeiten; keine Lücken/Doppelungen beim Weiterblättern; COUNT und Filter stimmen. Keyset muss alle drei Sortierschlüssel enthalten. Führende %-Teilstrings werden durch einen normalen B-Tree nicht gelöst; FTS erst nach Entscheidung über gewünschte Teilwort-/Token-Semantik. Quelle: history.rs:94; database.rs:926; db-probe-results.json. Aufwand M/P1.

**D2 — Accountidentität.** Nutzer: Rechercheure/Creator. accounts.rs:129 gruppiert exakt nach source/uploader; :215 verwendet einen bereits gespeicherten Profilpfad desselben Labels vor Auflösung des neuen Videos. URL-/Domain-/Staleguards verhindern nicht die Verwechslung zweier gleicher Labels. Akzeptanz: Zwei gleich benannte Kanäle getrennt, Umbenennung bleibt dieselbe Identität, unaufklärbare Altbestände bleiben ungeklärt statt geraten. Sichere Migration: neue nullable Accountreferenz/Stammtabelle, belegbares Backfill ohne Netzabfragen in Migration, Snapshots konservativ erhalten. Quelle: database.rs:137; history_rules.rs:23; accounts.rs:215. L/P1.

**D3 — Suchsemantik.** Nutzer: alle nicht-ASCII-Namen, besonders deutschsprachige Recherche. Isoliert bestätigtes Verhalten des tatsächlichen LIKE-Filters aus history.rs:96. Akzeptanz: Österreich/österreich und entsprechende Unicodefälle konsistent; %, _ und Backslash weiterhin literal; exakte Accountidentität bleibt separat. OCR-UDF pinefetch_contains zeigt bereits einen Ansatz, database.rs:259. Eine UDF kann Semantik vereinheitlichen, löst aber keine Vollscan-Skalierung. S–M/P2.

**D4 — Schemafrüherkennung.** database.rs:597–638 fordert Erweiterungstabellen nur bis Accounts; die anschließende Schleife :646 prüft vorhandene Objekte. history_subtitles ist ab 8 nötig; history.rs:294 fragt sie bei Details voraussetzungslos ab. Nachweis aus Code, kein Sourcepatch und kein provozierter Produktivstart. Akzeptanz: isolierte Schema-8/9/10-DB ohne diese Tabelle wird früh und unverändert verweigert; intakte historische/current DBs bleiben gültig; keine automatische Neuerstellung verlorener Untertitel. S/P2.

**D5 — DB-Guardbudget.** database.rs:1196 serialisiert Zugriffe korrekt. keywords.rs:91 verarbeitet bis 128 MiB/50.000 Terme während desselben Guards; history.rs:362 lädt nach Summen weitere Quellenzeilen zur Rustnormalisierung. Nutzer: parallele Analyse-/Downloadarbeit. Akzeptanz: gemessene Guardwartezeiten unter großen Textlasten, weiterhin konsistente Quellenrevision; Completion/Config bleiben bedienbar. Begrenzte Snapshotreads plus außerhalb ausgeführte CPU-Arbeit prüfen; keine unbeschränkte Vollkopie als vermeintlicher Fix. Readconnection/WAL nur nach getesteten Konsistenz-/Schemalifetime-Regeln. M/P2, derzeit Risiko statt gemessene UI-Blockade.

**D6 — Cleanup.** history.rs:604/:614 löscht verwaiste Accountsnapshots über alle Snapshots; EXPLAIN zeigt verschachtelte Scans. Akzeptanz: geteilte Snapshots bleiben bis zum letzten passenden Download, Einzellöschung prüft nur betroffene Identität mit passendem Index; Clear bleibt atomar. D2 kann die Identitätsseite sauber lösen. S–M/P2; kein aktueller Cleanup-Latenzbenchmark.

**D7 abgeschlossen für 2.7.0:** Beide SHA-Indizes bleiben bewusst erhalten. Eine isolierte Schema-10-SQL-Probe mit 100.000 History-/Caption-/Receiptgruppen ergab 216.793.088 B mit gegenüber 200.093.696 B ohne diese Indizes. Bei 2.000 einzelnen Completion-ähnlichen Transaktionen lag der Median bei 1.687,297 gegenüber 1.414,079 ms, also rund 0,137 ms pro Gruppe Differenz. Es gibt reale Kosten; ein Engpass des tatsächlichen Downloadworkflows ist damit nicht nachgewiesen. Produktlesepläne blieben gleich, beispielhafte externe SHA-Lookups verlieren ohne Index ihre gezielte Suche. Die Objekte gehören zum validierten veröffentlichten Schema; eine Entfernung würde Migration, Pflichtbackup und Kompatibilitätswechsel verlangen. Für diese Bereinigung überwiegt der Erhalt dieses Vertrags. Quelle: /tmp/pinefetch-dead-code-270.ykksdd/sha-index-results.json; 18 synthetische SQL-Läufe, kein Appbenchmark.

**D7/D8 — Wartung.** Hashwerte selbst werden tatsächlich gebraucht und bleiben erhalten. Nur Indizes sind mögliche Vereinfachung, MEDIUM CONFIDENCE bezüglich Entbehrlichkeit; externe SQL-Nutzung unbekannt. Receipts überleben Historydelete absichtlich und dürfen nicht blind als Müll gelöscht werden. Akzeptanz: belastbare Index-/Größenmessung und dokumentierte Wiederholungs-/Recoverygarantie vor Entfernung/Retention; aktueller Layoutvalidator gemeinsam versionieren. P3.

### Skalierung: Messung von Prognose trennen

| Größenordnung | Gemessener Anteil                                     | Prognose über reale Medien-/Textarchive                                                                                                               |
| ------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| 100 Downloads | Kurze Metadaten günstig                               | Wenige lange OCR-Videos können dennoch den Textspeicher dominieren. Keine Server-DB begründet.                                                        |
| 10.000        | Metadaten rund 2,4 MB, tiefe Seite rund 19 ms         | Textmenge/Frames pro Video wichtiger als bloße Downloadzahl; lange Reader/Keywords messen.                                                            |
| 100.000       | Metadaten rund 23 MB, erste Seite 69 ms, tiefe 285 ms | Transcript-/Captionsuche und Stats-/Cleanupscans können über den Metadatenbenchmark hinaus wachsen.                                                   |
| 1.000.000     | Metadaten rund 231 MB, tiefe Seite 2,26 s             | Querypfade/Keyset werden wichtig; OCR/Transcripts, Indizes und Backups bestimmen I/O und Upgradebudget. Kein Beweis einer prinzipiellen SQLitegrenze. |

Nach Datenart:

- **Suche:** Titel-/User-/Beschreibung-/Transcriptsuche mit führendem % skaliert über zu untersuchende Texte; OCR-Unicode-UDF linear über Frames eines Videos. Noch keine gemessene globale Volltextmillion.
- **History:** Lazy-Details und 20er-Seiten sind gut; Scan/Sort und wiederholtes COUNT werden dadurch nicht automatisch günstig.
- **OCR:** Frames vernünftig paginiert/indexiert, blocks_json nicht in jeder Listenpage. Große Ersatztransaktionen, Volltextexport und Suchcounts wachsen mit Frame-/Textmenge.
- **Transcripts/Captions/Subtitles:** Volltext wird offline in SQLite bewahrt; große Einzeltexte werden vollständig gelesen. Subtitlegrenzen: acht MiB je Track, 32 MiB gesamt. Keywordgrenze 128 MiB Input/50.000 akzeptierte Terme; keine globale Archivgrenze.
- **Social Insights:** Maximal 256 KiB Snapshotpayload je abgerufenem Video/Account; nur tatsächlich geladene Snapshots belegen Platz. JSON zweckmäßig; kein automatisches Zeitreihenwachstum.
- **Accounts:** Weniger Snapshots bei vielen Videos desselben Creators, derzeit jedoch problematische Labelidentität. Lookup-/Cleanupstrategie wichtiger als vorsorgliche JSONnormalisierung.
- **Writes/Wartung:** Prepared Inserts und kurze Transaktionen helfen; große OCRpublikation und Backups beanspruchen trotzdem Writer/Platte. Drei Sekunden Locktimeout bzw. 30 Sekunden Backupdeadline können bei großen/langsamen Archiven kontrollierte Fehler erzeugen. Kein Datenverlust daraus nachgewiesen.

**Reine Größenannahme, keine Messung:** Durchschnittlich ein MiB OCR-Payload je vollständig analysiertem Video ergäbe bei 100/10.000/100.000/1.000.000 Videos zusätzlich 100 MiB / 9,77 GiB / 97,66 GiB / 976,56 GiB. Durchschnittlich 20 KiB Transcript ergäbe 1,95 MiB / 195 MiB / 1,91 GiB / 19,07 GiB. Medien, Indizes, Backup und temporäre Dateien kommen hinzu. Reale Verteilung und Analysequote sind unbekannt; nicht aus diesen Annahmen Produktgrenzen ableiten.

### Grundregeln einer sicheren künftigen Migration

Neue Indizes/Tabellen/Referenzen additiv und versioniert einführen; Referenzlayout, Pflichtobjekte, FK-/Indexvalidator und historische Fixturekonstruktoren gemeinsam pflegen. Vorhandene Pflichtbackups, Immediate-Transaktion, Integritätskontrolle und Rollback erhalten. Keine Netzwerkauflösung während Migration. Neue Account-/Medienreferenzen nur nachweisbar backfillen; unbekannte Werte bewusst nullable lassen. Bei Tabellenneubau Counts und Inhalte vor Entfernen des Originals verifizieren. Lock-/Platz-/Backupfehler führen zu erhaltener alter DB, nicht zu stiller Teilmigration.

## 8. Drei neue Produktfunktionen

Die folgenden **exakt drei** Vorschläge sind strategische Produktempfehlungen, keine Fehler und keine bereits zugesagte Implementierung. Die Abwesenheitsprüfung umfasste Navigation, src/tauri-client.js:3, alle registrierten Commands in main.rs:131, aktuelles Schema und eigene Produkt-/Dokumentationssuche. Vorhandene lokale Reader, Keywords, Captioncopy, Queue und aktuelle Social-Snapshots wurden ausdrücklich berücksichtigt.

### Feature 1 — Lokaler Medienimport

**Problem:** Eigene Interviews, Bildschirmaufnahmen, SD-Kartenmaterial und vorhandene lokale Audiodateien können die teure Text-/OCRpipeline nicht als normale Eingangsquelle nutzen. DownloadRequest benötigt HTTP(S); pick_txt_file importiert Links, open_file_path öffnet lediglich vorhandene Dateien. OCR ist an Historyeinträge gebunden, ocr_store.rs und ocr_source.rs.

**Zielgruppe:** Journalisten, Forscher, Podcaster und Content-Creator mit eigenem oder berechtigt vorhandenem Material.

**User Story:** Eine Journalistin importiert ein lokales Interviewvideo, wählt Referenzieren oder Kopieren, erhält ein Transkript mit Zeiten und gegebenenfalls OCR, prüft Stellen im Player und verwendet Keywords. Das Ergebnis landet zusammen mit Onlinequellen in der vorhandenen History.

**Funktionsweise:** Datei-/Ordnerauswahl oder Drag-and-drop für lokale Medien, klare Besitz-/Speicherentscheidung, Analyseoptionen und dieselben Fortschritts-/Abbruchzustände wie vorhandene Jobs. Keine erfundene Online-URL für lokale Quellen.

**Integration:** ffprobe, Hashing, Dateibesitzregeln, Queue/Prozessverwaltung, Whisper, Vision, History, Player, Textreader und Keywords.

**Technik:** Eigener validierter Intake-/Jobtyp, Quellidentität und nullable Onlineherkunft, explizite Artefaktreferenzen aus E2, sichere Pfad-/Dateirechte, idempotente Importabschlüsse. Audio braucht nachvollziehbar andere Analyseoptionen als Video.

**Aufwand:** L. **Risiken:** Große Dateien, fehlende referenzierte Quelle, Format-/Rechteunterschiede, Doppelimporte und Datenschutz lokaler Interviews. Originale grundsätzlich nicht überschreiben; Kopie/Referenz für Nutzer verständlich.

**Nutzen:** Erweitert das Werkzeug unmittelbar auf eigene Produktionen, weitgehend unabhängig von Plattformdrift oder Zugriffshürden. Das ist ein neuer Eingangsworkflow, keine bloße Verbesserung des bereits vorhandenen TXT-Linkimports.

### Feature 2 — Recherche-Dossiers mit belegbaren Fundstellen

**Problem:** PineFetch speichert und analysiert einzelne Quellen; eine fertige Recherche mit mehreren Videos, ausgewählten Zitaten und Notizen muss außerhalb manuell zusammengesetzt werden. Es gibt keine persistenten Projekt-/Dossier-/Annotationstabellen oder entsprechenden Commands.

**Zielgruppe:** Rechercheure, Redaktionen, Wissenschaftler und Creator bei thematischen Produktionen.

**User Story:** Ein Rechercheur ordnet zwölf Videos einem Dossier zu, markiert fünf Transcript-/OCRstellen, kommentiert Widersprüche und exportiert ein offline nachvollziehbares Quellenpaket. Jede Stelle öffnet die zugehörige Quelle und lokale Zeitposition.

**Funktionsweise:** Neues Dossierobjekt mit Mitgliedsquellen, ausgewählten Belegen und Notizen; exportierbarer Bericht mit Original-URL, Hash, Zeitbezug und Analyseversion. Änderungen/fehlende Quellen werden sichtbar statt Zitate still umzudeuten.

**Integration:** History-IDs, bestehende SHA-Werte, Playerzeitnavigation, OCRrevisionen, gespeicherte Transcripts/Subtitles und lokale SQLitepersistenz.

**Technik:** Dossier-/Mitglieds-/Belegtabellen, stabile Belegidentitäten und gespeicherter Zitatausschnitt, Referenz- und Snapshotregeln bei Delete/Rerun, Exportmanifest und wählbare lokale Assets. Keine Cloud nötig.

**Aufwand:** L. **Risiken:** Sensible Notizen, Quelle-/Analyseänderungen, Löschsemantik, portable Exportgröße und korrekte Zeitlinien. Ein Dossier darf nicht auf bloß flüchtige sichtbare Zeilen verweisen.

**Nutzen:** Verbindet vorhandene Analyseergebnisse zu einem fertigen redaktionellen Arbeitsergebnis. Dies geht über E8s Sprung zur einzelnen Fundstelle hinaus.

### Feature 3 — Zeitliche Beobachtung öffentlicher Quellen

**Problem:** Social Insights speichert aktuell jeweils den letzten Snapshot; Refresh ersetzt ihn. Es gibt weder eine Beobachtungsliste noch abrufübergreifende Zeitreihen oder Profilvideo-Deltas. social_insights.rs:175 und docs/SOCIAL_INSIGHTS.md:4 beschreiben den aktuellen manuellen Snapshotworkflow.

**Zielgruppe:** Journalisten, Creator und Analysten, die öffentliche Entwicklungen über Tage nachvollziehen wollen.

**User Story:** Ein Nutzer nimmt einen öffentlichen Post oder Creator bewusst in eine Beobachtungsliste auf, erfasst später weitere zulässige öffentliche Zustände und sieht Metrikänderungen, neue öffentliche Videoreferenzen oder nicht erreichbare Quellen mit Zeit-/Fehlermarkierung.

**Funktionsweise:** Neuer opt-in Beobachtungsworkflow mit begrenzten manuellen und optional zeitgesteuerten Erfassungen, unveränderlichen Capturepunkten, Vergleich und Export. Fehlende Messung bleibt fehlend; kein versprochenes vollständiges Accountarchiv.

**Integration:** Verifizierte Profil-/Videoidentitäten, Fetchary/Chromium, vorhandene Social-DTOs, Quellenvalidierung und Prozessverwaltung.

**Technik:** Append-only Captures und Watchobjekte, begrenzter Scheduler/Worker, Sleep-/Resumeverhalten, Rate-Limits/Backoff, deduplizierte öffentliche Referenzen, Disk-/Netzbudget und Provenienz. Persönliche Cookies nicht ungefragt in Fetchary übernehmen.

**Aufwand:** L–XL. **Risiken:** Plattformdrift, Zugangsbeschränkungen, unvollständige Profile, Laufzeit/Netzlast, große Zeitreihen und Missinterpretation ausgefallener Abrufe.

**Nutzen:** Beantwortet die bisher ungelöste Frage nach Veränderungen über Zeit. Das ist ein neues Beobachtungsprodukt, keine bloße Aufhübschung vorhandener Snapshotkarten.

### Vergleich und strategische Empfehlung

Skala jeweils 1–10; bei Aufwand und Risiko bedeutet höher ungünstiger. Die Werte sind begründete Produkturteile, keine Messungen oder berechnete Wirtschaftlichkeitsprognosen.

| Ranking | Funktion                     | Nutzerwert | Strategischer Produktwert | Technischer Aufwand | Implementierungsrisiko |
| ------: | ---------------------------- | ---------: | ------------------------: | ------------------: | ---------------------: |
|       1 | Lokaler Medienimport         |          9 |                         9 |                   6 |                      4 |
|       2 | Recherche-Dossiers           |          9 |                         9 |                   7 |                      5 |
|       3 | Zeitliche Quellenbeobachtung |          8 |                         8 |                   8 |                      7 |

**Als nächsten größeren Produktschritt empfehle ich genau eine Funktion: lokalen Medienimport.** Sie eröffnet eigene Interviews und Produktionen für mehrere Nutzergruppen und verwendet vorhandene lokale Kernkompetenzen. Sie benötigt weniger neue Netz-/Plattformabhängigkeiten als Beobachtung und löst ein breites, sofort verständliches Problem. Vorher sollten E1/E2 und die wichtigsten P1-Zuverlässigkeitsarbeiten abgeschlossen werden. Dossiers sind strategisch attraktiv, profitieren aber ebenfalls erst von stabiler Quellen-/Artefaktidentität.

## 9. Priorisierte Entwicklungs-Roadmap

### P0

Kein bestätigter P0. Die Untersuchung rechtfertigt weder einen Alarm wegen ungesicherter SQLite-Migrationen noch einen vollständigen Architekturwechsel. Neue Evidenz für automatischen Datenverlust, Geheimnisweitergabe oder einen schweren Sicherheitsfehler würde die Einstufung ändern.

### P1

| Reihenfolge | Arbeit                                        | Nutzen / Risiko / Evidenz                                                    | Aufwand |
| ----------: | --------------------------------------------- | ---------------------------------------------------------------------------- | ------- |
|           1 | E1 portable Whisper-Runtime                   | Kernfähigkeit auf Zielgeräten; Linkage bewiesen, Clean-host-Abnahme fehlt    | L       |
|           2 | E2 explizite Video-/Transkriptreferenz        | Reproduzierter Kernworkflowfehler, Voraussetzung für sichere weitere Analyse | M–L     |
|           3 | E3 dauerhafte Queue und gewählter Modus       | Reproduzierter Arbeitsverlust bei normalem Neustart, besonders große Imports | M–L     |
|           4 | E4 Löschbestätigung                           | Breite Wirkung, kleiner Eingriff, schützt wertvolle Resultate                | S       |
|           5 | D2 stabile Creatoridentität                   | Hohe Bedeutung für richtige Quellenzuordnung; Modellkette bewiesen           | L       |
|           6 | E5 bewusste Playlistübernahme                 | Breite Fehlbedienungsmöglichkeit, vorhandener automatischer Pfad eindeutig   | M       |
|           7 | E6 präzise/transparent definierte Schnittzeit | Reale Abweichung, wichtig für Player-/Textausrichtung und Export             | M       |
|           8 | D1 Querypfade/Sortierindex/Keyset             | Messbar relevant bei großen Archiven; semantisch sorgfältige Migration       | M       |
|           9 | E7 handlungsfähige Inlinefehler               | Hilft allen Nutzern; bestehende sichere Fehlergrenzen erhalten               | S–M     |

Praktische Lieferfolge darf kleine sichere Patches vorziehen: E4 muss nicht auf das Ende einer großen Runtime- oder Accountmigration warten. Identitäts-/Artefaktänderungen sollten zusammen geplant, aber als unabhängig überprüfbare Migrationen geliefert werden.

### P2

E8 Keywords mit Belegen/Untertiteln; E9 Browserdiagnostik; E10 TXT-Importabschluss; E11 Cachebudget; E12/E13 Kompatibilitätskommunikation; E14 begrenzter Streamingabschluss; E15 Readerlast; E16 redundante Reads/Dokumentation. Dazu D3 Unicode-Suche, D4 Pflichtschemaobjekte, D5 Guardbudget und D6 punktueller Accountcleanup. Innerhalb dieser Gruppe zuerst kleine nachgewiesene Fehler, danach messpflichtige Performancehypothesen.

### P3

Die Dead-Code-/CSSbereinigung ist in 2.7.0 abgeschlossen; SHAindizes wurden geprüft und bewusst erhalten. Offen bleiben langfristige Speicher-/Backup-/Receiptregeln. Kein Grund, funktionierende native Integration, historische Fixtures oder Vendorcode vorschnell zu entfernen.

### Quick Wins

Mindestens fünf belastbare kleine Schritte existieren:

1. E4: Einzel-Historydelete bestätigen und tatsächlichen Löschumfang für Einzel-/Clearaktion erklären.
2. D4: history_subtitles in die versionabhängige Pflichtobjektprüfung aufnehmen, gezielte Integritätsregression.
3. E13: Unbekannte gültige Hosts neutral zur manuellen Metadatenprüfung führen.
4. E16: Bereits geladene Account-Insights verwenden statt zweiter identischer Pageabfrage — in 2.7.0 erledigt.
5. E15: OCR-Suche kurz entprellen, ohne den globalen Suchumfang einzuschränken.
6. E16: Defekte lokale Entwicklungs-/Integritätsdokumentationslinks berichtigen.
7. E7: Zunächst die häufigen Metadaten-/Enqueuefehler direkt am Composer anzeigen.

Diese Kandidaten sind keine pauschal eintägigen Zusagen. Besonders bei E7 hängt der Umfang davon ab, welche sichere Fehlerkategorisierung im Backend erforderlich wird.

### Größere technische Maßnahmen und technische Schulden

- **Artefakt- und Quellenidentität:** E2, D2 und E6 zusammen modellieren. Dateiname, Anzeigename und angeforderter Zeitoffset sind keine ausreichende dauerhafte Identität.
- **Unterbrechbare, wiederaufnehmbare Arbeit:** E3 auf vorhandene gute Completion-/Dateibesitzgarantien aufbauen; E14s Streamingabschluss gleichzeitig gezielt härten.
- **Messbare DB-Leseverträge:** D1 zuerst, danach D5/D6 mit echter Guard-/Lastmessung. SQLite beibehalten; mehr Verbindungen/WAL oder Suchindex nur aufgrund konkreter Messungen und Semantik.
- **Releaseabnahme:** E1 plus unveränderter nativer OCR-Smoke auf frischem Profil und unterstützten Zielsystemen. Die anfänglichen Vision-Timeouts und Test-/Produktionsdeadline separat beobachten.
- **Speicherlebenszyklus:** E11 und D8 gemeinsam inventarisieren: Originalmedien, Exporte, Playerderivate, SQLite und Backups haben unterschiedliche Besitz-/Wiederherstellungsregeln.
- **Danach ein neuer größerer Produktschritt:** lokaler Medienimport aus Abschnitt 8. Keine zusätzlichen neuen Module zwischen diese Zuverlässigkeitsarbeiten schieben.

## 10. Abschließende Bewertung

**Was funktioniert bereits besonders gut?** Die Anwendung verarbeitet Medien tatsächlich lokal und speichert nutzbare Ergebnisse. Die Kombination aus robusten SQLite-Upgrades, atomaren Completionwrites, vorsichtiger Dateiveröffentlichung, revisionssicherer OCR und brauchbaren Readern ist eine starke Basis. Die positiven nativen Download-/Audio-/Transcript-/OCR-/Playerabläufe stützen das über statische Betrachtung hinaus.

**Was ist noch nicht vollständig durchdacht?** Die Grenzen zwischen Datei, Historyeintrag und tatsächlicher Medienquelle; Queuearbeit über Neustarts; Accountidentität; genauer Schnittzeitbezug sowie der Nachweis einer wirklich unabhängigen Runtime. Weitere Lücken betreffen den letzten Schritt von Analyse zu nutzbarer Fundstelle und handlungsfähige Fehlerhilfe.

**Was sollte als Nächstes verbessert werden?** Runtime-Portabilität und die reproduzierten Medienzuordnungs-/Queuefehler zuerst; kleine Lösch-/Fehlerhilfe parallel. Danach stabile Creatoridentitäten, korrekte Schnittdefinition und die gemessenen Historyabfragen. Diese Schritte erhöhen den Wert vorhandener Funktionen stärker als eine lange Liste zusätzlicher Module.

**Was sollte vorerst unverändert bleiben?** Lokale Speicherung ohne Cloudzwang, explizite Socialabfragen mit Snapshotretention bei Fehlern, strenge Migrations-/Backup-/Dateibesitzregeln, konservative Legacyzuordnung, originale Creatoruntertitel und OCRprovenienz. Die vorhandenen Module und direkten konkreten Komponenten rechtfertigen keinen generischen Frameworkrewrite.

**Was ist die wichtigste technische Entscheidung für die Zukunft?** Stabile Identitäten und explizite Beziehungen für Quellen, Artefakte und Analysezeitlinien etablieren, während SQLite und die bestehenden Transaktionsgarantien erhalten bleiben. Dadurch werden bestehende Workflows zuverlässig und der empfohlene lokale Medienimport später wesentlich sicherer.

**Abschlusskontrolle der ursprünglichen Auditaufnahme:** Alle vier Aufgaben waren behandelt; die Testmatrix deckt sämtliche inventarisierten Bereiche ab und kennzeichnet Grenzen. Genau drei neue Produktfunktionen stehen in Abschnitt 8. Während der ursprünglichen Auditaufnahme wurden keine Produktdateien geändert und keine Commits/Pushes erstellt; der damalige SHA-256-Vergleich bestätigte alle 361 vorhandenen getrackten Dateien unverändert. Die anschließend ausdrücklich beauftragte Implementierung für 2.7.0 umfasst nun die oben vermerkte Bereinigung. Die Prüfbelege liegen im temporären Auditverzeichnis.

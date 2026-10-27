# PineFetch – Todo

Stand: v2.7.0. Offene Aufgaben in grober Prioritätsreihenfolge; die bisherigen Kennungen bleiben erhalten.

## Zuerst

- [ ] **D2 — Creator über stabile Identitäten zuordnen:** Gleich benannte Creator können derzeit vermischt werden, während eine Umbenennung dieselbe Quelle aufteilen kann. Stabile Plattformidentitäten halten Videos und gespeicherte Insights beim richtigen Creator.

- [ ] **E5 — Playlists erst nach bewusster Bestätigung übernehmen:** Ein eingefügter Link oder der Fensterfokus kann derzeit unbeabsichtigt eine ganze Playlist starten. Eine Vorschau mit Auswahl zwischen Einzelvideo und Playlist gibt Nutzern Kontrolle über Downloads, Speicher und Rechenzeit.

- [ ] **E6 — Schnittanfang, Dauer und Zeitangaben zuverlässig machen:** Der gewünschte Startpunkt wird beim schnellen Schnitt nicht immer genau eingehalten, und die gespeicherte Dauer kann weiterhin vom Original stammen. Verlässliche Schnittdaten halten Video, Untertitel, Transkript und OCR zeitlich zusammen.

- [ ] **D1 — History- und Accountabfragen für große Archive beschleunigen:** Große Bestände verursachen bereits messbare Wartezeiten beim Laden und Blättern. Passende Abfragen, Indizes und eine stabile Seitennavigation halten das Archiv auch bei vielen Downloads bedienbar.

- [ ] **E7 — Fehler direkt bei der betroffenen Aktion erklären:** Manche Fehler sind nur im Settings-Log sichtbar und lassen Nutzer ohne verständlichen nächsten Schritt zurück. Hinweise mit Wiederholen, Einstellungen öffnen oder Zielordner wählen erleichtern die Behebung ohne technische Logsuche.

- [ ] **Release auf einem sauberen Mac prüfen:** Die bestandene Sandbox-Abnahme ersetzt keinen Test auf einem separaten Mac ohne Entwicklerinstallation. Eine Prüfung auf unterstützten macOS-Versionen mit Transkription und OCR zeigt, ob das ausgelieferte Paket auch dort zuverlässig funktioniert.

## Danach

- [ ] **E8 — Keywords mit Fundstellen verbinden und Untertitel einbeziehen:** Gezählt wird bereits vorhandener Text, aber Nutzer können nicht direkt zur passenden Stelle springen und Creator-Untertitel fehlen als Quelle. Verlinkte Fundstellen und klar getrennte Untertitelsprachen machen die Ergebnisse überprüfbar und vollständiger.

- [ ] **E9 — Browserzugriff verständlich prüfen und erklären:** Ein erfolgreicher öffentlicher Abruf beweist keine Anmeldung für geschützte Inhalte, und unterschiedliche Fehler erscheinen derzeit zu ähnlich. Präzise Rückmeldungen helfen bei Profil-, Rechte- und Netzwerkproblemen, ohne private Cookies offenzulegen.

- [ ] **E10 — TXT-Import mit Fortschritt, Zeilenfehlern und Abbruch ergänzen:** Bei großen Linklisten fehlen konkrete Fehler pro Zeile und die Möglichkeit, die weitere Übernahme zu stoppen. Fortschritt und gezieltes Wiederholen fehlerhafter Zeilen sparen Arbeit und verhindern unnötige Doppelimporte.

- [ ] **E11 — Wiedergabe-Vorschauen automatisch begrenzen:** Alte Vorschaukopien werden bisher nicht über ein Größen- oder Altersbudget bereinigt und können mit dem Archiv wachsen. Eine sichere Bereinigung unbenutzter Vorschauen hält Speicher frei und bewahrt Originaldateien sowie laufende Wiedergaben.

- [ ] **E12 — Unterstützte Profilvideos einheitlich behandeln:** Unterschiedliche Plattformregeln können gelieferte Videolinks verwerfen und ein Profil dadurch fälschlich leer erscheinen lassen. Passende Prüfungen und klare Hinweise unterscheiden echte leere Profile von nicht unterstützten Abrufen.

- [ ] **E14 — Gesammelte Prozessausgaben begrenzen:** Für vollständig eingelesene Prozessausgaben fehlt noch ein Gesamtbudget, obwohl Streaming-Abschluss und einzelne Streamingzeilen bereits begrenzt sind. Ein Limit während des Einlesens schützt den Arbeitsspeicher vor ungewöhnlich großen Toolausgaben.

- [ ] **E15 — Große Texte und OCR-Ergebnisse flüssig bedienbar halten:** Die OCR-Suche startet bei jeder Eingabe Arbeit, und lange Textansichten bauen sämtliche Zeilen gleichzeitig auf. Weniger Suchanfragen sowie gemessene Verbesserungen für große Texte können Wartezeiten reduzieren, ohne Treffer, Kopieren oder Zeitnavigation einzuschränken.

- [ ] **E16 — Zusätzliche Metadatenabrufe und defekte Dokumentationslinks bereinigen:** Queue-Vorschaubilder können weitere Abrufe derselben Metadaten auslösen, während einige Entwicklungsverweise ins Leere führen. Gemeinsam genutzte laufende Abrufe sparen Netzarbeit, und gültige Dokumentationslinks machen die beschriebenen Prüfungen nachvollziehbar.

- [ ] **D3 — Suche mit Umlauten und Unicode vereinheitlichen:** Unterschiedliche Groß- und Kleinschreibung bei Namen wie Österreich kann derzeit zu abweichenden Treffern führen. Einheitliche Suchregeln verhindern, dass bereits gespeicherte Inhalte übersehen werden.

- [ ] **D4 — Fehlende Untertiteltabellen früh erkennen:** Eine beschädigte Datenbank ohne erforderliche Untertiteltabelle wird nicht in jedem Fall schon beim Start erkannt. Eine vollständige Schemaprüfung meldet das Problem früh, statt Nutzer erst beim Öffnen eines Eintrags scheitern zu lassen.

- [ ] **D5 — Lange Datenbankzugriffe messen und verkürzen:** Große Keyword- und Statistikabfragen teilen sich den Datenbankzugriff mit Downloads und Einstellungen und können diese dadurch aufhalten. Messungen zeigen, welche Berechnungen außerhalb des gesperrten Zugriffs stattfinden sollten, ohne die Datenkonsistenz zu verlieren.

- [ ] **D6 — Accountbereinigung auf betroffene Einträge beschränken:** Beim Löschen eines History-Eintrags wird derzeit eine breite Suche nach verwaisten Accountdaten ausgeführt. Eine gezielte Prüfung reduziert unnötige Arbeit bei großen Archiven und erhält gemeinsam genutzte Insights bis zum letzten zugehörigen Download.

## Langfristig

- [ ] **D8 — Speicher-, Backup- und Aufbewahrungsregeln festlegen:** Datenbank, Backups und Wiederherstellungsnachweise wachsen dauerhaft, ohne eine gemeinsame Aufbewahrungsstrategie. Verständliche Größenanzeigen und sichere Regeln halten den Speicherverbrauch planbar, ohne benötigte Daten oder Wiederherstellungsmöglichkeiten vorschnell zu löschen.

## Optionale Ideen für später

- [ ] **Lokale Medien importieren:** Eigene Interviews und vorhandene Audio- oder Videodateien lassen sich bisher nicht wie heruntergeladene Medien analysieren. Ein lokaler Import öffnet Transkription, OCR und History auch für eigenes Material und ist der nächste empfohlene größere Produktschritt nach den wichtigen Fehlerkorrekturen.

- [ ] **Recherche-Dossiers mit Belegen erstellen:** Ergebnisse aus mehreren Videos müssen derzeit außerhalb von PineFetch zusammengeführt werden. Dossiers mit Zitaten, Notizen und Quellenverweisen würden aus einzelnen Analysen ein nachvollziehbares Rechercheergebnis machen.

- [ ] **Öffentliche Quellen über die Zeit beobachten:** Social Insights bewahrt derzeit den letzten Stand und zeigt keine Entwicklung über mehrere Abrufe. Eine bewusst aktivierte Beobachtung mit gespeicherten Zeitpunkten könnte Veränderungen nachvollziehbar machen und fehlgeschlagene Abrufe sichtbar halten.

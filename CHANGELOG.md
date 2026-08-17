# Changelog

## 1.0.0 — Erste öffentliche Version

Virtual MIDI Master 1.0.0 ist die erste veröffentlichte Version der App. Sie bietet eine lokale MIDI-Patchbay für macOS und Windows, bei der die Routing-Engine unabhängig von der Konfigurationsoberfläche weiterläuft.

### Basisfunktionen

- Mehrere systemweit sichtbare virtuelle MIDI-1.0-Ein- und Ausgänge erstellen und verwalten.
- Physische und virtuelle MIDI-Geräte in einer Many-to-many-Patchbay verbinden.
- MIDI-1.0-Daten einschließlich Note-, CC-, Pitch-Bend-, Aftertouch-, Clock-, System-Realtime- und SysEx-Nachrichten routen.
- Nachrichten mit Filtern sowie Kanal- und CC-Mappings gezielt anpassen.
- Effekte pro Gerät in definierter Reihenfolge anwenden, einschließlich Scale Helper und Sustain Helper.
- Routing-Konfiguration atomar und versioniert speichern; ungültige Dateien sichern und mit Standardwerten starten.
- Hintergrund-Engine mit lokalem, nur für den angemeldeten Benutzer zugänglichem IPC; die MIDI-Ports bleiben beim Schließen des Fensters verfügbar.
- Autostart nach der Anmeldung auf macOS und Windows ohne Administratorrechte.
- Live-Monitor für begrenzte, reaktionsfähige MIDI-Ereignisanzeige.

### Unterstützte Systeme

- macOS 12 oder neuer mit CoreMIDI.
- Windows 11 24H2 oder 25H2 x64 mit Windows MIDI Services und dem passenden App SDK Runtime.

### Nicht Teil von 1.0.0

MIDI-2.0-Authoring, LFOs, Sequenzaufzeichnung, Node-Editor, Windows 10 und öffentliche Code-Signierung sind nicht Bestandteil dieser ersten Version.

# Discord setup

## Empfohlen: Virtueller Bildschirm (Tab „Bildschirm“)

Damit Discord unter **Bildschirm / Screen** einen Monitor mit **Performance-Optionen**
zeigt, braucht Windows einen echten (virtuellen) Monitor:

1. Einmalig **Parsec Virtual Display Driver** installieren  
   (Button in kynxShare: *Parsec-VDD Installer öffnen*, oder  
   https://builds.parsec.app/vdd/parsec-vdd-0.41.0.0.exe )
2. In kynxShare **Virtueller Bildschirm = An**
3. **Stream starten** → kynxShare steckt einen virtuellen Monitor und legt das Output
   fullscreen darauf
4. Discord → **Bildschirm teilen** → Tab **Bildschirm** → neuen Monitor wählen  
   (Performance / Auflösung wie bei normalen Screens)

Beim Stream-Start wird die UI-Live-Preview automatisch ausgeschaltet (weniger CPU).
Du kannst sie jederzeit wieder einschalten.

## Fallback: Fenster

Wenn der Treiber fehlt:

1. Stream starten → Fenster **kynxShare Output**
2. Discord → Tab **Fenster / Anwendungen** → **kynxShare Output**

## Tipps

- Virtuellen Monitor nie in kynxShare als Capture-Quelle aktivieren (Feedback-Schleife).
- Live-Preview aus = spürbar mehr FPS für Capture/Compose.
- Treiber-Docs: https://github.com/nomi-san/parsec-vdd

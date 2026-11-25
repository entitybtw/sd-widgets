sd-widgets daemon for Windows
=============================

1. Unpack this folder anywhere.
2. Run sdw-daemon.exe.
3. The setup wizard opens. It shows the pairing password the Steam Deck needs.
4. Optional: install VIIPER from the wizard if you want virtual gamepads.
   The install page can also fetch ffmpeg and the Virtual Display Driver in one
   click: ffmpeg is unpacked into the app's data folder, the driver goes through
   winget (Windows may ask for Administrator there).
5. Keep the app running while you use the Deck. "Start with Windows" is in the GUI.

Ports: 47830/tcp (API), 47831/udp (gamepad), 47832/udp (discovery).
Windows Defender may ask for permission on the local network the first time.

The daemon does not need Administrator for notifications, Home Assistant or the
monitor stream. Installing USBIP/VIIPER drivers may ask for elevation.

Docs: https://git.entitybtw.ru/entitybtw/sd-widgets

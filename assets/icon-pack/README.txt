Inari - icon pack
=================

App ID: com.fbnlrz.inari

The torii artwork the app ships in src-tauri/icons, laid out as a freedesktop
icon theme for anyone packaging Inari themselves. Regenerate it from those
source SVGs rather than editing anything here by hand.

Layout (freedesktop hicolor theme):
  scalable/apps/com.fbnlrz.inari.svg           full-color SVG (use this everywhere it's supported)
  symbolic/apps/com.fbnlrz.inari-symbolic.svg  monochrome, follows the panel/tray text color
  hicolor/<size>/apps/com.fbnlrz.inari.png     rasters: 16, 24, 32, 48, 64, 128, 256, 512
  extras/tray-white-22.png                     tray glyph, dark panels
  extras/tray-black-22.png                     tray glyph, light panels

Install (per-user):
  cp -r hicolor/*   ~/.local/share/icons/hicolor/
  cp -r scalable/*  ~/.local/share/icons/hicolor/scalable/
  cp -r symbolic/*  ~/.local/share/icons/hicolor/symbolic/
  gtk-update-icon-cache ~/.local/share/icons/hicolor

In your .desktop file:
  Icon=com.fbnlrz.inari

Note: Inari's own .deb/.rpm install the launcher icon as "inari" (see
install.sh and the Tauri bundle). This pack is for third-party packaging and
desktop integration; the app does not read it at runtime.

# x86_64 libretro core set

Installed from Arch `extra` (see `payloads/astro.conf`), into `/usr/lib/libretro`.

| System | Core (package) |
|---|---|
| Arcade, Neo Geo, CPS 1-3 | MAME (`libretro-mame`) |
| SNES | Snes9x |
| Mega Drive / Master System / Game Gear / Sega CD | Genesis Plus GX; PicoDrive for 32X |
| NES | Nestopia |
| GB / GBC / GBA | mGBA |
| N64 | Mupen64Plus-Next |
| PS1 | Beetle PSX HW |
| Dreamcast / Naomi / Atomiswave | Flycast |
| PSP | PPSSPP |
| GameCube / Wii | Dolphin |
| PS2 | Play! (`libretro-play`) |

Not in the Arch repos, so not on the image yet (the ARM manifest has them):
FBNeo, DuckStation, PCSX2 (standalone on ARM). They would need a
`NATIVE_BUILDS` script like RetroArch's. ES-DE is also AUR-only; the x86
image has no frontend yet.

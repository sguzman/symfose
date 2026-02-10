# SoundFont Assets

Place SF2 files in this directory.

Bundled by default:

- `res/soundfonts/piano.sf2` (TimGM6mb)
- license/attribution: `res/soundfonts/piano.sf2.LICENSE`

If configured path is missing, Symfose also checks common Linux system paths:

- `/usr/share/sounds/sf2/FluidR3_GM.sf2`
- `/usr/share/sounds/sf2/TimGM6mb.sf2`
- `/usr/share/sounds/sf2/default-GM.sf2`
- `/usr/share/soundfonts/FluidR3_GM.sf2`
- `/usr/share/sf2/FluidR3_GM.sf2`

For best realism, prefer a dedicated piano SF2 or a high-quality GM SoundFont with a strong acoustic grand preset.

Current default profiles use this bundled GM file for:

- `piano` (`preset = 0`)
- `acoustic_guitar` (`preset = 24`)

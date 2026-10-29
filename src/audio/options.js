// The audio settings' choices, for the settings dialog (src/menu.js).
// Defaults are in src/settings.js with the other settings.
import { LAYOUTS } from "./layouts.js";
import { DYNAMICS } from "./spatial.js";

export const AUDIO_QUALITIES = [
  ["enhanced", "Enhanced (2026 sound stage)"],
  ["original", "Original (2004)"],
];

export const AUDIO_OUTPUTS = [["auto", "Automatic (from the device)"], ...Object.entries(LAYOUTS).map(([k, v]) => [k, v.label])];

export const AUDIO_DYNAMICS = Object.entries(DYNAMICS).map(([k, v]) => [k, v.label]);

export const AUDIO_VOLUMES = [
  ["masterVolume", "Master"],
  ["effectsVolume", "Effects"],
  ["musicVolume", "Music"],
  ["voiceVolume", "Voice"],
  ["ambienceVolume", "Ambience"],
  ["interfaceVolume", "Interface"],
];

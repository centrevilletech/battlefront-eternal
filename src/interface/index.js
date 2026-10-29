// The interface kit: what every 2004-style screen draws with, in a battle
// (the HUD, the spawn screen, the scoreboard, the pause menu) or on a page
// of its own (the front end). docs/menus.md describes it.
export { IW, PLANE, SAFE, SAFE_SCALE, interfaceHeight, interfaceCamera, anchorMatrix, planePoint } from "./space.js";
export { Batch } from "./batch.js";
export { Fonts } from "./fonts.js";
export { InterfaceSurface, commonTextureUrl } from "./surface.js";
export { nineSlice, wrapLines, insideQuad } from "./shapes.js";
export { TitleIntro, INTRO, INTRO_LENGTH } from "./flashy.js";
export { InterfaceCamera, interfaceMaterial, dressModel, dressBody, meshesOf, repeating, roomTints, LOGO_UNITS, BODY_UNITS, ROOM_SCALE } from "./models.js";
export { commonModel, loadCommonModels } from "./common-models.js";

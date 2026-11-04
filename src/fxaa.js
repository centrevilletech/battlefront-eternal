import { shaderSources } from "./render-shaders.js";
import { Vector2 } from "three";
const uniforms = { tDiffuse: { value: null }, resolution: { value: new Vector2(1 / 1024, 1 / 512) } };

// three's FXAA (NVIDIA's algorithm; C# implementation by Jasper Flick, GLSL
// port by Dave Hoskins; three.js addons, MIT), with the same result for
// fewer texture reads. three's reads all nine neighbours before it decides
// whether a pixel is on an edge at all, a choice the centre and its four
// direct neighbours alone make; most pixels aren't, and keep the centre
// colour. Here the four diagonals are read only for pixels on an edge, and a
// skipped pixel returns the centre read it already made: five reads instead
// of ten. On an old integrated GPU the pass cost more than the whole near
// scene's shading at the same size.
const fragmentShader = shaderSources["post-hud/fxaa"].fragment;

export const FXAAShader = { name: "FXAAShader", uniforms: uniforms, vertexShader: shaderSources["post-hud/fxaa"].vertex, fragmentShader };

// FXAA (FAST): FXAA 3.11's console path (Timothy Lottes, NVIDIA), as the
// PlayStation 3 and Xbox 360 ran it: the contrast of four diagonal reads and
// the centre, then a blur along the edge's direction from two pairs of reads,
// without the PC path's search along the edge. Nine reads a pixel and no
// loops: on an old integrated GPU about a third of the cost of FXAA above
// (Intel HD 2000 at 960x540: 2.9 ms against 7.6), a little softer on long,
// nearly horizontal or vertical edges.
const fastFragmentShader = shaderSources["post-hud/fxaa-fast"].fragment;

export const FXAAFastShader = { name: "FXAAFastShader", uniforms: uniforms, vertexShader: shaderSources["post-hud/fxaa"].vertex, fragmentShader: fastFragmentShader };

#ifdef FAR
// Linear fog by depth; additive surfaces fog toward black.
float fog = clamp((vDepth - fogRange.x) / (fogRange.y - fogRange.x), 0.0, 1.0);
#ifdef ADDITIVE
color.rgb *= 1.0 - fog;
#else
color.rgb = mix(color.rgb, fogColor, fog);
#endif
#else
float near = clamp((fade.y - vDepth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
#ifdef BLENDED
color.a *= near * near;
#else
// Most pixels are fully near: don't spend a far-scene texture read
// on every fragment unless this pixel is actually in the dissolve.
// An explicit level (the image has one) keeps the read inside the
// branch on Direct3D, whose compiler hoists implicit-gradient ones.
if (near < 1.0) color.rgb = mix(textureLod(farMap, gl_FragCoord.xy / resolution, 0.0).rgb, color.rgb, near);
color.a = 1.0;
#endif
#endif
#if defined( ALPHA_TEST ) && !defined( BLENDED )
color.a = coverage;
#endif
gl_FragColor = color;
#endif
}

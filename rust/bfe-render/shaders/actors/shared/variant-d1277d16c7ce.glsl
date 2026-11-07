#ifdef USE_LOGDEPTHBUF
	vFragDepth = 1.0 + gl_Position.w;
	vIsPerspective = float( isPerspectiveMatrix( projectionMatrix ) );
#endif
#if 0 > 0
	vClipPosition = - mvPosition.xyz;
#endif
	vHighPrecisionZW = gl_Position.zw;
}
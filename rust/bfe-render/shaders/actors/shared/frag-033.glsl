shadow -= (1.0 - sampleShadow) * weight;
}
cascadeShadow = directionalLightShadows[ 2 ];
interval = cascades[ 2 ];
weight = 1.0;

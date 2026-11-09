The synthetic pattern fixture contains five lossless I420 VP9 frames, 32 by 24
pixels at 10 fps. It contains no game assets. Recreate with:

```sh
ffmpeg -f lavfi -i 'testsrc2=size=32x24:rate=10:duration=0.5' -an \
  -c:v libvpx-vp9 -lossless 1 -pix_fmt yuv420p pattern.webm
```

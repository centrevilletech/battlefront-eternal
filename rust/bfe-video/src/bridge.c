/* Keep libvpx's ABI-owned structs in C; Rust only borrows displayed planes.
 * The image remains valid until the next decode call or decoder destruction. */
#include <stdlib.h>
#include <vpx/vpx_decoder.h>
#include <vpx/vp8dx.h>
void *bfe_vpx_new(void) {
    vpx_codec_ctx_t *ctx = calloc(1, sizeof(*ctx));
    if (!ctx) return NULL;
    vpx_codec_dec_cfg_t config = { 1, 0, 0 };
    if (vpx_codec_dec_init(ctx, vpx_codec_vp9_dx(), &config, 0)) {
        free(ctx);
        return NULL;
    }
    return ctx;
}
void bfe_vpx_free(void *ctx) {
    vpx_codec_destroy(ctx);
    free(ctx);
}
int bfe_vpx_decode(void *ctx, const unsigned char *data, unsigned int size,
                   unsigned char **planes, int *strides, unsigned int *size_out) {
    if (vpx_codec_decode(ctx, data, size, NULL, 0)) return -1;
    vpx_codec_iter_t iter = NULL;
    vpx_image_t *image = vpx_codec_get_frame(ctx, &iter);
    if (!image) return 0;
    if (image->fmt != VPX_IMG_FMT_I420) return -1;
    size_out[0] = image->d_w;
    size_out[1] = image->d_h;
    for (int i = 0; i < 3; ++i) {
        planes[i] = image->planes[i];
        strides[i] = image->stride[i];
    }
    return 1;
}

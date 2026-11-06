// The renderer's only C++ boundary. Basis retains a borrowed view of the KTX2
// bytes; Rust keeps that allocation alive until bfe_basis_close returns.
// There are no callbacks, GL calls or browser objects on this side.
// Decoder instances belong to one worker and are never shared between threads.
#include "../vendor/basis-universal/transcoder/basisu_transcoder.h"

using Decoder = basist::ktx2_transcoder;
extern "C" {
void bfe_basis_init() { basist::basisu_transcoder_init(); }
Decoder* bfe_basis_open(const uint8_t* bytes, uint32_t size) {
    auto* decoder = new Decoder;
    if (!decoder->init(bytes, size) || !decoder->start_transcoding()) {
        delete decoder;
        return nullptr;
    }
    return decoder;
}
void bfe_basis_close(Decoder* decoder) { delete decoder; }
uint32_t bfe_basis_width(const Decoder* d) { return d->get_width(); }
uint32_t bfe_basis_height(const Decoder* d) { return d->get_height(); }
uint32_t bfe_basis_levels(const Decoder* d) { return d->get_levels(); }
uint32_t bfe_basis_faces(const Decoder* d) { return d->get_faces(); }
uint32_t bfe_basis_layers(const Decoder* d) { return d->get_layers(); }
uint32_t bfe_basis_alpha(const Decoder* d) { return d->get_has_alpha(); }
uint32_t bfe_basis_premultiplied(const Decoder* d) { return d->get_dfd_flags() & 1; }
uint32_t bfe_basis_uastc(const Decoder* d) { return d->is_uastc(); }
// Let the library size compressed terminal levels, including PVRTC's minimum
// footprint. RGBA's capacity is in pixels, compressed capacity is in blocks.
uint32_t bfe_basis_size(uint32_t format, uint32_t width, uint32_t height) {
    return basist::basis_compute_transcoded_image_size_in_bytes(static_cast<basist::transcoder_texture_format>(format), width, height);
}
uint32_t bfe_basis_transcode(Decoder* d, uint32_t level, uint32_t layer, uint32_t face, uint32_t format, uint8_t* out, uint32_t bytes) {
    const auto target = static_cast<basist::transcoder_texture_format>(format);
    const auto block_bytes = basist::basis_get_bytes_per_block_or_pixel(target);
    // The former transcoder predates adaptive ETC1S chroma filtering. Disable
    // that newer BC7 refinement to retain its original authored block bytes.
    const uint32_t flags = d->is_uastc() ? 0 : basist::cDecodeFlagsNoETC1SChromaFiltering;
    return block_bytes && d->transcode_image_level(level, layer, face, out, bytes / block_bytes, target, flags, 0, 0, -1, -1);
}
}

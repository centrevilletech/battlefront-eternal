// The post pass (post.rs): the frame's last passes over the near scene's
// image, the composite (the soft-focus blur, bloom and speed blur), FXAA
// and the bilinear upscale to the window. Empty for now: three draws them.
export function postPass() {
  return { name: "post", phase: "post" };
}

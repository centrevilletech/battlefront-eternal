// Interface image IO retains dimensions for glyph/art layout and a native
// sampler id. Rust owns image storage, sampling, aliases and loss restoration.
import {pngImage} from '../texture-decode.js';
import {rendererFailure} from '../rust-render/failure.js';
const MAX_SIZE=16384;
export function whiteImage(renderer){const gpu=renderer.resources.gpu;return {interfaceImage:true,sample:gpu.ui_image(0),width:1,height:1,ready:true,revision:1,observers:new Set(),gpu};}
export function canvasImage(canvas){return {interfaceImage:true,canvas,sample:null,width:canvas.width,height:canvas.height,ready:true,revision:1,observers:new Set(),gpu:null};}
// Admission resolves a logical sample once. Numeric native assets and image
// IO records share this boundary, without a live texture-version scanner.
export function imageSample(resources,image){
 if(image==null)return -1;if(typeof image==='number')return image;
 if(image.interfaceAlias){if(image.repeat)resources.gpu.ui_repeat(image.sample);return image.sample;}
 if(!image.interfaceImage){const sample=resources.staticSample(image);if(image.interfaceRepeat)resources.gpu.ui_repeat(sample);return sample;}
 if(image.sample===null){image.gpu=resources.gpu;image.sample=resources.gpu.ui_canvas(image.canvas);image.canvas=null;if(image.repeat)resources.gpu.ui_repeat(image.sample);}
 return image.sample;
}
export function releaseImage(image){if(image?.interfaceImage&&image.sample!==null){image.gpu.dispose_sample(image.sample);image.sample=null;image.ready=false;}}
export function repeatImage(image){if(image==null)return image;if(typeof image==='number')return {interfaceAlias:true,sample:image,repeat:true};if(image.interfaceImage){image.repeat=true;if(image.sample!==null)image.gpu.ui_repeat(image.sample);}else image.interfaceRepeat=true;return image;}
// Observers run only when loading/atlas publication changes dimensions. The
// HUD registers its native asset rows there instead of polling every texture.
export function observeImage(image,observer){if(image?.interfaceImage){image.observers.add(observer);observer(image);}return ()=>image?.observers?.delete(observer);}
export async function publishImage(renderer,image,decoded,width=decoded.width,height=decoded.height){
 const resources=renderer.resources;resources.gpu.ui_png(image.sample,decoded.width,decoded.height,decoded.encoded,decoded.data);
 await resources.enqueueNative(image.sample,decoded.width*decoded.height*4);
 if(image.repeat)resources.gpu.ui_repeat(image.sample);
 image.width=width;image.height=height;image.ready=true;image.revision++;
 for(const observer of image.observers)observer(image);
}
// Failed decoding stops the owning renderer through its existing error path.
// A disposed renderer releases transport bytes without publishing new storage.
export class InterfaceImages {
 constructor(renderer){this.renderer=renderer;this.failure=null;}
 load(url,onLoad){const renderer=this.renderer,gpu=renderer.resources.gpu,image={interfaceImage:true,sample:gpu.ui_image(1),width:0,height:0,ready:false,revision:0,observers:new Set(),gpu};
  pngImage(url,Math.min(MAX_SIZE,renderer.capabilities.maxTextureSize)).then(async decoded=>{try{if(renderer.resources.disposed)return;await publishImage(renderer,image,decoded);await onLoad?.(image);}finally{decoded.release();}}).catch(cause=>{this.failure??=rendererFailure(renderer,cause);renderer.resources.failure=this.failure;});return image;
 }
}

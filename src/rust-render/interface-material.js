// Interface appearance inputs are semantic UI state. Rust owns defaults,
// catalog variants, samplers and raster policy; there is no Material shell.
import {ui_appearance_defaults} from '../rust-pkg/bfe_render.js';
import {UI_RESOURCE} from './interface-resource-layout.js';
export const LOOK=UI_RESOURCE.LOOK;
let identity=0;
export function surfaceMaterial(program,options={}){
 const {uniforms=null,map=null,mask=null,...authored}=options;
 const values={...authored.values};let image=map,maskImage=mask;
 // Existing cold room/projector constructors publish authored values only.
 // Their wrappers die here; a frame never looks up a uniform name or object.
 if(uniforms)for(const [name,entry]of Object.entries(uniforms)){const value=entry.value;if(name==='map'){image=value;continue;}if(name==='mask'){maskImage=value;continue;}if(name==='mapTransform'||name==='uvTransform'||name==='hasMap')continue;values[name]=typeof value==='number'||typeof value==='boolean'?[+value]:Array.isArray(value)?value:value.toArray();}
 const style={program,...authored,values};
 return {interfaceAppearance:true,id:identity++,authored:JSON.stringify(style),values:ui_appearance_defaults(JSON.stringify(style)),map:image,mask:maskImage,masked:maskImage!==null&&maskImage!==undefined};
}
export function interfaceBasic(options={}){return surfaceMaterial('basic',options);}
export function interfacePoints(options={}){return surfaceMaterial('points',options);}
// Colour animation writes generated scalar inputs directly. RGB values are
// already in the interface's gamma-space convention, as setRGB was before.
export function paint(look,field,r,g,b){const v=look.values;v[field]=r;v[field+1]=g;v[field+2]=b;}
export function paintArray(look,field,rgb){paint(look,field,rgb[0],rgb[1],rgb[2]);}

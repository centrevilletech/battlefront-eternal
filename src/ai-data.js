// Asset IO for cold native AI setup. Keep original JSON bytes alongside the
// page's parsed metadata so authored scalar forms survive native loading.
async function json(path) {
  const response=await fetch(path);if(!response.ok)throw Error(`${path}: ${response.status}`);
  const bytes=new Uint8Array(await response.arrayBuffer());
  return {value:JSON.parse(new TextDecoder().decode(bytes)),bytes};
}
async function bin(path) {
  const response=await fetch(path);if(!response.ok)throw Error(`${path}: ${response.status}`);
  return response.arrayBuffer();
}
export async function loadAIData(map,sides,weaponData=loadAIWeaponData(sides)) {
  const optional=promise=>promise.catch(()=>null);
  const [nav,mission,classes]=await Promise.all([
    optional(json(`/maps/${map}/ai-nav.json`)),optional(json(`/maps/${map}/ai-mission.json`)),
    weaponData,
  ]);
  const navBin=nav?.value?.bin?.file?await optional(bin(`/maps/${map}/${nav.value.bin.file}`)):null;
  return {nav:nav?.value??null,navBytes:nav?.bytes??null,navBin,mission:mission?.value??null,
    classBytes:classes};
}

// The page needs cooked targeting policy, but never navigation or bot state.
export function loadAIWeaponData(sides) {
  return Promise.all(sides.map(side=>bin(`/sides/${side}/ai.json`)
    .then(bytes=>new Uint8Array(bytes)).catch(()=>null)));
}
